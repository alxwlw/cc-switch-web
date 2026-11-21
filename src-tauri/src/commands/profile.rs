//! 项目 Profile 管理命令

use crate::error::AppError;
use tauri::State;

use super::run_db_task;
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
pub async fn list_profiles(state: State<'_, AppState>) -> Result<ProfilesResponse, String> {
    run_db_task(&state, ProfileService::list_response).await
}

/// 项目的写入口（这里的命令和托盘的两项）互相串行：主线程不再替它们排队。`apply` 收尾才写
/// 当前项目指针、自动保存会整行回写旧项目，期间的清除 / 重命名 / 删除会被盖掉；新建的快照会
/// 拍到切到一半的状态。
pub(crate) static PROFILE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn profile_mutation<T, E, F>(state: &State<'_, AppState>, task: F) -> Result<T, String>
where
    T: Send + 'static,
    E: Into<String> + Send + 'static,
    F: FnOnce(&AppState) -> Result<T, E> + Send + 'static,
{
    let _serial = PROFILE_LOCK.lock().await;
    run_db_task(state, task).await
}

#[tauri::command]
pub async fn create_profile(
    state: State<'_, AppState>,
    name: String,
    scope: String,
) -> Result<ProfileDto, String> {
    let scope = ProfileScope::parse(&scope)?;
    profile_mutation(&state, move |state| {
        ProfileService::create(state, &name, scope).map(ProfileDto::from)
    })
    .await
}

#[tauri::command]
pub async fn update_profile(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    resnapshot: Option<bool>,
    scope: Option<String>,
) -> Result<ProfileDto, String> {
    let scope = scope.map(|s| ProfileScope::parse(&s)).transpose()?;
    profile_mutation(&state, move |state| {
        ProfileService::update(state, &id, name, resnapshot.unwrap_or(false), scope)
            .map(ProfileDto::from)
    })
    .await
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    profile_mutation(&state, move |state| ProfileService::delete(state, &id)).await
}

#[tauri::command]
pub async fn clear_current_profile(
    state: State<'_, AppState>,
    scope: String,
) -> Result<(), String> {
    let scope = ProfileScope::parse(&scope)?;
    profile_mutation(&state, move |state| {
        state.db.set_current_profile_id(scope.as_str(), None)
    })
    .await
}

/// 应用项目快照（只作用于发起页所属分组内的应用）。
///
/// `ProviderService::switch` 用 block_on 拿切换锁，整段只能在阻塞线程池里跑
/// （同托盘的「应用项目」入口）。
#[tauri::command]
pub async fn apply_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    scope: String,
) -> Result<Vec<String>, String> {
    let scope = ProfileScope::parse(&scope)?;
    profile_mutation(&state, move |state| {
        let warnings = ProfileService::apply(state, &id, scope)?;
        emit_profile_apply_events(&app, state, &id, scope);
        Ok::<_, AppError>(warnings)
    })
    .await
}
