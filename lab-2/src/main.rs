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
mod sync;
mod uart;

core::arch::global_asm!(include_str!("entry.S"));

extern "C" {
    /// 链接脚本导出的内核镜像末端物理地址（页对齐）。
    fn ekernel();
}

/// hart 0 发布 UART 初始化完成状态；其他 hart 以 Acquire 等待。
static STARTED: AtomicBool = AtomicBool::new(false);

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
    println!("hart {} paging enabled", hart_id);

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
