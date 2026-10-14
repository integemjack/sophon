// 主线程命令队列：对应 docs/ARCHITECTURE.md 的 runOnMain / runOnMainSync。
// 界面线程持有 AppCore；工作线程把闭包投进队列，界面每帧执行。
// （本程序是单进程本地应用，用命令队列而不是跨线程锁，
//  与文档「主线程修改被界面观察的状态、工作线程阻塞式顺序书写」的约定一致。）
use crossbeam_channel::{Receiver, Sender};

/// 对 AppCore 的一次修改命令
pub type CoreCommand = Box<dyn FnOnce(&mut crate::AppCore) + Send>;

#[derive(Clone)]
pub struct MainThread {
    tx: Sender<CoreCommand>,
}

pub struct MainThreadReceiver {
    rx: Receiver<CoreCommand>,
}

pub fn channel() -> (MainThread, MainThreadReceiver) {
    let (tx, rx) = crossbeam_channel::unbounded::<CoreCommand>();
    (MainThread { tx }, MainThreadReceiver { rx })
}

impl MainThread {
    /// 排队到主线程执行（== runOnMain）
    pub fn post(&self, f: impl FnOnce(&mut crate::AppCore) + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
    }
}

impl MainThreadReceiver {
    /// 取出并执行全部排队的命令；返回是否有命令（有则界面需要重绘）
    pub fn drain(&self, core: &mut crate::AppCore) -> bool {
        let mut any = false;
        while let Ok(cmd) = self.rx.try_recv() {
            cmd(core);
            any = true;
        }
        any
    }
}
