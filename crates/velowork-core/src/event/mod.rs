//! 全局事件总线（EventBus），用于解耦跨模块通知。
//!
//! 生产者（工作区变更、凭据变更、同步状态等）通过 [`EventBus::publish`]
//! 广播 [`Event`]；消费者实现 [`Subscriber`]（如 AI 记忆、终端、资源监控、
//! 状态栏、同步引擎）订阅事件，取代手动刷新与 `Arc<...>` 直接传递。
//!
//! 设计要点：
//! - 进程级单例，经 [`EventBus::global`] 访问，无第三方依赖。
//! - 线程安全（`Mutex` + `Arc`），可在后台线程发布；订阅者需自行处理向
//!   GPUI 主线程的调度（如 `cx.spawn`）。
//! - 订阅以 [`Subscription`] 句柄管理生命周期：句柄 drop 时自动退订。

mod bus;
#[allow(clippy::module_inception)]
mod event;
mod subscriber;

pub use bus::{publish, EventBus, Subscription};
pub use event::Event;
pub use subscriber::{ClosureSubscriber, Subscriber};
