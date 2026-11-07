//! 基于 `core::fmt` 的格式化输出，串口写入由全局自旋锁保护。

use core::fmt::{self, Write};

use crate::{sync::SpinLock, uart};

struct Console;

impl Write for Console {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            uart::putchar(byte);
        }
        Ok(())
    }
}

/// 保护一次完整格式化输出的全局锁。
///
/// 锁必须覆盖整个 `write_fmt`，只对单字符加锁仍会让两条消息在字符之间交错。
///
/// 中断处理里的回显也必须走这把锁：否则回显字符会插进另一个 hart 正在打印的
/// 消息中间（例如把 `hart 1 traps enabled` 变成 `hart K1 traps enabled`）。
/// `SpinLock` 在加锁前关闭本 hart 的 S-mode 中断，所以中断处理持锁不会自死锁。
static PRINT_LOCK: SpinLock = SpinLock::new();

pub fn print(arguments: fmt::Arguments<'_>) {
    let _guard = PRINT_LOCK.lock();
    Console.write_fmt(arguments).ok();
}

/// 在串口锁的保护下执行 `body`，用于把中断里的多次回显合并成一段不被打断的输出。
///
/// 临界区内**不能**再调用 `println!`：这把锁不可重入，重入会在同一个 hart 上自旋死锁。
pub fn with_lock<T>(body: impl FnOnce() -> T) -> T {
    let _guard = PRINT_LOCK.lock();
    body()
}

macro_rules! println {
    () => {
        $crate::console::print(format_args!("\n"))
    };
    ($format:expr $(, $argument:expr)* $(,)?) => {
        $crate::console::print(format_args!(concat!($format, "\n") $(, $argument)*))
    };
}
