//! [`Subscriber`] trait：事件总线上的消费者接口，以及闭包适配器。

use crate::event::Event;

/// 订阅 [`EventBus`](crate::event::EventBus) 上广播事件的消费者。
///
/// 典型实现者为长生命周期实体（AI 记忆存储、终端管理器、资源监控、状态栏、
/// 同步引擎）。订阅与 [`Subscription`](crate::event::Subscription) 句柄绑定，
/// 句柄 drop 时自动从总线移除。
pub trait Subscriber: Send + Sync {
    /// 处理单个已发布事件。
    fn on_event(&self, event: &Event);
}

/// 将闭包适配为 [`Subscriber`]，便于临时/局部订阅。
pub struct ClosureSubscriber<F> {
    handler: F,
}

impl<F> ClosureSubscriber<F>
where
    F: Fn(&Event) + Send + Sync + 'static,
{
    /// 用给定闭包创建订阅者。
    pub fn new(handler: F) -> Self {
        Self { handler }
    }
}

impl<F> Subscriber for ClosureSubscriber<F>
where
    F: Fn(&Event) + Send + Sync + 'static,
{
    fn on_event(&self, event: &Event) {
        (self.handler)(event);
    }
}
