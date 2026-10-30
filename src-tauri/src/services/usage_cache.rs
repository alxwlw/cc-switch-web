//! 托盘展示用的用量缓存（进程内、写穿式）。
//!
//! 各 usage 查询命令成功时写入；系统托盘构建菜单时读取。不持久化，
//! 进程重启即空，由下一次自动查询或托盘悬停触发的刷新重新填充。

use std::collections::HashMap;
use std::sync::RwLock;

use crate::app_config::AppType;
use crate::provider::UsageResult;
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;

#[derive(Default)]
pub struct UsageCache {
    subscription: RwLock<HashMap<AppType, SubscriptionQuota>>,
    codex_oauth: RwLock<HashMap<String, SubscriptionQuota>>,
    script: RwLock<HashMap<(AppType, String), UsageResult>>,
}

impl UsageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_subscription(&self, app_type: AppType, quota: SubscriptionQuota) {
        if let Ok(mut w) = self.subscription.write() {
            w.insert(app_type, quota);
        }
    }

    pub fn put_codex_oauth(&self, account_id: String, quota: SubscriptionQuota) {
        if let Ok(mut w) = self.codex_oauth.write() {
            w.insert(account_id, quota);
        }
    }

    /// Managed accounts must never share the CLI's app-wide subscription snapshot.
    pub fn with_codex_oauth<R>(
        &self,
        account_id: &str,
        f: impl FnOnce(&SubscriptionQuota) -> R,
    ) -> Option<R> {
        self.codex_oauth
            .read()
            .ok()
            .and_then(|r| r.get(account_id).map(f))
    }

    pub fn put_script(&self, app_type: AppType, provider_id: String, result: UsageResult) {
        if let Ok(mut w) = self.script.write() {
            w.insert((app_type, provider_id), result);
        }
    }

    /// 以借用形式暴露订阅快照，避免托盘每次重建时深拷贝整个 `SubscriptionQuota`。
    pub fn with_subscription<R>(
        &self,
        app_type: &AppType,
        f: impl FnOnce(&SubscriptionQuota) -> R,
    ) -> Option<R> {
        self.subscription
            .read()
            .ok()
            .and_then(|r| r.get(app_type).map(f))
    }

    /// 以借用形式暴露脚本型用量结果，同上。
    pub fn with_script<R>(
        &self,
        app_type: &AppType,
        provider_id: &str,
        f: impl FnOnce(&UsageResult) -> R,
    ) -> Option<R> {
        self.script
            .read()
            .ok()
            .and_then(|r| r.get(&(app_type.clone(), provider_id.to_string())).map(f))
    }

    pub fn invalidate_script(&self, app_type: &AppType, provider_id: &str) {
        // 热路径会对每个禁用脚本的 provider 在托盘重建时调用一次：先走读锁
        // `contains_key` 快速放行"本来就不在缓存里"的常见情况，避免无谓的写锁升级。
        let key = (app_type.clone(), provider_id.to_string());
        if !self.script.read().is_ok_and(|r| r.contains_key(&key)) {
            return;
        }
        if let Ok(mut w) = self.script.write() {
            w.remove(&key);
        }
    }

    pub fn invalidate_subscription(&self, app_type: &AppType) {
        if !self
            .subscription
            .read()
            .is_ok_and(|r| r.contains_key(app_type))
        {
            return;
        }
        if let Ok(mut w) = self.subscription.write() {
            w.remove(app_type);
        }
    }

    /// Drop all process-local usage snapshots after a database/provider restore.
    pub fn invalidate_all(&self) {
        if let Ok(mut subscriptions) = self.subscription.write() {
            subscriptions.clear();
        }
        if let Ok(mut scripts) = self.script.write() {
            scripts.clear();
        }
        if let Ok(mut accounts) = self.codex_oauth.write() {
            accounts.clear();
        }
    }
}

/// Which usage snapshot `publish_usage` writes through to the cache.
pub enum UsageKind {
    /// Script-provider result: `put_script` + emit
    /// `{kind:"script", appType, providerId, data}`.
    Script {
        app_type: AppType,
        provider_id: String,
        data: UsageResult,
    },
    /// Official-subscription quota: `put_subscription` + emit
    /// `{kind:"subscription", appType, data}` (no `providerId`).
    Subscription {
        app_type: AppType,
        data: SubscriptionQuota,
    },
    /// Codex OAuth quota: ONLY `put_codex_oauth`, NO emit (parity with the
    /// desktop Codex OAuth command, which never emitted).
    CodexOAuth {
        account_id: String,
        data: SubscriptionQuota,
    },
}

