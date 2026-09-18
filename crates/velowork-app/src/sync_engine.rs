//! 自动同步引擎：在后台持续运行，按「定时」或「配置变更」触发 WebDAV 快照同步。
//!
//! 放在 `velowork-app` 而非 `velowork-workspace`，是因为它要读取运行中的
//! `SettingsState`（`velowork-app-core`）并操作 UI 提示（`ToastManager`），
//! 而 `velowork-workspace` 不能反向依赖 `velowork-app-core`。信号通道的全局状态
//! 仍由 `velowork-workspace::sync::engine` 持有，本模块在启动时注册发送端。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::channel::mpsc;
use futures::future::{select, Either};
use futures::StreamExt;
use gpui::{App, AppContext as _, Entity, Global};

use velowork_app_core::settings::settings_entity;
use velowork_core::profiles::ProfilePaths;
use velowork_core::storage::{Database, database};
use velowork_i18n::i18n;
use velowork_terminal::pty_manager::get_tokio_runtime;
use velowork_workspace::repositories::credential::CredentialApplicationService;
use velowork_workspace::secure_storage::load_sync_passphrase;
use velowork_workspace::security::current_security_service;
use velowork_workspace::settings::{SyncProvider, SyncSettings};
use velowork_workspace::sync::{
    create_sync_provider, register_sync_signal, sync_snapshot, AnySyncProvider, SyncResult,
};
use velowork_workspace::toast::{Toast, ToastAction, ToastActionStyle, ToastManager};

/// 防止手动同步与自动同步并发执行同一份基线文件。
static SYNCING: AtomicBool = AtomicBool::new(false);

/// 同步结果概览，用于状态栏 `sync_status_btn` 的圆点着色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncOutcome {
    /// 同步成功（绿点）。
    Success,
    /// 存在冲突（黄点）。
    Conflict,
    /// 同步失败（红点）。
    Error,
}

/// 全局同步运行时状态，供状态栏读取以驱动同步状态按钮。
///
/// 仿照 `GlobalTransferStore`：以可观察的 GPUI 实体持有，状态变更时 `cx.notify()`，
/// 状态栏通过 `cx.observe` 订阅后自动重渲染。
#[derive(Default)]
pub struct SyncStatusStore {
    /// 是否正在同步（状态栏此时显示刷新图标）。
    pub syncing: bool,
    /// 上一次同步的结果概览（`None` 表示本次运行尚未同步过）。
    pub last_outcome: Option<SyncOutcome>,
    /// 数据恢复/拉取同步完成的版本计数。每次拉取或恢复覆盖本地数据库时自增，
    /// 触发依赖数据库全量状态的面板（历史命令、AI助手、会话等）重新加载。
    pub restore_epoch: u64,
}

/// `SyncStatusStore` 的全局包装。
#[derive(Clone)]
pub struct GlobalSyncStatus(pub Entity<SyncStatusStore>);

impl Global for GlobalSyncStatus {}

/// 通知所有订阅者：本地数据已通过云端同步或还原进行了重载。
pub fn notify_runtime_state_restored(cx: &mut App) {
    if let Some(store) = sync_status_store(cx) {
        store.update(cx, |s, cx| {
            s.restore_epoch = s.restore_epoch.wrapping_add(1);
            cx.notify();
        });
    }
}

/// 初始化全局同步状态存储。应在应用启动、全局设置就绪后调用一次。
pub fn init_sync_status(cx: &mut App) {
    let store = cx.new(|_cx| SyncStatusStore::default());
    cx.set_global(GlobalSyncStatus(store));
}

/// 获取全局同步状态实体（若已初始化）。
pub fn sync_status_store(cx: &App) -> Option<Entity<SyncStatusStore>> {
    cx.try_global::<GlobalSyncStatus>().map(|g| g.0.clone())
}

/// 依据同步结果推断状态栏圆点应显示的概览状态。
fn outcome_of(result: &anyhow::Result<SyncResult>) -> SyncOutcome {
    match result {
        Err(_) => SyncOutcome::Error,
        Ok(r) if !r.errors.is_empty() => SyncOutcome::Error,
        Ok(r) if r.conflicts > 0 => SyncOutcome::Conflict,
        Ok(_) => SyncOutcome::Success,
    }
}

/// 设置「正在同步」标志并通知状态栏刷新。
fn set_syncing_flag(cx: &mut App, syncing: bool) {
    if let Some(store) = sync_status_store(cx) {
        store.update(cx, |s, cx| {
            s.syncing = syncing;
            cx.notify();
        });
    }
}

