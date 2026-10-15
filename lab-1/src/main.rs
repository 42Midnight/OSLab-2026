#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use core::panic::PanicInfo;

mod arch;
#[macro_use]
mod console;
mod sync;
mod uart;

core::arch::global_asm!(include_str!("entry.S"));

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
        core::arch::asm!("mret", options(noreturn));
    }
}

#[no_mangle]
extern "C" fn rust_main() -> ! {
    let hart_id = arch::hart_id();

    // 只有 hart 0 初始化 UART；初始化完成后以 Release 发布。
    if hart_id == 0 {
        uart::init();
        STARTED.store(true, Ordering::Release);

        println!("ECNU OSLab rCore kernel entered S-mode");
        println!(
            "interrupt nesting self-test: {}",
            if sync::interrupt_nesting_selftest() {
                "PASS"
            } else {
                "FAIL"
            }
        );
        println!(
            "Rust formatting: char={} string={} d={} p={:x} x={:#x}",
            'A', "uart", -2025, 0x2025_u32, 0x1234_5678_8000_0000_u64,
        );
    } else {
        // 其他 hart 以 Acquire 等待，保证观察到 STARTED 时 UART 已初始化完成。
        while !STARTED.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }

    if hart_id == 0 {
        println!(
            "hart {} says: char={} string={} d={} p={:x} x={:#x}",
            hart_id, 'A', "uart", -2025, 0x2025_u32, 0x1234_5678_8000_0000_u64,
        );
    } else {
        println!(
            "hart {} says: char={} string={} d={} p={:x} x={:#x}",
            hart_id, 'B', "uart", -2025, 0x2025_u32, 0x1234_5678_8000_0000_u64,
        );
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
