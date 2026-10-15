//! CSR、中断与机器相关操作封装。

use core::arch::asm;

pub const MSTATUS_MPP_MASK: usize = 3 << 11;
pub const MSTATUS_MPP_S: usize = 1 << 11;
pub const PMP_R: usize = 1 << 0;
pub const PMP_W: usize = 1 << 1;
pub const PMP_X: usize = 1 << 2;
pub const PMP_A_TOR: usize = 1 << 3;
const SSTATUS_SIE: usize = 1 << 1;

#[inline]
pub fn read_mstatus() -> usize {
    let value;
    unsafe { asm!("csrr {}, mstatus", out(reg) value) };
    value
}

#[inline]
pub unsafe fn write_mstatus(value: usize) {
    unsafe { asm!("csrw mstatus, {}", in(reg) value) };
}

#[inline]
pub unsafe fn write_mepc(value: usize) {
    unsafe { asm!("csrw mepc, {}", in(reg) value) };
}

#[inline]
pub unsafe fn write_satp(value: usize) {
    unsafe { asm!("csrw satp, {}", in(reg) value) };
}

/// 写 `satp` 选择页表，并执行 `sfence.vma` 丢弃此前的地址转换缓存。
///
/// # Safety
/// `token` 必须指向仍然存活的 Sv39 根页，且当前 PC、栈、UART 和页表页都已映射。
#[inline]
pub unsafe fn activate_page_table(token: usize) {
    unsafe {
        asm!("csrw satp, {}", in(reg) token);
        asm!("sfence.vma");
    }
}

#[inline]
pub unsafe fn write_pmpaddr0(value: usize) {
    unsafe { asm!("csrw pmpaddr0, {}", in(reg) value) };
}

#[inline]
pub unsafe fn write_pmpcfg0(value: usize) {
    unsafe { asm!("csrw pmpcfg0, {}", in(reg) value) };
}

#[inline]
pub unsafe fn write_tp(value: usize) {
    unsafe { asm!("mv tp, {}", in(reg) value) };
}

#[inline]
pub fn hart_id() -> usize {
    let value;
    unsafe { asm!("mv {}, tp", out(reg) value) };
    value
}

#[inline]
fn read_sstatus() -> usize {
    let value;
    unsafe { asm!("csrr {}, sstatus", out(reg) value) };
    value
}

/// 当前 hart 的 S-mode 中断是否开启。
///
/// Lab 2 的 `rust_main` 不再调用 Lab 1 的中断嵌套自检，但保留该 CSR 读取接口
/// 以便后续实验（trap / 时钟中断）和排错使用。
#[inline]
#[allow(dead_code)]
pub fn interrupts_enabled() -> bool {
    read_sstatus() & SSTATUS_SIE != 0
}

#[inline]
pub fn enable_interrupts() {
    unsafe { asm!("csrs sstatus, {}", in(reg) SSTATUS_SIE) };
}

#[inline]
pub fn disable_interrupts() {
    unsafe { asm!("csrc sstatus, {}", in(reg) SSTATUS_SIE) };
}

#[inline]
pub fn wait_for_interrupt() {
    unsafe { asm!("wfi") };
}