/// 记录一次同步结果并清除「正在同步」标志。
fn set_sync_outcome(cx: &mut App, outcome: SyncOutcome) {
    if let Some(store) = sync_status_store(cx) {
        store.update(cx, |s, cx| {
            s.syncing = false;
            s.last_outcome = Some(outcome);
            cx.notify();
        });
    }
}

/// 手动触发一次同步（由状态栏 `sync_status_btn` 点击触发）。
///
/// 与自动同步共用 `SYNCING` 原子标志以避免并发；密码从系统密钥库读取，
/// 因此无需任何输入框交互即可在状态栏直接发起同步。
pub fn trigger_manual_sync(cx: &mut App) {
    let sync = settings_entity(cx).read(cx).settings.sync.clone();

    // 1. 校验同步源必填配置（若未配置服务器地址或未合规，给予用户明确 Toast 引导）。
    if let Err(err) = sync.validate_configuration() {
        let err_reason = i18n!(cx, err.translation_key());
        // 避免快速连击重复堆叠 Toast：先清理旧的未配置提醒 Toast
        ToastManager::dismiss("sync_unconfigured_warning", cx);

        let mut toast = Toast::warning(format!(
            "{}: {}",
            i18n!(cx, "settings.sync.config_incomplete"),
            err_reason
        ))
        .with_actions(vec![ToastAction::new(
            "open_sync_settings",
            i18n!(cx, "settings.sync.go_to_settings"),
            ToastActionStyle::Primary,
        )])
        .with_ttl(Duration::from_secs(6));
        toast.id = "sync_unconfigured_warning".to_string();
        ToastManager::post(toast, cx);
        return;
    }

    // 2. 已有同步进行中（自动或手动），忽略本次点击。
    if SYNCING.swap(true, Ordering::SeqCst) {
        return;
    }
    set_syncing_flag(cx, true);

    cx.spawn(async move |cx| {
        let result = run_auto_sync(sync).await;
        SYNCING.store(false, Ordering::SeqCst);
        // 回到主线程：更新状态圆点 + 记录时间 + Toast 提示（手动触发始终提示）。
        let _ = cx.update(|cx| {
            set_sync_outcome(cx, outcome_of(&result));
            show_auto_sync_result(&result, true, cx);
        });
    })
    .detach();
}

/// 启动自动同步引擎。应在全局设置与 ToastManager 就绪后调用一次。
pub fn start_sync_engine(cx: &App) {
    let (tx, mut rx) = mpsc::unbounded::<()>();
    register_sync_signal(tx);

    cx.spawn(async move |cx| {
        loop {
            // 等待下一次触发：定时到期 或 配置变更信号（避免应用启动时无故立即执行一次同步）。
            let sync = cx.update(|cx| settings_entity(cx).read(cx).settings.sync.clone());
            let interval = Duration::from_secs(sync.sync_interval_secs.max(60) as u64);
            let timer = smol::Timer::after(interval);
            let signal = rx.next();
            match select(timer, signal).await {
                Either::Left(_) | Either::Right(_) => {}
            }

            // 读取最新同步配置
            let sync = cx.update(|cx| settings_entity(cx).read(cx).settings.sync.clone());
            let is_security_unlocked = current_security_service().map(|s| s.is_unlocked()).unwrap_or(true);
            if sync.enabled && sync.auto_sync && sync.validate_configuration().is_ok() && is_security_unlocked {
                // 短暂防抖，合并连续变更信号
                smol::Timer::after(Duration::from_secs(2)).await;

                let is_unlocked = current_security_service().map(|s| s.is_unlocked()).unwrap_or(true);
                if is_unlocked && !SYNCING.swap(true, Ordering::SeqCst) {
                    // 标记「同步中」，状态栏切换为刷新图标
                    let _ = cx.update(|cx| set_syncing_flag(cx, true));
                    let result = run_auto_sync(sync).await;
                    SYNCING.store(false, Ordering::SeqCst);
                    // 回到主线程更新状态圆点（后台自动同步静默执行，仅失败时提示）
                    let _ = cx.update(|cx| {
                        set_sync_outcome(cx, outcome_of(&result));
                        show_auto_sync_result(&result, false, cx);
                    });
                }
            }
        }
    })
    .detach();
}

/// 解析同步加密口令：优先使用持久化的同步加密口令，否则回退到提供商认证密钥
/// （用户已为同步提供的密钥，跨设备一致）。
///
/// **不再使用固定默认口令兜底**：默认口令等于把所有人同步 Bundle 用同一已知密钥
/// 加密，是安全隐患。若两者皆空，同步必须失败并提示用户设置同步加密口令，而非
/// 静默用弱默认密钥加密。
fn resolve_sync_passphrase(fallback_secret: &str) -> anyhow::Result<String> {
    if let Some(p) = load_sync_passphrase()
        && !p.is_empty()
    {
        return Ok(p);
    }
    if !fallback_secret.is_empty() {
        return Ok(fallback_secret.to_string());
    }
    anyhow::bail!(
        "未配置同步加密口令：请在设置中设置「同步加密口令」，或配置提供商访问凭证用于同步加密"
    )
}

