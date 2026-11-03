use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

/// Store 中的键名
const STORE_KEY_APP_CONFIG_DIR: &str = "app_config_dir_override";

/// 缓存当前的 app_config_dir 覆盖路径，避免存储 AppHandle
static APP_CONFIG_DIR_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn override_cache() -> &'static RwLock<Option<PathBuf>> {
    APP_CONFIG_DIR_OVERRIDE.get_or_init(|| RwLock::new(None))
}

fn update_cached_override(value: Option<PathBuf>) {
    if let Ok(mut guard) = override_cache().write() {
        *guard = value;
    }
}

/// 获取缓存中的 app_config_dir 覆盖路径
pub fn get_app_config_dir_override() -> Option<PathBuf> {
    override_cache().read().ok()?.clone()
}

fn read_override_from_store(app: &tauri::AppHandle) -> Option<PathBuf> {
    let store = match app.store_builder("app_paths.json").build() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("无法创建 Store: {e}");
            return None;
        }
    };

    match store.get(STORE_KEY_APP_CONFIG_DIR) {
        Some(Value::String(path_str)) => {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                return None;
            }

            let path = resolve_path(path_str);

            if !path.exists() {
                log::warn!(
                    "Store 中配置的 app_config_dir 不存在: {path:?}\n\
                     将使用默认路径。"
                );
                return None;
            }

            log::info!("使用 Store 中的 app_config_dir: {path:?}");
            Some(path)
        }
        Some(_) => {
            log::warn!("Store 中的 {STORE_KEY_APP_CONFIG_DIR} 类型不正确，应为字符串");
            None
        }
        None => None,
    }
}

/// 从 Store 刷新 app_config_dir 覆盖值并更新缓存
pub fn refresh_app_config_dir_override(app: &tauri::AppHandle) -> Option<PathBuf> {
    let value = read_override_from_store(app);
    update_cached_override(value.clone());
    value
}

/// 写入 app_config_dir 到 Tauri Store
pub fn set_app_config_dir_to_store(
    app: &tauri::AppHandle,
    path: Option<&str>,
) -> Result<(), AppError> {
    let store = app
        .store_builder("app_paths.json")
        .build()
        .map_err(|e| AppError::Message(format!("创建 Store 失败: {e}")))?;

    match path {
        Some(p) => {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                store.set(STORE_KEY_APP_CONFIG_DIR, Value::String(trimmed.to_string()));
                log::info!("已将 app_config_dir 写入 Store: {trimmed}");
            } else {
                store.delete(STORE_KEY_APP_CONFIG_DIR);
                log::info!("已从 Store 中删除 app_config_dir 配置");
            }
        }
        None => {
            store.delete(STORE_KEY_APP_CONFIG_DIR);
            log::info!("已从 Store 中删除 app_config_dir 配置");
        }
    }

    store
        .save()
        .map_err(|e| AppError::Message(format!("保存 Store 失败: {e}")))?;

    refresh_app_config_dir_override(app);
    Ok(())
}

/// Headless（web 服务器）没有 Tauri Store：覆盖值放在同名文件 `app_paths.json` 里，
/// 键名与桌面端一致（`app_config_dir_override`）。
const HEADLESS_OVERRIDE_FILE: &str = "app_paths.json";

/// Headless override 文件的路径：`<config_dir>/cc-switch-web/app_paths.json`。
///
/// 这个目录不能放在 `get_app_config_dir()` 下面（那正是被覆盖的东西）。
/// `CC_SWITCH_TEST_HOME` 生效时落在 `<test_home>/.config/` 下：`dirs::config_dir()`
/// 在 Linux 上读 `XDG_CONFIG_HOME`，测试若不短路就会写进开发者真实的配置目录。
pub fn headless_override_file_path() -> PathBuf {
    let base = match crate::config::test_home_override() {
        Some(home) => home.join(".config"),
        None => dirs::config_dir().unwrap_or_else(|| crate::config::get_home_dir().join(".config")),
    };
    base.join("cc-switch-web").join(HEADLESS_OVERRIDE_FILE)
}

