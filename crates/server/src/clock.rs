#![expect(
    clippy::disallowed_methods,
    reason = "clock 模块是唯一取当前时间的地方（docs/conventions.md §3）"
)]

use std::sync::{Mutex, PoisonError};

use jiff::Timestamp;

/// 当前时间的来源。服务端代码只通过它取时间，测试换成 [`FixedClock`] 就能控制时间。
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// 生产环境用的系统时钟。
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// 测试用的时钟：返回设定的时间，直到再次调用 [`FixedClock::set`]。
#[derive(Debug)]
pub struct FixedClock(Mutex<Timestamp>);

impl FixedClock {
    pub fn new(now: Timestamp) -> Self {
        Self(Mutex::new(now))
    }

    pub fn set(&self, now: Timestamp) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = now;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
