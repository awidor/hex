//! A flag one side raises and the other checks between steps.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cancels one stream or one job. Clones share the flag.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

thread_local! {
    static JOB_CANCEL: RefCell<Option<CancelToken>> = const { RefCell::new(None) };
}

/// Runs `work` with `cancel` installed as this thread's job token, so a
/// request sent inside it is skipped, and not retried, once the job's waiter
/// gave up. The engine's offline closures reach the transcriber through types
/// that know nothing of jobs.
pub(crate) fn with_job_cancel<R>(cancel: &CancelToken, work: impl FnOnce() -> R) -> R {
    struct Restore(Option<CancelToken>);
    impl Drop for Restore {
        fn drop(&mut self) {
            JOB_CANCEL.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(JOB_CANCEL.with(|slot| slot.borrow_mut().replace(cancel.clone())));
    work()
}

/// The token of the job running on this thread, if any.
pub(crate) fn job_cancel() -> Option<CancelToken> {
    JOB_CANCEL.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_one_flag() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
    }

    #[test]
    fn a_job_token_is_installed_only_while_its_work_runs() {
        assert!(job_cancel().is_none());
        let outer = CancelToken::new();
        with_job_cancel(&outer, || {
            let inner = CancelToken::new();
            inner.cancel();
            with_job_cancel(&inner, || {
                assert!(job_cancel().is_some_and(|token| token.is_cancelled()));
            });
            assert!(job_cancel().is_some_and(|token| !token.is_cancelled()));
        });
        assert!(job_cancel().is_none());
    }
}
