//! 供应商连通性检查命令
//!
//! 注意：本检查只探测 base_url 是否可达，不发真实大模型请求，也不触碰故障转移
//! 熔断器（熔断器由真实转发流量驱动）。详见 `services::stream_check`。

use crate::app_config::AppType;
use crate::error::AppError;
use crate::proxy::providers::copilot_auth::CopilotAuthManager;
use crate::services::stream_check::{StreamCheckConfig, StreamCheckResult, StreamCheckService};
use crate::store::AppState;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// 连通性检查（单个供应商）
#[tauri::command]
pub async fn stream_check_provider(
    state: State<'_, AppState>,
    app_type: String,
    provider_id: String,
) -> Result<StreamCheckResult, AppError> {
    let app_type: AppType = app_type.parse()?;
    stream_check_provider_inner(&state, &app_type, &provider_id).await
}

/// Shared with the headless web route: handle-free, manager from `AppState`.
pub(crate) async fn stream_check_provider_inner(
    state: &AppState,
    app_type: &AppType,
    provider_id: &str,
) -> Result<StreamCheckResult, AppError> {
    let providers = state.db.get_all_providers(app_type)?;
    let provider = providers
        .get(provider_id)
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

    // Copilot 端点是动态的（随 OAuth token 解析），需预先取出 host 再探测；
    // 其余供应商传 None，由服务层从 settings_config 提取 base_url。无需鉴权。
    let base_url_override =
        resolve_copilot_base_url_override(provider, &state.copilot_auth).await?;
    StreamCheckService::check_with_retry(
        app_type,
        provider,
        &StreamCheckConfig::default(),
        base_url_override,
    )
    .await
}

/// Copilot 供应商的 base_url 需要从 OAuth 管理器动态解析（按账号或默认端点）。
/// `is_full_url` 的供应商已是完整地址，无需解析。管理器直接取自 `AppState`
/// （与 `managed_auth_states().copilot` 同一个 `Arc`），桌面与 web 共用一路。
async fn resolve_copilot_base_url_override(
    provider: &crate::provider::Provider,
    copilot_auth: &Arc<RwLock<CopilotAuthManager>>,
) -> Result<Option<String>, AppError> {
    let is_copilot = provider.is_github_copilot();
    let is_full_url = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.is_full_url)
        .unwrap_or(false);

    if !is_copilot || is_full_url {
        return Ok(None);
    }

    let auth_manager = copilot_auth.read().await;
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("github_copilot"));

    let endpoint = match account_id.as_deref() {
        Some(id) => auth_manager.get_api_endpoint(id).await,
        None => auth_manager.get_default_api_endpoint().await,
    };

    Ok(Some(endpoint))
}

#[cfg(test)]
mod tests {
    use super::resolve_copilot_base_url_override;
    use std::sync::Arc;

    use serde_json::json;
    use serial_test::serial;

    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::store::AppState;

    /// The frontend reaches this command over IPC with `appType`, so it must
    /// accept the same app ids (aliases, any case) as the other commands.
    #[test]
    fn app_type_arg_accepts_the_same_ids_as_other_commands() {
        let db = Arc::new(Database::memory().unwrap());
        // Official providers fail before any network probe, which shows the
        // lookup found the provider under the right app.
        for app in [
            AppType::Claude,
            AppType::Kimi,
            AppType::Omp,
            AppType::Copilot,
            AppType::WorkBuddy,
        ] {
            let mut provider = Provider::with_id(
                format!("official-{app}"),
                "Official".into(),
                json!({}),
                None,
            );
            provider.category = Some("official".into());
            db.save_provider(&app, &provider).unwrap();
        }
        let app = tauri::test::mock_builder()
            .manage(AppState::new(db))
            .invoke_handler(tauri::generate_handler![super::stream_check_provider])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let check = |app_type: &str, provider_id: &str| {
            tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: "stream_check_provider".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "http://tauri.localhost".parse().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(
                        json!({"appType": app_type, "providerId": provider_id}),
                    ),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .unwrap_err()
        };

        let official = json!("Official providers do not expose a reachability-check target");
        for (app_type, provider_id) in [
            ("claude", "official-claude"),
            ("Claude", "official-claude"),
            (" CLAUDE ", "official-claude"),
            ("kimi-code", "official-kimi"),
            ("oh-my-pi", "official-omp"),
            ("github-copilot", "official-copilot"),
            ("work_buddy", "official-workbuddy"),
        ] {
            assert_eq!(check(app_type, provider_id), official, "{app_type}");
        }

        let unknown = check("open-code", "official-claude");
        let unknown = unknown.as_str().unwrap();
        assert!(
            unknown.contains("Unsupported app id: 'open-code'"),
            "{unknown}"
        );
    }

    /// The `AppState`-based resolve short-circuits non-Copilot providers to
    /// `None` without touching the manager or the network.
    #[tokio::test]
    #[serial]
    async fn appstate_resolve_returns_none_for_non_copilot() {
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

        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let plain = Provider::with_id("plain".into(), "plain".into(), json!({}), None);
        let resolved = resolve_copilot_base_url_override(&plain, &state.copilot_auth)
            .await
            .expect("non-copilot never errors");
        assert_eq!(resolved, None);
        assert!(Arc::ptr_eq(
            &state.copilot_auth,
            &state.managed_auth_states().copilot.0
        ));
    }
}
