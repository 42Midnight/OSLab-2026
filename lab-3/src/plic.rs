//! PLIC（平台级中断控制器）驱动：只服务 QEMU virt 上 UART 的 IRQ 10。

use core::ptr::{read_volatile, write_volatile};

const PLIC: usize = 0x0c00_0000;
pub const UART_IRQ: u32 = 10;
/// UART 的 S-mode 使能位只写在 hart 0 的 context 上。
const S_ENABLE_HART0: usize = PLIC + 0x2080;
const S_THRESHOLD_HART0: usize = PLIC + 0x201000;
const S_CLAIM_HART0: usize = PLIC + 0x201004;

/// 读取寄存器地址对应的 `*mut u32`。
fn reg(address: usize) -> *mut u32 {
    address as *mut u32
}

/// 全局初始化：把 UART 中断的优先级设为 1（0 表示屏蔽该中断源）。
pub fn init() {
    // 优先级寄存器数组位于 PLIC 基址，每个中断源 4 字节。
    unsafe { write_volatile(reg(PLIC + UART_IRQ as usize * 4), 1) };
}

/// 初始化当前 hart 的 PLIC context。
///
/// 本实验只让 hart 0 接收 UART 外部中断，其他 hart 不使能、也不设 threshold，
/// 这样 UART 的每个字符只会产生一次中断。
pub fn init_hart(hart_id: usize) {
    if hart_id == 0 {
        unsafe { write_volatile(reg(S_ENABLE_HART0), 1 << UART_IRQ) };
        unsafe { write_volatile(reg(S_THRESHOLD_HART0), 0) };
    }
}

/// 取走一个待处理中断号；返回 0 表示没有待处理中断。
pub fn claim() -> u32 {
    unsafe { read_volatile(S_CLAIM_HART0 as *const u32) }
}

/// 通知 PLIC 该中断已处理完，允许同一中断源再次上报。
///
/// 必须写回 `claim` 刚取出的那个非零 IRQ，不能写死成 UART 的 10。
pub fn complete(irq: u32) {
    unsafe { write_volatile(reg(S_CLAIM_HART0), irq) };
}
