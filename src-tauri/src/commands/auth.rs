use tauri::State;

use crate::app_config::AppType;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthError;
use crate::proxy::providers::copilot_auth::{
    CopilotAuthError, GitHubAccount, GitHubDeviceCodeResponse,
};
use crate::proxy::providers::xai_oauth_auth::{XaiOAuthAccount, XaiOAuthError};
use crate::services::agy_account::AgyAccount;
use crate::store::AppState;

const AUTH_PROVIDER_GITHUB_COPILOT: &str = "github_copilot";
const AUTH_PROVIDER_CODEX_OAUTH: &str = "codex_oauth";
const AUTH_PROVIDER_XAI_OAUTH: &str = "xai_oauth";
pub(crate) const AUTH_PROVIDER_AGY_OAUTH: &str = "agy_oauth";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthAccount {
    pub id: String,
    pub provider: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
    pub is_default: bool,
    pub github_domain: String,
    /// Codex 专用：旧账号缺少写入原生 Codex auth.json 所需的 id_token。
    pub reauth_required: bool,
    /// xAI 专用：refresh token 已失效，账号不可再用于请求。
    pub requires_reauth: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthStatus {
    pub provider: String,
    pub authenticated: bool,
    pub default_account_id: Option<String>,
    pub migration_error: Option<String>,
    pub accounts: Vec<ManagedAuthAccount>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthDeviceCodeResponse {
    pub provider: String,
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub(crate) fn ensure_auth_provider(auth_provider: &str) -> Result<&'static str, String> {
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => Ok(AUTH_PROVIDER_GITHUB_COPILOT),
        AUTH_PROVIDER_CODEX_OAUTH => Ok(AUTH_PROVIDER_CODEX_OAUTH),
        AUTH_PROVIDER_XAI_OAUTH => Ok(AUTH_PROVIDER_XAI_OAUTH),
        AUTH_PROVIDER_AGY_OAUTH => Ok(AUTH_PROVIDER_AGY_OAUTH),
        _ => Err(format!("Unsupported auth provider: {auth_provider}")),
    }
}

fn map_account(
    provider: &str,
    account: GitHubAccount,
    default_account_id: Option<&str>,
) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: default_account_id == Some(account.id.as_str()),
        reauth_required: account.reauth_required,
        requires_reauth: false,
        id: account.id,
        provider: provider.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
        github_domain: account.github_domain,
    }
}

fn map_xai_account(
    account: XaiOAuthAccount,
    default_account_id: Option<&str>,
) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: default_account_id == Some(account.id.as_str()),
        id: account.id,
        provider: AUTH_PROVIDER_XAI_OAUTH.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
        github_domain: account.github_domain,
        reauth_required: false,
        requires_reauth: account.requires_reauth,
    }
}

fn map_agy_account(account: AgyAccount, current_account_id: Option<&str>) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: current_account_id == Some(account.id.as_str()),
        login: account.label(),
        avatar_url: account.picture,
        authenticated_at: account.authenticated_at,
        github_domain: "google.com".to_string(),
        reauth_required: false,
        requires_reauth: account.requires_reauth,
        provider: AUTH_PROVIDER_AGY_OAUTH.to_string(),
        id: account.id,
    }
}

fn map_device_code_response(
    provider: &str,
    response: GitHubDeviceCodeResponse,
) -> ManagedAuthDeviceCodeResponse {
    ManagedAuthDeviceCodeResponse {
        provider: provider.to_string(),
        device_code: response.device_code,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        expires_in: response.expires_in,
        interval: response.interval,
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "camelCase")]
pub async fn auth_start_login(
    auth_provider: String,
    github_domain: Option<String>,
    target_account_id: Option<String>,
    login_method: Option<String>,
    state: State<'_, AppState>,
) -> Result<ManagedAuthDeviceCodeResponse, String> {
    start_login_inner(
        state.inner(),
        &auth_provider,
        github_domain.as_deref(),
        target_account_id.as_deref(),
        login_method.as_deref(),
    )
    .await
}

