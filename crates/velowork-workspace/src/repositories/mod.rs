//! Repository → Domain → Application 三层数据访问。
//!
//! - `XxxRepository`：仅做行 ↔ 领域对象的 CRUD 映射，不含业务规则。
//! - `XxxDomainService`：业务规则校验。
//! - `XxxApplicationService`：用例编排（Create/Rename/Duplicate/…），UI / AI 只调它。
//!
//! 所有写操作应经 `smol::unblock` 后台执行（见各 ApplicationService 注释），
//! 避免阻塞 GPUI 主线程。

pub mod ai;
pub mod credential;
pub mod history;
pub mod host;
pub mod service_tree;
pub mod session;
pub mod snippet;
pub mod ssh_session_tree;
pub mod tunnel_tree;
pub mod workspace;

pub use ai::{
    AiAttachmentRow, AiContextRow, AiConversationRow, AiConversationSearchResult, AiMemoryRow,
    AiMessageRow, AiRepository, AiService,
};
pub use credential::{CredentialApplicationService, CredentialRepository};
pub use history::{HistoryApplicationService, HistoryEntry, HistoryRepository};
pub use host::{Host, HostApplicationService, HostRepository};
pub use service_tree::{ServiceTreeRepository, ServiceTreeRow};
pub use session::{Session, SessionApplicationService, SessionRepository};
pub use snippet::{Snippet, SnippetApplicationService, SnippetRepository};
pub use ssh_session_tree::{SessionTreeRow, SshSessionTreeRepository};
pub use tunnel_tree::{TunnelTreeRepository, TunnelTreeRow};
pub use workspace::WorkspaceRepository;

/// 当前 ISO-8601 时间戳（UTC，秒精度），用于行 `created_at` / `updated_at`。
pub fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d) = unix_days_to_ymd(s / 86400);
    let h = (s % 86400) / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{sec:02}Z")
}

fn unix_days_to_ymd(mut n: u64) -> (u64, u64, u64) {
    let mut y = 1970u64;
    loop {
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days = if leap { 366 } else { 365 };
        if n < days {
            break;
        }
        n -= days;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let months: [u64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 1u64;
    for &days in &months {
        if n < days {
            break;
        }
        n -= days;
        mo += 1;
    }
    (y, mo, n + 1)
}
