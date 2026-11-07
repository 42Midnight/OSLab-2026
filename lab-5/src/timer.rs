//! CLINT 时钟：M-mode 预约下一次事件并转发 SSIP，S-mode 计数并清中断。

use core::sync::atomic::{AtomicUsize, Ordering};

pub const INTERVAL: usize = 1_000_000;
const CLINT_MTIME: usize = 0x0200_bff8;
const CLINT_MTIMECMP: usize = 0x0200_4000;
const MAX_HARTS: usize = 2;

static TICKS: AtomicUsize = AtomicUsize::new(0);
/// 每个 hart 五字 scratch：`[a1, a2, a3, mtimecmp, interval]`。
///
/// `timer_vector` 的偏移 24 和 32 分别对应 mtimecmp 地址与 INTERVAL，
/// 两个 hart 必须各自使用属于自己的那五个字，不能共用。
static mut TIMER_SCRATCH: [[usize; 5]; MAX_HARTS] = [[0; 5]; MAX_HARTS];

unsafe extern "C" {
    fn timer_vector();
}

/// # Safety
/// 只能由对应 hart 在 M-mode 启动阶段调用一次。
pub unsafe fn init_machine(hart_id: usize) {
    assert!(hart_id < MAX_HARTS);

    // 预约第一次时钟事件：mtimecmp = mtime + INTERVAL。
    let mtimecmp = (CLINT_MTIMECMP + hart_id * 8) as *mut usize;
    let now = unsafe { core::ptr::read_volatile(CLINT_MTIME as *const usize) };
    unsafe { core::ptr::write_volatile(mtimecmp, now.wrapping_add(INTERVAL)) };

    // 准备本 hart 的 scratch，供 timer_vector 在 M-mode 无栈环境下使用。
    let base = core::ptr::addr_of_mut!(TIMER_SCRATCH).cast::<usize>();
    let scratch = unsafe { base.add(hart_id * 5) };
    unsafe {
        scratch.add(3).write(mtimecmp as usize);
        scratch.add(4).write(INTERVAL);
        core::arch::asm!("csrw mscratch, {}", in(reg) scratch);
        core::arch::asm!("csrw mtvec, {}", in(reg) timer_vector as *const () as usize);
        // mstatus.MIE：打开 M-mode 全局中断；mie.MTIE：允许机器时钟中断。
        core::arch::asm!("csrs mstatus, {}", in(reg) 1usize << 3);
        core::arch::asm!("csrs mie, {}", in(reg) 1usize << 7);
    }
}

/// 处理一次 S-mode 软件中断（由 M-mode 时钟事件转发而来）。
///
/// 只有 hart 0 增加全局 `ticks`，避免两个 hart 把同一次时钟事件重复计数；
/// 但两个 hart 都必须清除自己的 `sip.SSIP`，否则该 hart 会陷入死循环。
pub fn update(hart_id: usize) {
    if hart_id == 0 {
        TICKS.fetch_add(1, Ordering::AcqRel);
    }
    crate::arch::clear_supervisor_software_interrupt();
}

/// Lab 3 的验收接口；Lab 4 的用户态计时由 [`crate::trap_user`] 单独统计。
#[allow(dead_code)]
pub fn ticks() -> usize {
    TICKS.load(Ordering::Acquire)
}