pub(crate) async fn start_login_inner(
    state: &AppState,
    auth_provider: &str,
    github_domain: Option<&str>,
    target_account_id: Option<&str>,
    login_method: Option<&str>,
) -> Result<ManagedAuthDeviceCodeResponse, String> {
    let auth_provider = ensure_auth_provider(auth_provider)?;
    let browser_login = match login_method.unwrap_or("device_code") {
        "device_code" => false,
        "browser" if auth_provider == AUTH_PROVIDER_CODEX_OAUTH => true,
        _ => return Err("Unsupported login method for this auth provider".into()),
    };
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            if target_account_id.is_some() {
                return Err("Targeted re-authentication is only supported for Codex OAuth".into());
            }
            let auth_manager = state.copilot_auth.read().await;
            let response = auth_manager
                .start_device_flow(github_domain)
                .await
                .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = &state.codex_oauth_manager;
            let response = if browser_login {
                auth_manager.start_browser_flow(target_account_id).await
            } else {
                auth_manager.start_device_flow(target_account_id).await
            }
            .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            if target_account_id.is_some() {
                return Err("Targeted re-authentication is only supported for Codex OAuth".into());
            }
            let auth_manager = state.xai_oauth.read().await;
            let response = auth_manager
                .start_device_flow()
                .await
                .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        // 浏览器登录没有用户码：device_code 放这次登录的 id，verification_uri 放授权页地址
        AUTH_PROVIDER_AGY_OAUTH => {
            if target_account_id.is_some() {
                return Err("Targeted re-authentication is only supported for Codex OAuth".into());
            }
            let start = state
                .agy_account
                .start_login()
                .await
                .map_err(|e| e.to_string())?;
            Ok(ManagedAuthDeviceCodeResponse {
                provider: auth_provider.to_string(),
                device_code: start.flow_id,
                user_code: String::new(),
                verification_uri: start.auth_url,
                expires_in: start.expires_in,
                interval: 1,
            })
        }
        _ => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "camelCase")]
pub async fn auth_poll_for_account(
    auth_provider: String,
    device_code: String,
    github_domain: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<ManagedAuthAccount>, String> {
    poll_for_account_inner(
        state.inner(),
        &auth_provider,
        &device_code,
        github_domain.as_deref(),
    )
    .await
}

pub(crate) async fn poll_for_account_inner(
    state: &AppState,
    auth_provider: &str,
    device_code: &str,
    github_domain: Option<&str>,
) -> Result<Option<ManagedAuthAccount>, String> {
    let auth_provider = ensure_auth_provider(auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = state.copilot_auth.write().await;
            match auth_manager
                .poll_for_token(device_code, github_domain)
                .await
            {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account.map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(CopilotAuthError::AuthorizationPending) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = &state.codex_oauth_manager;
            match auth_manager
                .poll_for_token(device_code, || async {
                    state
                        .proxy_service
                        .lock_switch_for_app(AppType::Codex.as_str())
                        .await
                })
                .await
            {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account.map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(CodexOAuthError::AuthorizationPending) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = state.xai_oauth.write().await;
            match auth_manager.poll_for_token(device_code).await {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account
                        .map(|account| map_xai_account(account, default_account_id.as_deref())))
                }
                Err(XaiOAuthError::AuthorizationPending) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_AGY_OAUTH => {
            let Some(account) = state
                .agy_account
                .poll_login(device_code)
                .await
                .map_err(|e| e.to_string())?
            else {
                return Ok(None);
            };
            let current = state
                .agy_account
                .status()
                .await
                .ok()
                .and_then(|status| status.current_account_id);
            Ok(Some(map_agy_account(account, current.as_deref())))
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_cancel_login(
    auth_provider: String,
    device_code: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    cancel_login_inner(state.inner(), &auth_provider, &device_code).await
}

pub(crate) async fn cancel_login_inner(
    state: &AppState,
    auth_provider: &str,
    device_code: &str,
) -> Result<bool, String> {
    let auth_provider = ensure_auth_provider(auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_CODEX_OAUTH => Ok(state
            .codex_oauth_manager
            .cancel_device_flow(device_code)
            .await),
        AUTH_PROVIDER_AGY_OAUTH => Ok(state.agy_account.cancel_login(device_code).await),
        _ => Err("Login cancellation is only supported for Codex OAuth".to_string()),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_list_accounts(
    auth_provider: String,
    state: State<'_, AppState>,
) -> Result<Vec<ManagedAuthAccount>, String> {
    list_accounts_inner(state.inner(), &auth_provider).await
}

pub(crate) async fn list_accounts_inner(
    state: &AppState,
    auth_provider: &str,
) -> Result<Vec<ManagedAuthAccount>, String> {
    let auth_provider = ensure_auth_provider(auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = state.copilot_auth.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_account(auth_provider, account, default_account_id.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = &state.codex_oauth_manager;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_account(auth_provider, account, default_account_id.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = state.xai_oauth.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_xai_account(account, default_account_id.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_AGY_OAUTH => {
            let status = state
                .agy_account
                .status()
                .await
                .map_err(|e| e.to_string())?;
            let current = status.current_account_id;
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_agy_account(account, current.as_deref()))
                .collect())
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_get_status(
    auth_provider: String,
    state: State<'_, AppState>,
) -> Result<ManagedAuthStatus, String> {
    get_status_inner(state.inner(), &auth_provider).await
}

pub(crate) async fn get_status_inner(
    state: &AppState,
    auth_provider: &str,
) -> Result<ManagedAuthStatus, String> {
    let auth_provider = ensure_auth_provider(auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = state.copilot_auth.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: status.authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: status.migration_error,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    })
                    .collect(),
            })
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = &state.codex_oauth_manager;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: status.authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: None,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    })
                    .collect(),
            })
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = state.xai_oauth.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: status.authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: None,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| map_xai_account(account, default_account_id.as_deref()))
                    .collect(),
            })
        }
        // 「默认账号」在这里是 agy 当前登录的那个号
        AUTH_PROVIDER_AGY_OAUTH => {
            let status = state
                .agy_account
                .status()
                .await
                .map_err(|e| e.to_string())?;
            let current = status.current_account_id;
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: !status.accounts.is_empty(),
                default_account_id: current.clone(),
                migration_error: None,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| map_agy_account(account, current.as_deref()))
                    .collect(),
            })
        }
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_remove_account(
    auth_provider: String,
    account_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    remove_account_inner(state.inner(), &auth_provider, &account_id).await
}