/// Shared publish tail for usage queries: write-through into `UsageCache` +
/// emit `usage-cache-updated` on the bus with the desktop-identical payload.
///
/// Emit-then-put order matches both desktop call sites it replaces
/// (`commands::provider::queryProviderUsage`,
/// `commands::subscription::get_subscription_quota`).
/// Tray refresh stays in the desktop command wrappers (it needs `AppHandle`,
/// which this handle-free function deliberately cannot touch).
pub(crate) fn publish_usage(state: &AppState, kind: UsageKind) {
    match kind {
        UsageKind::Script {
            app_type,
            provider_id,
            data,
        } => {
            state.events.emit(
                "usage-cache-updated",
                serde_json::json!({
                    "kind": "script",
                    "appType": app_type.as_str(),
                    "providerId": &provider_id,
                    "data": &data,
                }),
            );
            state.usage_cache.put_script(app_type, provider_id, data);
        }
        UsageKind::Subscription { app_type, data } => {
            state.events.emit(
                "usage-cache-updated",
                serde_json::json!({
                    "kind": "subscription",
                    "appType": app_type.as_str(),
                    "data": &data,
                }),
            );
            state.usage_cache.put_subscription(app_type, data);
        }
        UsageKind::CodexOAuth { account_id, data } => {
            state.usage_cache.put_codex_oauth(account_id, data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::services::subscription::CredentialStatus;
    use serial_test::serial;
    use std::sync::Arc;

    fn fake_quota() -> SubscriptionQuota {
        SubscriptionQuota {
            tool: "claude".to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers: vec![],
            extra_usage: None,
            reset_credits: None,
            credits_balance: None,
            error: None,
            queried_at: Some(0),
        }
    }

    fn fake_result() -> UsageResult {
        UsageResult {
            success: true,
            data: None,
            error: None,
        }
    }

    struct EnvGuard(Option<std::ffi::OsString>);
    impl EnvGuard {
        fn set(path: &std::path::Path) -> Self {
            let prev = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", path);
            Self(prev)
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    struct TestState {
        _env: EnvGuard,
        _home: tempfile::TempDir,
        state: AppState,
    }

    fn test_state() -> TestState {
        let home = tempfile::tempdir().unwrap();
        let env = EnvGuard::set(home.path());
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        TestState {
            _env: env,
            _home: home,
            state,
        }
    }

    #[test]
    #[serial]
    fn publish_script_writes_cache_and_emits_exact_payload() {
        let held = test_state();
        let state = &held.state;
        let mut rx = state.events.subscribe();
        let data = UsageResult {
            success: true,
            data: Some(vec![crate::provider::UsageData {
                plan_name: Some("pro".to_string()),
                remaining: Some(80.0),
                total: Some(100.0),
                used: Some(20.0),
                unit: Some("%".to_string()),
                is_valid: Some(true),
                invalid_message: None,
                extra: None,
            }]),
            error: None,
        };

        publish_usage(
            state,
            UsageKind::Script {
                app_type: AppType::Claude,
                provider_id: "p1".to_string(),
                data: data.clone(),
            },
        );

        assert!(state
            .usage_cache
            .with_script(&AppType::Claude, "p1", |r| r.success)
            .unwrap_or(false));
        let event = rx.try_recv().expect("usage-cache-updated event");
        assert_eq!(event.name, "usage-cache-updated");
        assert_eq!(
            event.payload,
            serde_json::json!({
                "kind": "script",
                "appType": "claude",
                "providerId": "p1",
                "data": serde_json::to_value(&data).unwrap(),
            })
        );
    }

    #[test]
    #[serial]
    fn publish_subscription_writes_cache_and_emits_without_provider_id() {
        let held = test_state();
        let state = &held.state;
        let mut rx = state.events.subscribe();
        let quota = fake_quota();

        publish_usage(
            state,
            UsageKind::Subscription {
                app_type: AppType::Codex,
                data: quota.clone(),
            },
        );

        assert!(state
            .usage_cache
            .with_subscription(&AppType::Codex, |q| q.success)
            .unwrap_or(false));
        let event = rx.try_recv().expect("usage-cache-updated event");
        assert_eq!(event.name, "usage-cache-updated");
        assert!(event.payload.get("providerId").is_none());
        assert_eq!(
            event.payload,
            serde_json::json!({
                "kind": "subscription",
                "appType": "codex",
                "data": serde_json::to_value(&quota).unwrap(),
            })
        );
    }

    #[test]
    #[serial]
    fn publish_codex_oauth_writes_cache_and_emits_nothing() {
        let held = test_state();
        let state = &held.state;
        let mut rx = state.events.subscribe();

        publish_usage(
            state,
            UsageKind::CodexOAuth {
                account_id: "acc-1".to_string(),
                data: fake_quota(),
            },
        );

        assert!(state
            .usage_cache
            .with_codex_oauth("acc-1", |q| q.success)
            .unwrap_or(false));
        assert!(rx.try_recv().is_err(), "codex kind must not emit");
    }

    #[test]
    fn subscription_round_trip() {
        let cache = UsageCache::new();
        assert!(cache
            .with_subscription(&AppType::Claude, |q| q.success)
            .is_none());
        cache.put_subscription(AppType::Claude, fake_quota());
        let got = cache
            .with_subscription(&AppType::Claude, |q| q.success)
            .unwrap();
        assert!(got);
        assert!(cache
            .with_subscription(&AppType::Codex, |q| q.success)
            .is_none());
    }

    #[test]
    fn script_round_trip_and_invalidate() {
        let cache = UsageCache::new();
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_none());
        cache.put_script(AppType::Codex, "pid".to_string(), fake_result());
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_some());
        cache.invalidate_script(&AppType::Codex, "pid");
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_none());
    }

    #[test]
    fn script_keys_isolated_by_app_type() {
        let cache = UsageCache::new();
        cache.put_script(AppType::Claude, "same".to_string(), fake_result());
        assert!(cache
            .with_script(&AppType::Claude, "same", |r| r.success)
            .is_some());
        assert!(cache
            .with_script(&AppType::Codex, "same", |r| r.success)
            .is_none());
    }

    #[test]
    fn invalidate_all_clears_all_usage_snapshots() {
        let cache = UsageCache::new();
        cache.put_subscription(AppType::Claude, fake_quota());
        cache.put_codex_oauth("account-1".to_string(), fake_quota());
        cache.put_script(AppType::Codex, "provider".to_string(), fake_result());

        cache.invalidate_all();

        assert!(cache
            .with_subscription(&AppType::Claude, |quota| quota.success)
            .is_none());
        assert!(cache
            .with_script(&AppType::Codex, "provider", |usage| usage.success)
            .is_none());
        assert!(cache
            .with_codex_oauth("account-1", |quota| quota.success)
            .is_none());
    }
}
