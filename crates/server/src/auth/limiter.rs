//! 登录限流（D48）：按客户端 IP 记录最近 15 分钟内的失败次数。只存在内存里（D38）。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};

use jiff::{SignedDuration, Timestamp};

/// 统计失败次数的时间窗口。
pub const WINDOW: SignedDuration = SignedDuration::from_mins(15);

/// 窗口内失败到这么多次后，拒绝这个 IP 的所有登录尝试。
pub const MAX_FAILURES: usize = 5;

#[derive(Debug, Default)]
pub struct LoginLimiter {
    failures: Mutex<HashMap<IpAddr, Vec<Timestamp>>>,
}

impl LoginLimiter {
    /// 窗口内的失败次数还没到上限时，先记一次失败并返回 `true`，登录成功后再用
    /// [`LoginLimiter::succeeded`] 清掉。到了上限返回 `false`，不再记录。
    /// 先记后验证，同一 IP 并发提交的尝试也不会超过上限。
    pub fn begin_attempt(&self, ip: IpAddr, now: Timestamp) -> bool {
        let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        let cutoff = now - WINDOW;
        failures.retain(|_, times| {
            times.retain(|t| *t > cutoff);
            !times.is_empty()
        });
        let times = failures.entry(ip).or_default();
        if times.len() >= MAX_FAILURES {
            return false;
        }
        times.push(now);
        true
    }

    /// 登录成功后清空这个 IP 的记录。
    pub fn succeeded(&self, ip: IpAddr) {
        self.failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&ip);
    }
}
