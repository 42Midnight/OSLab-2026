//! 内核态 trap 分发：Lab 3 的时钟与 UART 路径继续为 hart 1 服务。
//!
//! hart 0 进入用户态后 `stvec` 被改成 trampoline，用户 trap 由
//! [`crate::trap_user`] 处理；本文件的入口只负责内核态（主要是 hart 1）的中断。

use core::sync::atomic::{AtomicBool, Ordering};

use crate::{arch, plic, timer, uart};

const SCAUSE_INTERRUPT: usize = 1usize << (usize::BITS - 1);
const S_SOFTWARE: usize = 1;
const S_EXTERNAL: usize = 9;

/// 内核态收到验收字符时只报告一次。
static UART_REPORTED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn kernel_vector();
}

#[repr(C)]
pub struct KernelTrapContext {
    pub x: [usize; 32],
}

/// Lab 3 的自检接口；Lab 4 的启动流程不再调用，保留以便回归验证。
#[allow(dead_code)]
pub fn context_layout_ok() -> bool {
    core::mem::size_of::<KernelTrapContext>() == 32 * core::mem::size_of::<usize>()
}

/// 全局 trap 初始化：PLIC 优先级只需要设置一次，因此只由 hart 0 调用。
pub fn init_global() {
    plic::init();
}

/// 当前 hart 的 trap 初始化。
///
/// 顺序不能颠倒：先把入口写好、把 PLIC 的本 hart 上下文准备好，
/// 最后才打开中断开关，否则中断可能在没有入口的情况下到达。
pub fn init_hart() {
    unsafe { arch::set_stvec(kernel_vector as *const () as usize) };
    plic::init_hart(arch::hart_id());
    arch::enable_supervisor_trap_interrupts();
}

#[no_mangle]
extern "C" fn kernel_trap_handler(context: &mut KernelTrapContext) {
    // 现场由 trap.S 按 KernelTrapContext 布局保存好，Rust 侧只需要读 CSR 判断原因。
    // `context` 本实验不逐字段使用，但它是现场地址确实传进 handler 的证据。
    let _ = context;

    let scause = arch::read_scause();
    let is_interrupt = scause & SCAUSE_INTERRUPT != 0;
    let code = scause & !SCAUSE_INTERRUPT;

    if is_interrupt {
        if code == S_SOFTWARE {
            // M-mode 时钟转发：清 SSIP，只有 hart 0 增加全局 ticks。
            timer::update(arch::hart_id());
        } else if code == S_EXTERNAL {
            // UART 经 PLIC 送来的外部中断：claim → 读取 → complete。
            let irq = plic::claim();
            if irq == plic::UART_IRQ {
                if uart::handle_receive_interrupt() && !UART_REPORTED.swap(true, Ordering::AcqRel) {
                    println!("lab-3 uart test: PASS byte=K");
                }
            } else if irq != 0 {
                println!("unexpected external irq {} ignored", irq);
            }
            // 每个非零 claim 都必须配对一次 complete，未知 IRQ 也不例外。
            if irq != 0 {
                plic::complete(irq);
            }
        } else {
            report_unknown_trap("interrupt", code);
            halt();
        }
        return;
    }

    // 内核态同步异常：本实验不实现内核页故障恢复。
    report_unknown_trap("exception", code);
    halt();
}

/// 打印未实现 trap 的原因码与 CSR 现场，便于定位第一个出错的地方。
fn report_unknown_trap(kind: &str, code: usize) {
    println!(
        "unhandled {} code={} scause={:#x} sepc={:#x} stval={:#x}",
        kind,
        code,
        arch::read_scause(),
        arch::read_sepc(),
        arch::read_stval()
    );
}

/// 未实现的 trap 无法安全返回：不清 `sepc`、也不加 4，直接停机。
fn halt() -> ! {
    loop {
        arch::wait_for_interrupt();
    }
}
