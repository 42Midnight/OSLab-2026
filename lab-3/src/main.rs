#![no_std]
#![no_main]

use core::{
    arch::asm,
    panic::PanicInfo,
    sync::atomic::{AtomicBool, Ordering},
};

mod arch;
#[macro_use]
mod console;
mod mm;
mod plic;
mod sync;
mod timer;
mod trap;
mod uart;

core::arch::global_asm!(include_str!("entry.S"));
core::arch::global_asm!(include_str!("trap.S"));

extern "C" {
    /// 链接脚本导出的内核镜像末端物理地址（页对齐）。
    fn ekernel();
}

/// hart 0 发布 UART 初始化完成状态；其他 hart 以 Acquire 等待。
static STARTED: AtomicBool = AtomicBool::new(false);

/// hart 0 发布 trap/PLIC 全局初始化完成状态；hart 1 以 Acquire 等待。
static TRAPS_READY: AtomicBool = AtomicBool::new(false);

/// M-mode 委托掩码：把标准同步异常全部交给 S-mode。
const MEDELEG_ALL: usize = 0xffff;

/// M-mode 委托掩码：S-mode 软件中断（1）、时钟中断（5）、外部中断（9）。
///
/// 机器时钟中断 MTIP 不在此列，它由 M-mode 的 `timer_vector` 转成 SSIP。
const MIDELEG_S_MASK: usize = (1 << 1) | (1 << 5) | (1 << 9);

#[no_mangle]
extern "C" fn start(hart_id: usize) -> ! {
    let mut mstatus = arch::read_mstatus();
    mstatus &= !arch::MSTATUS_MPP_MASK;
    mstatus |= arch::MSTATUS_MPP_S;

    unsafe {
        arch::write_mstatus(mstatus);
        arch::write_mepc(rust_main as *const () as usize);
        arch::write_satp(0);
        arch::write_pmpaddr0(usize::MAX);
        arch::write_pmpcfg0(arch::PMP_R | arch::PMP_W | arch::PMP_X | arch::PMP_A_TOR);
        arch::write_tp(hart_id);

        // 委托：S-mode 的异常与软件/时钟/外部中断都从 kernel_vector 进入。
        arch::write_medeleg(MEDELEG_ALL);
        arch::write_mideleg(MIDELEG_S_MASK);

        // 时钟在 M-mode 处理：预约第一次时钟事件并准备 timer_vector 的现场。
        // 必须在 mret 之前完成，mret 之后当前 hart 就运行在 S-mode。
        timer::init_machine(hart_id);

        asm!("mret", options(noreturn));
    }
}

#[no_mangle]
extern "C" fn rust_main() -> ! {
    let hart_id = arch::hart_id();

    if hart_id == 0 {
        uart::init();
        STARTED.store(true, Ordering::Release);

        // 只有 hart 0 建立空闲页帧链表。
        unsafe {
            mm::frame_allocator_init(
                mm::PhysAddr(ekernel as *const () as usize),
                mm::PhysAddr(0x8800_0000),
            );
        }

        // 先运行内存自检；任一项失败都不得继续启用分页。
        let tests_ok = mm::run_tests();
        if !tests_ok {
            println!("lab-2 memory self-test failed; paging stays disabled");
            loop {
                arch::wait_for_interrupt();
            }
        }
    } else {
        // 先确认 UART 可用，保证后续任何输出都不会丢失。
        while !STARTED.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }

    // 两个 hart 共用 hart 0 建立的同一张内核页表。
    let token = loop {
        let token = mm::kernel_token();
        if token != 0 {
            break token;
        }
        core::hint::spin_loop();
    };
    unsafe { arch::activate_page_table(token) };

    // 启动顺序：PLIC 页已映射 → PLIC 全局初始化 → 每 hart 写 stvec 并开中断
    // → UART 接收使能。PLIC 的优先级寄存器只在分页建立后才可访问。
    if hart_id == 0 {
        trap::init_global();
        TRAPS_READY.store(true, Ordering::Release);
    } else {
        while !TRAPS_READY.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }
    trap::init_hart();
    if hart_id == 0 {
        uart::enable_receive_interrupt();
    }
    println!("hart {} traps enabled", hart_id);

    if hart_id == 0 {
        println!(
            "lab-3 context test: {}",
            if trap::context_layout_ok() {
                "PASS"
            } else {
                "FAIL"
            }
        );

        // 等待五次连续时钟中断再判定通过（讲义要求至少 3 次）。
        // 阈值取 5 而不是 3，是为了让“M-mode 每次重新预约 mtimecmp + S-mode 清 SSIP”
        // 必须连续成立 5 轮；若 mtimecmp 没有续约或 SSIP 没被清掉，计数会停在 1。
        while timer::ticks() < 5 {
            arch::wait_for_interrupt();
        }
        println!("lab-3 timer test: PASS ticks={}", timer::ticks());
    }

    loop {
        arch::wait_for_interrupt();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        arch::wait_for_interrupt();
    }
}