pub(crate) async fn remove_account_inner(
    state: &AppState,
    auth_provider: &str,
    account_id: &str,
) -> Result<(), String> {
    let auth_provider = ensure_auth_provider(auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = state.copilot_auth.write().await;
            auth_manager
                .remove_account(account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            remove_codex_oauth_account_with_switch_lock(state, account_id).await
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = state.xai_oauth.write().await;
            auth_manager
                .remove_account(account_id)
                .await
                .map_err(|e| e.to_string())
        }
        // 只删 CC Switch 里的副本，不替 agy 登出
        AUTH_PROVIDER_AGY_OAUTH => state
            .agy_account
            .remove(account_id)
            .await
            .map_err(|e| e.to_string()),
        _ => unreachable!(),
    }
}

pub(crate) async fn remove_codex_oauth_account_with_switch_lock(
    app_state: &AppState,
    account_id: &str,
) -> Result<(), String> {
    // Serialize Auth Center credential deletion with managed provider
    // add/update/switch/hot-switch. Otherwise a switch that already preflighted
    // a bundle could recreate auth.json after removal.
    let _switch_guard = app_state
        .proxy_service
        .lock_switch_for_app(AppType::Codex.as_str())
        .await;
    app_state
        .codex_oauth_manager
        .remove_account(account_id)
        .await
        .map_err(|error| error.to_string())?;
    app_state
        .db
        .unbind_codex_managed_accounts(&[account_id.to_string()])
        .map_err(|error| {
            format!("账号已删除，但 Codex 供应商解绑失败；请编辑供应商重新选择账号或切换供应商: {error}")
        })?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_set_default_account(
    auth_provider: String,
    account_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    set_default_account_inner(state.inner(), &auth_provider, &account_id).await
}

pub(crate) async fn set_default_account_inner(
    state: &AppState,
    auth_provider: &str,
    account_id: &str,
) -> Result<(), String> {
    let auth_provider = ensure_auth_provider(auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = state.copilot_auth.write().await;
            auth_manager
                .set_default_account(account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_OAUTH => {
            let auth_manager = &state.codex_oauth_manager;
            auth_manager
                .set_default_account(account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = state.xai_oauth.write().await;
            auth_manager
                .set_default_account(account_id)
                .await
                .map_err(|e| e.to_string())
        }
        // 切 agy 的全局登录（写钥匙串）
        AUTH_PROVIDER_AGY_OAUTH => state
            .agy_account
            .switch_to(account_id)
            .await
            .map_err(|e| e.to_string()),
        _ => unreachable!(),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_logout(auth_provider: String, state: State<'_, AppState>) -> Result<(), String> {
    logout_inner(state.inner(), &auth_provider).await
}

pub(crate) async fn logout_inner(state: &AppState, auth_provider: &str) -> Result<(), String> {
    let auth_provider = ensure_auth_provider(auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = state.copilot_auth.write().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_OAUTH => logout_codex_oauth_with_switch_lock(state).await,
        AUTH_PROVIDER_XAI_OAUTH => {
            let auth_manager = state.xai_oauth.write().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_AGY_OAUTH => state
            .agy_account
            .remove_all()
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        _ => unreachable!(),
    }
}

pub(crate) async fn logout_codex_oauth_with_switch_lock(
    app_state: &AppState,
) -> Result<(), String> {
    let _switch_guard = app_state
        .proxy_service
        .lock_switch_for_app(AppType::Codex.as_str())
        .await;
    // Only this device's accounts: rows synced from another device stay bound there.
    let removed: Vec<String> = app_state
        .codex_oauth_manager
        .list_accounts()
        .await
        .into_iter()
        .map(|account| account.id)
        .collect();
    app_state
        .codex_oauth_manager
        .clear_auth()
        .await
        .map_err(|error| error.to_string())?;
    app_state
        .db
        .unbind_codex_managed_accounts(&removed)
        .map_err(|error| {
            format!("托管账号已登出，但 Codex 供应商解绑失败；请编辑供应商重新选择账号或切换供应商: {error}")
        })?;
    Ok(())
}
