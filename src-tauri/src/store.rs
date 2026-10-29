use crate::commands::{AgyAccountState, CodexOAuthState, CopilotAuthState, XaiOAuthState};
use crate::database::Database;
use crate::events::EventHub;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::proxy::providers::copilot_auth::CopilotAuthManager;
use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
use crate::services::agy_account::AgyAccountManager;
use crate::services::kimi_oauth::KimiOAuthManager;
use crate::services::{ProxyService, UsageCache};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 全局应用状态
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    // 内部已使用细粒度锁（accounts/access_tokens/refresh_locks），所有方法均为
    // `&self`，无需外层 RwLock；避免持有粗粒度锁跨网络刷新导致的连锁阻塞。
    pub codex_oauth_manager: Arc<CodexOAuthManager>,
    /// Copilot 设备码登录管理器（桌面 State 与 web 路由共享同一实例）
    pub copilot_auth: Arc<RwLock<CopilotAuthManager>>,
    /// xAI (Grok) OAuth 管理器
    pub xai_oauth: Arc<RwLock<XaiOAuthManager>>,
    /// Antigravity 官方账号管理器
    pub agy_account: Arc<AgyAccountManager>,
    /// Kimi Code OAuth 管理器（桌面命令与 web 路由共享同一实例）
    pub kimi_oauth: Arc<KimiOAuthManager>,
    /// 后端 -> 前端事件总线（桌面端桥接到 AppHandle，web 端走 SSE）
    pub events: EventHub,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let app_config_dir = crate::config::get_app_config_dir();
        let codex_oauth_manager = Arc::new(CodexOAuthManager::new(app_config_dir.clone()));
        let copilot_auth = Arc::new(RwLock::new(CopilotAuthManager::new(app_config_dir.clone())));
        let xai_oauth = Arc::new(RwLock::new(XaiOAuthManager::new(app_config_dir.clone())));
        let agy_account = Arc::new(AgyAccountManager::new(app_config_dir));
        let events = EventHub::new();
        let proxy_service = ProxyService::new(db.clone()).with_events(events.clone());

        Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            codex_oauth_manager,
            copilot_auth,
            xai_oauth,
            agy_account,
            kimi_oauth: Arc::new(KimiOAuthManager::default()),
            events,
        }
    }

    /// 桌面端需要 `app.manage(...)` 的四个认证 State 包装，均为字段的 `Arc` 克隆，
    /// 不会产生新的管理器实例。
    pub(crate) fn managed_auth_states(&self) -> ManagedAuthStates {
        ManagedAuthStates {
            copilot: CopilotAuthState(self.copilot_auth.clone()),
            codex: CodexOAuthState(self.codex_oauth_manager.clone()),
            xai: XaiOAuthState(self.xai_oauth.clone()),
            agy: AgyAccountState(self.agy_account.clone()),
        }
    }
}

pub(crate) struct ManagedAuthStates {
    pub copilot: CopilotAuthState,
    pub codex: CodexOAuthState,
    pub xai: XaiOAuthState,
    pub agy: AgyAccountState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

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

    #[test]
    #[serial]
    fn managed_wrappers_share_appstate_arcs() {
        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let m = state.managed_auth_states();
        assert!(Arc::ptr_eq(&m.copilot.0, &state.copilot_auth));
        assert!(Arc::ptr_eq(&m.codex.0, &state.codex_oauth_manager));
        assert!(Arc::ptr_eq(&m.xai.0, &state.xai_oauth));
        assert!(Arc::ptr_eq(&m.agy.0, &state.agy_account));
    }

    #[test]
    #[serial]
    fn kimi_oauth_manager_is_shared_across_state_clones() {
        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        assert!(Arc::ptr_eq(&state.kimi_oauth, &state.clone().kimi_oauth));
    }

    #[tokio::test]
    #[serial]
    async fn managers_load_from_app_config_dir() {
        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let dir = crate::config::get_app_config_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("copilot_auth.json"),
            r#"{"version":3,"default_account_id":"1001","accounts":{"1001":{"github_token":"t","user":{"login":"octo-copilot","id":1001,"avatar_url":null},"authenticated_at":1}}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("xai_oauth_auth.json"),
            r#"{"version":1,"default_account_id":"xai-acc-7","accounts":{"xai-acc-7":{"account_id":"xai-acc-7","login":"grok-user","refresh_token":"r","authenticated_at":2}}}"#,
        )
        .unwrap();

        let state = AppState::new(Arc::new(Database::memory().unwrap()));

        let copilot = state.copilot_auth.read().await.list_accounts().await;
        assert_eq!(copilot.len(), 1);
        assert_eq!(copilot[0].login, "octo-copilot");
        let xai = state.xai_oauth.read().await.list_accounts().await;
        assert_eq!(xai.len(), 1);
        assert_eq!(xai[0].login, "grok-user");
    }
}
