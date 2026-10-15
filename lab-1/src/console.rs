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
static PRINT_LOCK: SpinLock = SpinLock::new();

pub fn print(arguments: fmt::Arguments<'_>) {
    let _guard = PRINT_LOCK.lock();
    Console.write_fmt(arguments).ok();
}

macro_rules! println {
    () => {
        $crate::console::print(format_args!("\n"))
    };
    ($format:expr $(, $argument:expr)* $(,)?) => {
        $crate::console::print(format_args!(concat!($format, "\n") $(, $argument)*))
    };
}
