//! 项目 Profile 管理命令

use tauri::State;

pub use crate::services::profile::{CurrentProfileIds, ProfileDto, ProfilesResponse};
use crate::services::profile::{ProfileScope, ProfileService};
use crate::store::AppState;

/// Profile 应用完成后的统一收尾：发事件（经 `state.events`）+ 重建托盘菜单
///
/// 事件部分见 [`ProfileService::emit_apply_events`]（web 路由共用）；
/// 托盘刷新只属于桌面壳。UI 与托盘两个入口必须共用此函数。
pub fn emit_profile_apply_events(
    app: &tauri::AppHandle,
    state: &AppState,
    profile_id: &str,
    scope: ProfileScope,
) {
    ProfileService::emit_apply_events(state, profile_id, scope);
    crate::tray::refresh_tray_menu(app);
}

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<ProfilesResponse, String> {
    ProfileService::list_response(&state).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_profile(
    state: State<'_, AppState>,
    name: String,
    scope: String,
) -> Result<ProfileDto, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    ProfileService::create(&state, &name, scope)
        .map(ProfileDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    resnapshot: Option<bool>,
    scope: Option<String>,
) -> Result<ProfileDto, String> {
    let scope = scope
        .map(|s| ProfileScope::parse(&s))
        .transpose()
        .map_err(|e| e.to_string())?;
    ProfileService::update(&state, &id, name, resnapshot.unwrap_or(false), scope)
        .map(ProfileDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    ProfileService::delete(&state, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_current_profile(state: State<'_, AppState>, scope: String) -> Result<(), String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    state
        .db
        .set_current_profile_id(scope.as_str(), None)
        .map_err(|e| e.to_string())
}

/// 应用项目快照（只作用于发起页所属分组内的应用）。
///
/// 注意：必须保持同步命令（跑在 Tauri 线程池）——`ProviderService::switch`
/// 内部使用 block_on 获取切换锁，放进 async 命令会在运行时线程上 panic。
#[tauri::command]
pub fn apply_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    scope: String,
) -> Result<Vec<String>, String> {
    let scope = ProfileScope::parse(&scope).map_err(|e| e.to_string())?;
    let warnings = ProfileService::apply(&state, &id, scope).map_err(|e| e.to_string())?;
    emit_profile_apply_events(&app, &state, &id, scope);
    Ok(warnings)
}
