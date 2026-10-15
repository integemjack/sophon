// 取消令牌：层级取消（父取消 → 子一起取消；子取消不影响父）。
// 对应 docs/ARCHITECTURE.md 的 CancelToken。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
struct CancelState {
    cancelled: AtomicBool,
    parent: Option<CancelToken>,
}

#[derive(Clone, Debug)]
pub struct CancelToken {
    state: Arc<CancelState>,
}

impl CancelToken {
    pub fn new() -> Self {
        CancelToken {
            state: Arc::new(CancelState {
                cancelled: AtomicBool::new(false),
                parent: None,
            }),
        }
    }

    /// 子令牌：父取消时子一起取消
    pub fn child(&self) -> Self {
        CancelToken {
            state: Arc::new(CancelState {
                cancelled: AtomicBool::new(false),
                parent: Some(self.clone()),
            }),
        }
    }

    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::SeqCst);
    }

    /// 自己或任意祖先被取消
    pub fn is_cancelled(&self) -> bool {
        if self.state.cancelled.load(Ordering::SeqCst) {
            return true;
        }
        if let Some(parent) = &self.state.parent {
            return parent.is_cancelled();
        }
        false
    }

    pub fn check(&self) -> anyhow::Result<()> {
        if self.is_cancelled() {
            Err(anyhow::anyhow!("cancelled"))
        } else {
            Ok(())
        }
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}
