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
mod process;
mod sync;
mod task_context;
mod timer;
mod trap;
mod trap_user;
mod uart;
mod user_image;

core::arch::global_asm!(include_str!("entry.S"));
core::arch::global_asm!(include_str!("trap.S"));
core::arch::global_asm!(include_str!("trap_user.S"));
core::arch::global_asm!(include_str!("switch.S"));

extern "C" {
    /// 链接脚本导出的内核镜像末端物理地址（页对齐）。
    fn ekernel();
    /// 保存当前内核上下文并恢复下一个内核上下文。
    fn __switch(current: *mut task_context::TaskContext, next: *const task_context::TaskContext);
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

        // 委托：S-mode 的异常与软件/时钟/外部中断都从内核 trap 入口进入。
        // 用户态 trap 的入口由 trap_return 在进入 U-mode 前改成 trampoline。
        arch::write_medeleg(MEDELEG_ALL);
        arch::write_mideleg(MIDELEG_S_MASK);

        // 时钟在 M-mode 处理：预约第一次时钟事件并准备 timer_vector 的现场。
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
            halt();
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

    if hart_id != 0 {
        // 本实验只创建一个用户进程，由 hart 0 运行；hart 1 继续响应自己的时钟中断。
        loop {
            arch::wait_for_interrupt();
        }
    }

    // 启动顺序：分页 → PLIC/trap → 构造进程 → layout_check → 设置当前进程
    // → 初始化 trap context → trap_return → sret。

    // 先验证错误路径：把镜像的 p_memsz 改成超过装载页数上限后，装载器必须在
    // 返回错误的同时把已经申请的页帧全部归还，否则后续进程会缺页。
    let mut oversized = [0u8; 0x110];
    oversized.copy_from_slice(user_image::USER_ELF);
    oversized[104..112].copy_from_slice(&0x10_0000u64.to_le_bytes());
    let frames_before = mm::free_frame_count();
    let rejected = process::UserProcess::from_elf(&oversized).is_err();
    let frames_after = mm::free_frame_count();
    let rollback_ok = rejected && frames_before == frames_after;
    println!(
        "lab-4 elf rollback test: {}",
        if rollback_ok { "PASS" } else { "FAIL" }
    );
    if !rollback_ok {
        halt();
    }

    let process = match process::UserProcess::from_elf(user_image::USER_ELF) {
        Ok(process) => process,
        Err(error) => {
            println!("lab-4 elf load failed: {}", error);
            halt();
        }
    };
    println!(
        "lab-4 first process: pid={} entry={:#x} segments={} heap_top={:#x} user_sp={:#x} stack_pages={} pages={}",
        process.pid(),
        process.entry(),
        process.segment_count(),
        process.heap_top(),
        process.user_sp(),
        process.user_stack_pages(),
        process.loaded_pages()
    );

    let address_space_ok = process.layout_check();
    println!(
        "lab-4 address-space test: {}",
        if address_space_ok { "PASS" } else { "FAIL" }
    );
    if !address_space_ok {
        halt();
    }

    process::set_current(process);
    let current = match process::current() {
        Some(current) => current,
        None => halt(),
    };
    current.init_trap_context(token);
    trap_user::init();

    // 用清零的内核上下文作为 current 调用 __switch：恢复后的 ra 是 trap_return，
    // 因此 ret 会进入用户态返回流程，而不是从 rust_main 直接调用用户入口。
    arch::disable_interrupts();
    let mut idle = task_context::TaskContext::zero();
    unsafe {
        let next = current.task_context_mut() as *mut task_context::TaskContext;
        __switch(&mut idle, next);
    }
    halt()
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    halt()
}

fn halt() -> ! {
    arch::disable_interrupts();
    loop {
        arch::wait_for_interrupt();
    }
}
