use crate::app_config::AppType;
use crate::services::kimi_oauth::{KimiLoginPoll, KimiLoginStart, KimiOAuthStatus, KimiRegion};
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub async fn kimi_oauth_status(state: State<'_, AppState>) -> Result<KimiOAuthStatus, String> {
    state.kimi_oauth.status().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_start(
    region: KimiRegion,
    account_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<KimiLoginStart, String> {
    state.kimi_oauth.start(region, account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_poll(
    flow_id: String,
    state: State<'_, AppState>,
) -> Result<KimiLoginPoll, String> {
    state.kimi_oauth.poll(&flow_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_cancel(
    flow_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.kimi_oauth.cancel(&flow_id).await)
}

/// Separate accounts are deleted with their saved profile; Kimi's own default
/// login is only removed from the list.
#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_remove(
    account_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    kimi_oauth_remove_inner(&state, &account_id).await
}

/// Shared by the desktop command and the web route.
pub(crate) async fn kimi_oauth_remove_inner(
    state: &AppState,
    account_id: &str,
) -> Result<(), String> {
    // Listing Kimi providers writes native entries back to the database too.
    let _guard = state.proxy_service.lock_switch_for_app("kimi").await;
    state
        .kimi_oauth
        .remove(account_id, || {
            state.db.delete_provider(&AppType::Kimi, account_id)?;
            Ok(())
        })
        .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_quota(
    account_id: String,
    state: State<'_, AppState>,
) -> Result<SubscriptionQuota, String> {
    state.kimi_oauth.quota(&account_id).await
}

/// The account's current official model catalog, for editing its card.
#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_models(
    account_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    state.kimi_oauth.models(&account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn kimi_oauth_use_account(
    account_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.kimi_oauth.use_account(&account_id).await
}