/// 构建同步所需的运行时上下文（provider / profile / cred / db / passphrase）。
///
/// `override_secret` 为已解析的提供商密钥（来自输入框或系统密钥库）。
/// 若为 `None`，则从安全存储中自动加载。
/// 返回的 `profile` 为进程级 `&'static` 引用，可直接传入 `sync_snapshot`。
pub(crate) fn build_sync_context(
    sync: &SyncSettings,
    override_secret: Option<&str>,
) -> anyhow::Result<(
    AnySyncProvider,
    &'static ProfilePaths,
    CredentialApplicationService,
    Option<Arc<Database>>,
    String,
)> {
    let secret = match override_secret {
        Some(s) => s.to_string(),
        None => match sync.provider {
            SyncProvider::WebDav => {
                velowork_workspace::secure_storage::load_webdav_password().unwrap_or_default()
            }
            SyncProvider::S3 => {
                velowork_workspace::secure_storage::load_s3_secret_key().unwrap_or_default()
            }
        },
    };

    let provider = create_sync_provider(sync, if secret.is_empty() { None } else { Some(&secret) })?;
    let profile = velowork_core::profiles::current();
    let db = database().or_else(|| {
        Database::open(&profile.database_path())
            .ok()
            .map(Arc::new)
    });
    // CredentialApplicationService 需要持有 DB 句柄用于凭据元数据；若全局句柄
    // 暂不可用，则直接打开 Profile 业务库文件（导入/导出凭据元数据所需）。
    let cred_db = match db.clone() {
        Some(d) => d,
        None => Arc::new(
            Database::open_in_memory().map_err(|e| anyhow::anyhow!("打开内存库失败: {e}"))?,
        ),
    };
    // 凭据明文经 SecurityService（Level 1 SQLite，DEK 加密）读写。
    let security = current_security_service()?;
    let cred = CredentialApplicationService::new(cred_db, security);
    let passphrase = resolve_sync_passphrase(&secret)?;
    Ok((provider, profile, cred, db, passphrase))
}

/// 执行一次自动同步（在共享 Tokio 运行时上跑网络调用）。
///
/// 不持有 `cx`：网络调用与结果展示解耦，避免 `&mut AsyncApp` 跨 `.await` 导致
/// 的生命周期错误。结果由调用方在 `cx.update` 中展示。
async fn run_auto_sync(sync: SyncSettings) -> anyhow::Result<SyncResult> {
    let strategy = sync.conflict_strategy;
    let scope = sync.data_scope.clone();
    let (provider, profile, cred, db, passphrase) = build_sync_context(&sync, None)?;

    get_tokio_runtime()
        .spawn(async move {
            sync_snapshot(&provider, profile, &cred, db, &passphrase, strategy, &scope).await
        })
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("{}", e)))
}

/// 在主线程展示一次同步的结果（更新最近同步时间 + 视情况展示 Toast 提示）。
fn show_auto_sync_result(result: &anyhow::Result<SyncResult>, is_manual: bool, cx: &mut App) {
    match result {
        Ok(r) => {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            settings_entity(cx).update(cx, |state, cx| state.set_last_sync_at(Some(now), cx));
            if r.downloaded > 0 {
                settings_entity(cx).update(cx, |state, cx| {
                    state.reload_settings(cx);
                });
                crate::keybindings::reload_keybindings(cx);
                crate::views::overlays::settings::settings_panel::render_sync::reload_runtime_state_after_restore(cx);
            }
            if !r.errors.is_empty() {
                ToastManager::warning(
                    format!(
                        "{}（{} errors）",
                        i18n!(cx, "settings.sync.auto_sync_done"),
                        r.errors.len()
                    ),
                    cx,
                );
            } else if is_manual {
                ToastManager::success(
                    format!(
                        "{}：↑{} ↓{} ⚠{}",
                        i18n!(cx, "settings.sync.auto_sync_done"),
                        r.uploaded,
                        r.downloaded,
                        r.conflicts
                    ),
                    cx,
                );
            }
        }
        Err(e) => {
            if is_manual {
                ToastManager::error(
                    format!("{}: {}", i18n!(cx, "settings.sync.auto_sync_failed"), e),
                    cx,
                );
            } else {
                log::warn!("[sync] 后台自动同步失败: {:#}", e);
            }
        }
    }
}
