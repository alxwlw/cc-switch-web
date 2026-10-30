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
        let proxy_service = ProxyService::new(db.clone())
            .with_events(events.clone())
            .with_auth(crate::proxy::server::OAuthManagers {
                copilot: copilot_auth.clone(),
                codex: codex_oauth_manager.clone(),
                xai: xai_oauth.clone(),
            });

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

    #[test]
    #[serial]
    fn proxy_auth_bundle_shares_appstate_arcs() {
        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let auth = state.proxy_service.auth_for_test().expect("bundle set");
        assert!(Arc::ptr_eq(&auth.copilot, &state.copilot_auth));
        assert!(Arc::ptr_eq(&auth.codex, &state.codex_oauth_manager));
        assert!(Arc::ptr_eq(&auth.xai, &state.xai_oauth));
    }

    /// Headless (no AppHandle): the forwarder takes the Copilot token from the manager
    /// that AppState owns; after the account is removed through that manager the next
    /// request fails closed and nothing with the old token reaches the upstream.
    ///
    /// The Copilot manager exchanges the GitHub token at api.github.com on a cold cache,
    /// so the test seeds the manager's token + endpoint cache (`test_seed_runtime`).
    #[tokio::test]
    #[serial]
    async fn headless_proxy_forwards_with_manager_token_and_stops_after_logout() {
        use axum::{http::HeaderMap, routing::any, Router};
        use std::sync::Mutex;

        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let dir = crate::config::get_app_config_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("copilot_auth.json"),
            r#"{"version":3,"default_account_id":"1001","accounts":{"1001":{"github_token":"gh-secret","user":{"login":"octo","id":1001,"avatar_url":null},"authenticated_at":1}}}"#,
        )
        .unwrap();

        let seen: Arc<Mutex<Vec<(String, Option<String>)>>> = Arc::default();
        let log = seen.clone();
        let app = Router::new().fallback(any(
            move |uri: axum::http::Uri, headers: HeaderMap| {
                let log = log.clone();
                async move {
                    log.lock().unwrap().push((
                        uri.path().to_string(),
                        headers
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string),
                    ));
                    (
                        [("content-type", "application/json")],
                        r#"{"id":"c1","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
                    )
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        state
            .copilot_auth
            .read()
            .await
            .test_seed_runtime("1001", "tid-managed-token", &format!("http://{addr}"))
            .await;
        let mut provider = crate::provider::Provider::with_id(
            "copilot-1".to_string(),
            "Copilot".to_string(),
            serde_json::json!({"env": {"ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"}}),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("github_copilot".to_string()),
            ..Default::default()
        });
        state.db.save_provider("claude", &provider).unwrap();
        state
            .db
            .set_current_provider("claude", "copilot-1")
            .unwrap();
        let mut config = state.db.get_proxy_config().await.unwrap();
        config.listen_address = "127.0.0.1".to_string();
        config.listen_port = 0;
        state.db.update_proxy_config(config).await.unwrap();
        let info = state.proxy_service.start().await.expect("start proxy");

        let client = reqwest::Client::new();
        let send_to = |port: u16| {
            let client = client.clone();
            async move {
                client
                    .post(format!("http://127.0.0.1:{port}/v1/messages"))
                    .json(&serde_json::json!({
                        "model": "gpt-4o", "max_tokens": 8,
                        "messages": [{"role": "user", "content": "hi"}]
                    }))
                    .send()
                    .await
                    .unwrap()
            }
        };

        let first = send_to(info.port).await;
        assert!(
            first.status().is_success(),
            "{}",
            first.text().await.unwrap()
        );
        let chat_auth: Vec<_> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.ends_with("/chat/completions"))
            .map(|(_, auth)| auth.clone())
            .collect();
        assert_eq!(
            chat_auth,
            vec![Some("Bearer tid-managed-token".to_string())]
        );

        // Config update restarts the server through start_with; the restarted forwarder
        // must still carry the managers.
        let mut config = state.db.get_proxy_config().await.unwrap();
        config.listen_port = 0; // differs from the persisted ephemeral port -> restart
        assert!(state.proxy_service.update_config(&config).await.unwrap());
        let new_port = state.proxy_service.get_status().await.unwrap().port;
        assert_ne!(new_port, info.port, "server did not restart");
        let restarted = send_to(new_port).await;
        assert!(
            restarted.status().is_success(),
            "{}",
            restarted.text().await.unwrap()
        );
        let chat_auth: Vec<_> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.ends_with("/chat/completions"))
            .map(|(_, auth)| auth.clone())
            .collect();
        assert_eq!(
            chat_auth,
            vec![Some("Bearer tid-managed-token".to_string()); 2],
            "restart path lost the managers"
        );

        // Logout through the same manager, then the next request must not leave with the old token.
        state
            .copilot_auth
            .read()
            .await
            .remove_account("1001")
            .await
            .unwrap();
        let before = seen.lock().unwrap().len();
        let second = send_to(new_port).await;
        assert!(
            !second.status().is_success(),
            "request after logout must fail"
        );
        let leaked: Vec<_> = seen.lock().unwrap()[before..]
            .iter()
            .filter(|(_, auth)| {
                auth.as_deref()
                    .is_some_and(|a| a.contains("tid-managed-token"))
            })
            .cloned()
            .collect();
        assert!(leaked.is_empty(), "old token reached upstream: {leaked:?}");

        state.proxy_service.stop().await.unwrap();
        upstream.abort();
    }

    /// The Copilot models inner resolves its manager from AppState and serves the
    /// stub catalog with the manager's token: no external network, the models
    /// endpoint is seeded to the stub via `test_seed_runtime`. Codex/xAI inners
    /// hit hardcoded upstreams, so they cannot be stubbed this way (hence Copilot).
    #[tokio::test]
    #[serial]
    async fn copilot_models_inner_serves_stub_catalog_with_manager_token() {
        use axum::{http::HeaderMap, routing::any, Router};
        use std::sync::Mutex;

        type SeenHit = (String, String, Option<String>);

        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let dir = crate::config::get_app_config_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("copilot_auth.json"),
            r#"{"version":3,"default_account_id":"1001","accounts":{"1001":{"github_token":"gh-secret","user":{"login":"octo","id":1001,"avatar_url":null},"authenticated_at":1}}}"#,
        )
        .unwrap();

        let seen: Arc<Mutex<Vec<SeenHit>>> = Arc::default();
        let log = seen.clone();
        let app = Router::new().fallback(any(
            move |method: axum::http::Method,
                  uri: axum::http::Uri,
                  headers: HeaderMap| {
                let log = log.clone();
                async move {
                    log.lock().unwrap().push((
                        method.to_string(),
                        uri.path().to_string(),
                        headers
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string),
                    ));
                    (
                        [("content-type", "application/json")],
                        r#"{"data":[
                            {"id":"stub-gpt","name":"Stub GPT","vendor":"Stub","model_picker_enabled":true},
                            {"id":"stub-hidden","name":"Stub Hidden","vendor":"Stub","model_picker_enabled":false}
                        ]}"#,
                    )
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let managed = state.managed_auth_states();
        assert!(Arc::ptr_eq(&managed.copilot.0, &state.copilot_auth));
        state
            .copilot_auth
            .read()
            .await
            .test_seed_runtime("1001", "tid-models-token", &format!("http://{addr}"))
            .await;

        let models = crate::commands::copilot_get_models_inner(&state, Some("1001".to_string()))
            .await
            .expect("stub catalog");
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["stub-gpt"]
        );

        let via_default = crate::commands::copilot_get_models_inner(&state, None)
            .await
            .expect("stub catalog via default");
        assert_eq!(via_default.len(), models.len());
        assert_eq!(via_default[0].id, "stub-gpt");

        let hits: Vec<_> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, path, _)| method == "GET" && path.ends_with("/models"))
            .map(|(_, _, auth)| auth.clone())
            .collect();
        assert_eq!(hits, vec![Some("Bearer tid-models-token".to_string())]);

        upstream.abort();
    }

    /// A fresh server has fresh (closed) breakers, so health rows left by the previous
    /// server must not survive: on the first start and on the config-restart path.
    #[tokio::test]
    #[serial]
    async fn starting_a_server_clears_stale_provider_health() {
        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let db = Arc::new(Database::memory().unwrap());
        db.update_proxy_config(crate::proxy::types::ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })
        .await
        .unwrap();
        db.save_provider(
            "claude",
            &crate::provider::Provider::with_id(
                "p1".into(),
                "P1".into(),
                serde_json::json!({}),
                None,
            ),
        )
        .unwrap();
        let state = AppState::new(db.clone());
        let unhealthy = || async {
            db.update_provider_health("p1", "claude", false, Some("boom".into()))
                .await
                .unwrap();
            // threshold 5: four more failures flip is_healthy
            for _ in 0..4 {
                db.update_provider_health("p1", "claude", false, Some("boom".into()))
                    .await
                    .unwrap();
            }
            assert!(
                !db.get_provider_health("p1", "claude")
                    .await
                    .unwrap()
                    .is_healthy
            );
        };

        unhealthy().await;
        state.proxy_service.start().await.unwrap();
        assert!(
            db.get_provider_health("p1", "claude")
                .await
                .unwrap()
                .is_healthy
        );

        unhealthy().await;
        let mut config = db.get_proxy_config().await.unwrap();
        config.listen_port = 0; // differs from the persisted ephemeral port -> restart
        assert!(state.proxy_service.update_config(&config).await.unwrap());
        assert!(
            db.get_provider_health("p1", "claude")
                .await
                .unwrap()
                .is_healthy
        );

        state.proxy_service.stop().await.unwrap();
    }

    /// The bound AppState reaches the forwarder's ProxyState on both construction paths
    /// (first start, config restart); unbound services start without one.
    #[tokio::test]
    #[serial]
    async fn bound_app_state_reaches_the_proxy_server_on_start_and_restart() {
        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let db = Arc::new(Database::memory().unwrap());
        db.update_proxy_config(crate::proxy::types::ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })
        .await
        .unwrap();
        let state = AppState::new(db.clone());

        state.proxy_service.start().await.unwrap();
        assert!(
            !state
                .proxy_service
                .app_state_bound_in_server_for_test()
                .await
        );
        state.proxy_service.stop().await.unwrap();

        state.proxy_service.bind_app_state(state.clone());
        state.proxy_service.start().await.unwrap();
        assert!(
            state
                .proxy_service
                .app_state_bound_in_server_for_test()
                .await
        );

        let mut config = db.get_proxy_config().await.unwrap();
        config.listen_port = 0;
        assert!(state.proxy_service.update_config(&config).await.unwrap());
        assert!(
            state
                .proxy_service
                .app_state_bound_in_server_for_test()
                .await
        );
        state.proxy_service.stop().await.unwrap();
    }
}
