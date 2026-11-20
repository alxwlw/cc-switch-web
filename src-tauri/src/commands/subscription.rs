use std::str::FromStr;
use tauri::State;

use crate::app_config::AppType;
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;

/// Shared with the headless web route: the agy branch resolves its manager
/// from `AppState` (desktop previously used `try_state::<AgyAccountState>()`
/// on the `AppHandle`; the managed wrapper holds the same `Arc`).
pub(crate) async fn get_subscription_quota_inner(
    state: &AppState,
    tool: &str,
) -> Result<SubscriptionQuota, String> {
    if tool == "agy" {
        crate::commands::agy_account::agy_quota(&state.agy_account, None).await
    } else {
        crate::services::subscription::get_subscription_quota(tool).await
    }
}

/// 查询官方订阅额度
///
/// 读取 CLI 工具已有的 OAuth 凭据并调用官方 API 获取使用额度。
/// `Ok`（成功或确定性失败）写入 `UsageCache`、通知托盘刷新并 emit
/// `usage-cache-updated`，让前端 React Query 与托盘共享同一份最新数据；失败
/// 快照写入后 `format_subscription_summary` 会通过 `success=false` 守卫返回
/// `None`，托盘 suffix 自然消失，避免长期滞留旧配额数字。
/// `Err`（瞬时传输失败）不写快照、不 emit：保留上一份托盘快照，与前端
/// react-query reject 保留上次 data 的语义一致（emit 失败快照会经
/// `useUsageCacheBridge` 盲写回 query 缓存，抹掉本该保留的旧值）。
#[tauri::command]
pub async fn get_subscription_quota(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    tool: String,
) -> Result<SubscriptionQuota, String> {
    let inner = get_subscription_quota_inner(&state, &tool).await;
    if let Ok(snapshot) = &inner {
        if let Ok(app_type) = AppType::from_str(&tool) {
            crate::services::publish_usage(
                &state,
                crate::services::UsageKind::Subscription {
                    app_type,
                    data: snapshot.clone(),
                },
            );
            crate::tray::schedule_tray_refresh(&app);
        }
    }
    inner
}

#[cfg(test)]
mod tests {
    use super::get_subscription_quota_inner;
    use crate::database::Database;
    use crate::store::AppState;
    use serial_test::serial;
    use std::sync::Arc;

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

    fn test_state() -> (tempfile::TempDir, EnvGuard, AppState) {
        let home = tempfile::tempdir().unwrap();
        let env = EnvGuard::set(home.path());
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        (home, env, state)
    }

    /// Unknown tools short-circuit to `not_found` without touching credentials
    /// or the network.
    #[tokio::test]
    #[serial]
    async fn inner_unknown_tool_returns_not_found_without_io() {
        let (_home, _env, state) = test_state();
        let quota = get_subscription_quota_inner(&state, "no-such-tool")
            .await
            .expect("unknown tool is a deterministic Ok");
        assert_eq!(quota.tool, "no-such-tool");
        assert!(!quota.success);
        assert!(matches!(
            quota.credential_status,
            crate::services::subscription::CredentialStatus::NotFound
        ));
    }

    /// The agy branch resolves the manager from `AppState`; with no agy binary
    /// on PATH (or no keychain on Linux) it falls back without network I/O.
    /// Assumes the test environment has no `agy` binary (same assumption as
    /// the tray/panel paths: an installed `agy` would spawn `agy --print`
    /// with a 30 s timeout instead).
    #[tokio::test]
    #[serial]
    async fn inner_agy_branch_uses_appstate_manager() {
        let (_home, _env, state) = test_state();
        assert!(Arc::ptr_eq(
            &state.agy_account,
            &state.managed_auth_states().agy.0
        ));
        let quota = get_subscription_quota_inner(&state, "agy")
            .await
            .expect("agy fallback is Ok without network");
        assert_eq!(quota.tool, "agy");
    }
}
