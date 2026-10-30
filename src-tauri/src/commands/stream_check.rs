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
    app_type: AppType,
    provider_id: String,
) -> Result<StreamCheckResult, AppError> {
    stream_check_provider_inner(&state, &app_type, &provider_id).await
}

/// Shared with the headless web route: handle-free, manager from `AppState`.
pub(crate) async fn stream_check_provider_inner(
    state: &AppState,
    app_type: &AppType,
    provider_id: &str,
) -> Result<StreamCheckResult, AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
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
    let is_copilot = is_copilot_provider(provider);
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

fn is_copilot_provider(provider: &crate::provider::Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("github_copilot")
        || provider
            .settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str())
            .map(|url| url.contains("githubcopilot.com"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{is_copilot_provider, resolve_copilot_base_url_override};
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;
    use serial_test::serial;

    #[test]
    fn copilot_provider_detection_accepts_provider_type_or_base_url() {
        let typed_provider = Provider {
            id: "p1".to_string(),
            name: "typed".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        assert!(is_copilot_provider(&typed_provider));

        let url_provider = Provider {
            id: "p2".to_string(),
            name: "url".to_string(),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        assert!(is_copilot_provider(&url_provider));
    }

    #[test]
    fn copilot_full_url_metadata_is_available_for_override_guard() {
        let provider = Provider {
            id: "p3".to_string(),
            name: "relay".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                is_full_url: Some(true),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        assert!(is_copilot_provider(&provider));
        assert_eq!(
            provider.meta.as_ref().and_then(|meta| meta.is_full_url),
            Some(true)
        );
    }

    /// The `AppState`-based resolve short-circuits non-Copilot providers to
    /// `None` without touching the manager or the network.
    #[tokio::test]
    #[serial]
    async fn appstate_resolve_returns_none_for_non_copilot() {
        use crate::database::Database;
        use crate::store::AppState;
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

        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let plain = Provider {
            id: "plain".to_string(),
            name: "plain".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
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
