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

/// 写 `medeleg`：把同步异常委托给 S-mode 处理。
///
/// Lab 3 的验收路径不产生异常，但按讲义要求把标准异常全部委托下去，
/// 使 S-mode 的 `kernel_vector` 成为异常的唯一入口，而不是被 M-mode 截获。
#[inline]
pub unsafe fn write_medeleg(value: usize) {
    unsafe { asm!("csrw medeleg, {}", in(reg) value) };
}

/// 写 `mideleg`：把 S-mode 软件、时钟、外部中断委托给 S-mode 处理。
#[inline]
pub unsafe fn write_mideleg(value: usize) {
    unsafe { asm!("csrw mideleg, {}", in(reg) value) };
}

/// 写 `stvec`：设置 S-mode 的 trap 入口地址。
///
/// 低两位为 MODE，0 表示 Direct 模式，因此入口地址必须四字节对齐。
#[inline]
pub unsafe fn set_stvec(entry: usize) {
    unsafe { asm!("csrw stvec, {}", in(reg) entry) };
}

/// 读 `scause`：trap 原因，最高位区分中断与异常。
#[inline]
pub fn read_scause() -> usize {
    let value;
    unsafe { asm!("csrr {}, scause", out(reg) value) };
    value
}

/// 读 `sepc`：被打断指令的地址。
#[inline]
pub fn read_sepc() -> usize {
    let value;
    unsafe { asm!("csrr {}, sepc", out(reg) value) };
    value
}

/// 读 `stval`：异常附加信息（如出错地址）。
#[inline]
pub fn read_stval() -> usize {
    let value;
    unsafe { asm!("csrr {}, stval", out(reg) value) };
    value
}

/// 打开 S-mode 已实现的中断源和全局中断开关。
///
/// `sie.SSIE`（bit 1）接收 M-mode 时钟转发来的软件中断，
/// `sie.SEIE`（bit 9）接收 PLIC 送来的 UART 外部中断。
/// 未实现的 `sie.STIE`（bit 5）保持关闭：本实验不处理 S-mode 时钟中断。
/// 写 `sstatus.SIE` 必须是初始化的最后一步，否则中断可能先于入口准备完成而到达。
#[inline]
pub fn enable_supervisor_trap_interrupts() {
    unsafe { asm!("csrs sie, {}", in(reg) (1usize << 1) | (1usize << 9)) };
    unsafe { asm!("csrs sstatus, {}", in(reg) SSTATUS_SIE) };
}

/// 清除 `sip.SSIP`，通知软件中断已处理。
///
/// 使用 `csrc` 只清 bit 1，不会覆盖 `sip` 中的其他位。
#[inline]
pub fn clear_supervisor_software_interrupt() {
    unsafe { asm!("csrc sip, {}", in(reg) 1usize << 1) };
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
