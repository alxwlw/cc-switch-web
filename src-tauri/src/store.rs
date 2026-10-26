use crate::database::Database;
use crate::events::EventHub;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::services::{ProxyService, UsageCache};
use std::sync::Arc;

/// 全局应用状态
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    // 内部已使用细粒度锁（accounts/access_tokens/refresh_locks），所有方法均为
    // `&self`，无需外层 RwLock；避免持有粗粒度锁跨网络刷新导致的连锁阻塞。
    pub codex_oauth_manager: Arc<CodexOAuthManager>,
    /// 后端 -> 前端事件总线（桌面端桥接到 AppHandle，web 端走 SSE）
    pub events: EventHub,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let codex_oauth_manager =
            Arc::new(CodexOAuthManager::new(crate::config::get_app_config_dir()));
        let events = EventHub::new();
        let proxy_service = ProxyService::new(db.clone()).with_events(events.clone());

        Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            codex_oauth_manager,
            events,
        }
    }
}