/// 读 override 文件里存的值（`~` 已展开，不检查目录是否存在），与进程内缓存无关。
///
/// 文件不存在 → `None`；JSON 损坏、键类型不对 → warning 并 `None`，服务器照常启动。
/// 这是“重启后会生效的值”，`GET /api/app-config-dir` 的 `override` 字段读它。
pub fn read_headless_override_file() -> Option<PathBuf> {
    let file = headless_override_file_path();
    let raw = match std::fs::read_to_string(&file) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!("无法读取 {}: {e}，忽略 app_config_dir 覆盖", file.display());
            return None;
        }
    };
    let doc: Value = match serde_json::from_str(&raw) {
        Ok(doc) => doc,
        Err(e) => {
            log::warn!(
                "{} 不是合法 JSON: {e}，忽略 app_config_dir 覆盖",
                file.display()
            );
            return None;
        }
    };
    match doc.get(STORE_KEY_APP_CONFIG_DIR) {
        Some(Value::String(path_str)) => {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                None
            } else {
                Some(resolve_path(path_str))
            }
        }
        Some(_) => {
            log::warn!(
                "{} 中的 {STORE_KEY_APP_CONFIG_DIR} 类型不正确，应为字符串",
                file.display()
            );
            None
        }
        None => None,
    }
}

/// 启动时把 override 文件读进缓存（`get_app_config_dir()` 的第二档）。
///
/// 必须在任何 `get_app_config_dir()` 调用之前执行。只有这个函数（和桌面端的
/// `refresh_app_config_dir_override`）写缓存：[`write_headless_override_file`] 不碰缓存，
/// 所以运行中的进程在重启前一直用旧目录。目录不存在 → warning 并 `None`。
pub fn load_headless_override() -> Option<PathBuf> {
    let value = read_headless_override_file().filter(|path| {
        let exists = path.exists();
        if !exists {
            log::warn!("override 文件中的 app_config_dir 不存在: {path:?}，将使用默认路径。");
        }
        exists
    });
    if let Some(path) = &value {
        log::info!("使用 override 文件中的 app_config_dir: {path:?}");
    }
    update_cached_override(value.clone());
    value
}

/// 原子写（临时文件 + rename）headless override 文件；`None` 删掉键，保留其他键。
///
/// 只写文件，不更新缓存：新目录在下次启动时才生效。
pub fn write_headless_override_file(path: Option<&Path>) -> Result<(), AppError> {
    let file = headless_override_file_path();
    let mut doc: Map<String, Value> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();

    match path {
        Some(p) => {
            let text = p
                .to_str()
                .ok_or_else(|| AppError::Message(format!("路径不是合法 UTF-8: {}", p.display())))?;
            doc.insert(
                STORE_KEY_APP_CONFIG_DIR.to_string(),
                Value::String(text.to_string()),
            );
        }
        None => {
            if !file.exists() {
                return Ok(());
            }
            doc.remove(STORE_KEY_APP_CONFIG_DIR);
        }
    }

    let bytes = serde_json::to_vec_pretty(&Value::Object(doc))
        .map_err(|e| AppError::Message(format!("序列化 {HEADLESS_OVERRIDE_FILE} 失败: {e}")))?;
    crate::config::atomic_write(&file, &bytes)
}

/// 解析路径，支持 ~ 开头的相对路径
fn resolve_path(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if let Some(stripped) = raw.strip_prefix("~\\") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }

    PathBuf::from(raw)
}

/// 从旧的 settings.json 迁移 app_config_dir 到 Store
pub fn migrate_app_config_dir_from_settings(app: &tauri::AppHandle) -> Result<(), AppError> {
    // app_config_dir 已从 settings.json 移除，此函数保留但不再执行迁移
    // 如果用户在旧版本设置过 app_config_dir，需要在 Store 中手动配置
    log::info!("app_config_dir 迁移功能已移除，请在设置中重新配置");

    let _ = refresh_app_config_dir_override(app);
    Ok(())
}
