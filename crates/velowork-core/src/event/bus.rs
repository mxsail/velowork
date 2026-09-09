//! [`EventBus`] 实现：进程级单例发布/订阅。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::event::{Event, Subscriber};

struct EventBusInner {
    subscribers: Mutex<Vec<(usize, Arc<dyn Subscriber>)>>,
    next_id: AtomicUsize,
}

/// 轻量、线程安全的发布/订阅事件总线。
///
/// 内部以 `Arc` 承载，故可 `Clone`；[`EventBus::global`] 返回进程级单例，
/// [`EventBus::new`] 创建独立实例（测试/隔离场景）。订阅者以
/// `(id, Arc<dyn Subscriber>)` 存储；发布时克隆订阅者列表，使回调内部可安全
/// （退）订阅而不死锁。
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

static BUS: OnceLock<EventBus> = OnceLock::new();

impl EventBus {
    /// 返回进程级事件总线（单例），首次使用时初始化。
    pub fn global() -> &'static EventBus {
        BUS.get_or_init(|| EventBus {
            inner: Arc::new(EventBusInner {
                subscribers: Mutex::new(Vec::new()),
                next_id: AtomicUsize::new(1),
            }),
        })
    }

    /// 创建独立实例（测试或隔离场景使用）。
    pub fn new() -> Self {
        EventBus {
            inner: Arc::new(EventBusInner {
                subscribers: Mutex::new(Vec::new()),
                next_id: AtomicUsize::new(1),
            }),
        }
    }

    /// 订阅一个消费者，返回 [`Subscription`]，其 drop 时自动退订。
    pub fn subscribe(&self, subscriber: Arc<dyn Subscriber>) -> Subscription {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        self.lock_subscribers().push((id, subscriber));
        Subscription {
            bus: self.clone(),
            id,
        }
    }

    /// 向所有当前订阅者广播事件。
    pub fn publish(&self, event: Event) {
        let subscribers = self
            .lock_subscribers()
            .iter()
            .map(|(_, s)| s.clone())
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            subscriber.on_event(&event);
        }
    }

    /// 按订阅 id 从总线移除（id 唯一，避免 `Arc::ptr_eq` 在 trait object 上的歧义）。
    fn remove(&self, id: usize) {
        self.lock_subscribers().retain(|(sid, _)| *sid != id);
    }

    /// 安全加锁：遇到 poison 时恢复内部数据而非 panic。
    fn lock_subscribers(&self) -> MutexGuard<'_, Vec<(usize, Arc<dyn Subscriber>)>> {
        match self.inner.subscribers.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        EventBus {
            inner: self.inner.clone(),
        }
    }
}

/// [`EventBus::subscribe`] 返回的句柄。其 drop 会退订关联的
/// [`Subscriber`]，实现 RAII 式生命周期管理。
pub struct Subscription {
    bus: EventBus,
    id: usize,
}

impl Subscription {
    /// 显式退订（等价于 drop 句柄）。
    pub fn unsubscribe(self) {
        self.bus.remove(self.id);
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.bus.remove(self.id);
    }
}

/// 便捷函数：在全局总线上发布事件。
pub fn publish(event: Event) {
    EventBus::global().publish(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ClosureSubscriber;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn publish_reaches_subscriber() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let sub = Arc::new(ClosureSubscriber::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        let _sub = bus.subscribe(sub);
        bus.publish(Event::SyncDirty);
        bus.publish(Event::SyncDirty);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn unsubscribe_stops_events() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let sub = Arc::new(ClosureSubscriber::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        {
            let _s = bus.subscribe(sub);
            bus.publish(Event::SyncDirty);
        } // 此处 drop → 自动退订
        bus.publish(Event::SyncDirty);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn explicit_unsubscribe_works() {
        let bus = EventBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let sub = Arc::new(ClosureSubscriber::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        let s = bus.subscribe(sub);
        bus.publish(Event::SyncDirty);
        s.unsubscribe();
        bus.publish(Event::SyncDirty);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
