//! 用户态陷阱：TrapContext 布局、入口初始化、分派与返回。
//!
//! 进入内核的路径是 `user_vector`（trampoline 页）→ 保存现场 → 切换内核页表与
//! 内核栈 → `user_trap_handler`；返回路径是 `trap_return` → `user_restore`
//! （同一 trampoline 页）→ 切回用户页表 → `sret`。

use core::sync::atomic::{AtomicBool, Ordering};

use crate::{arch, plic, process, syscall, timer, uart, user_vm};

const SCAUSE_INTERRUPT: usize = 1usize << (usize::BITS - 1);
const SCAUSE_USER_ECALL: usize = 8;
const SCAUSE_LOAD_PAGE_FAULT: usize = 13;
const SCAUSE_STORE_PAGE_FAULT: usize = 15;
const S_INTERRUPT_SOFTWARE: usize = 1;
const S_INTERRUPT_EXTERNAL: usize = 9;

static UART_REPORTED: AtomicBool = AtomicBool::new(false);

extern "C" {
    fn user_vector();
    fn user_restore();
}

/// 用户态 trap 现场。字段顺序与 `trap_user.S` 的 `n*8(sp)` 偏移一一对应。
#[repr(C)]
pub struct UserTrapContext {
    pub x: [usize; 32],
    pub sstatus: usize,
    pub sepc: usize,
    pub kernel_satp: usize,
    pub kernel_sp: usize,
    pub trap_handler: usize,
}

impl UserTrapContext {
    /// 构造首次进入用户态所需的现场。
    ///
    /// `SPP=0` 让 `sret` 回到 U-mode；`SPIE=1` 让 `sret` 后开启中断；`SIE=0`
    /// 保证返回流程在 trampoline 中不会被内核态中断打断。`x[2]` 是用户栈顶，
    /// 不能误写成内核栈顶。
    pub fn new(
        entry: usize,
        user_sp: usize,
        kernel_satp: usize,
        kernel_sp: usize,
        trap_handler: usize,
    ) -> Self {
        let mut sstatus = arch::read_sstatus();
        sstatus &= !(arch::SSTATUS_SPP | arch::SSTATUS_SIE);
        sstatus |= arch::SSTATUS_SPIE;

        let mut context = Self {
            x: [0; 32],
            sstatus,
            sepc: entry,
            kernel_satp,
            kernel_sp,
            trap_handler,
        };
        context.x[2] = user_sp;
        context
    }
}

/// 结构大小与字段偏移必须和 `trap_user.S` 的 `n*8(sp)` 完全一致。
pub fn layout_ok() -> bool {
    let word = core::mem::size_of::<usize>();
    core::mem::size_of::<UserTrapContext>() == 37 * word
        && core::mem::offset_of!(UserTrapContext, x) == 0
        && core::mem::offset_of!(UserTrapContext, sstatus) == 32 * word
        && core::mem::offset_of!(UserTrapContext, sepc) == 33 * word
        && core::mem::offset_of!(UserTrapContext, kernel_satp) == 34 * word
        && core::mem::offset_of!(UserTrapContext, kernel_sp) == 35 * word
        && core::mem::offset_of!(UserTrapContext, trap_handler) == 36 * word
}

/// 用户 trap 入口的初始化：检查 trampoline 布局，并预置 `sscratch`。
pub fn init() {
    let vector = user_vector as *const () as usize;
    let offset = trampoline_restore_offset();
    assert!(
        vector % crate::mm::PAGE_SIZE == 0,
        "user_vector is not page aligned"
    );
    assert!(
        offset > 0 && offset < crate::mm::PAGE_SIZE,
        "user_restore is outside the trampoline page"
    );
    assert!(layout_ok(), "UserTrapContext layout is wrong");
    assert!(
        crate::task_context::TaskContext::layout_ok(),
        "TaskContext layout is wrong"
    );
    // 首次返回用户态前 sscratch 就应指向 trap context，之后每次 user_restore 都会重设。
    unsafe { arch::set_sscratch(process::TRAP_CONTEXT) };
}

/// `user_restore` 相对 `user_vector` 的页内偏移。
pub fn trampoline_restore_offset() -> usize {
    user_restore as *const () as usize - user_vector as *const () as usize
}

/// `user_trap_handler` 的地址，写入 TrapContext 供 trampoline 跳转。
pub fn handler_address() -> usize {
    user_trap_handler as *const () as usize
}

/// 从 trampoline 返回用户态：先设 `stvec`，再跳到 trampoline 页内的恢复代码。
///
/// 不能用普通函数调用进入 `user_restore`：它切换 satp 后仍要继续取指，
/// 只有同时映射在两个页表里的 trampoline 页才能保证下一条指令有效。
pub fn trap_return() -> ! {
    let token = match process::current() {
        Some(current) => current.token(),
        None => halt(),
    };

    // 内核态从这里到 sret 之间必须关中断，否则中断会以 trampoline 为入口
    // 重入，把内核寄存器写进用户现场。
    arch::disable_interrupts();
    unsafe { arch::set_stvec(process::TRAMPOLINE) };

    let restore = process::TRAMPOLINE + trampoline_restore_offset();
    unsafe {
        core::arch::asm!(
            "fence.i",
            "jr {restore}",
            restore = in(reg) restore,
            in("a0") process::TRAP_CONTEXT,
            in("a1") token,
            options(noreturn)
        );
    }
}

/// 用户态 trap 的统一分发入口。
#[no_mangle]
pub extern "C" fn user_trap_handler(context: &mut UserTrapContext) -> ! {
    let scause = arch::read_scause();
    let is_interrupt = scause & SCAUSE_INTERRUPT != 0;
    let code = scause & !SCAUSE_INTERRUPT;

    if is_interrupt {
        match code {
            // M-mode 时钟转发来的 S-mode 软件中断。
            S_INTERRUPT_SOFTWARE => handle_timer(),
            // UART 经 PLIC 送来的外部中断。
            S_INTERRUPT_EXTERNAL => handle_external(),
            _ => unexpected("interrupt", code),
        }
    } else if code == SCAUSE_USER_ECALL {
        // 一条 ecall 只推进一次 sepc，再把返回值写回 a0。
        context.sepc += 4;
        let result = syscall::dispatch(context);
        context.x[10] = result as usize;
    } else if code == SCAUSE_LOAD_PAGE_FAULT || code == SCAUSE_STORE_PAGE_FAULT {
        // 合法的用户栈缺页：映射所需页面后不推进 sepc，让 sret 重试原指令。
        let fault_addr = arch::read_stval();
        if user_vm::grow_stack(fault_addr).is_err() {
            println!(
                "lab-5 invalid stack fault scause={:#x} sepc={:#x} stval={:#x}",
                arch::read_scause(),
                arch::read_sepc(),
                arch::read_stval()
            );
            halt();
        }
    } else {
        unexpected("exception", code);
    }

    // 返回用户态必须走 trampoline，普通 return 会继续使用内核栈。
    trap_return()
}

/// 处理用户态时钟中断：清 SSIP；Lab 5 不额外打印，保留 Lab 4 的时钟路径。
fn handle_timer() {
    timer::update(0);
}

/// 处理用户态外部中断：claim → 排空 UART FIFO → complete。
fn handle_external() {
    let irq = plic::claim();
    if irq == plic::UART_IRQ {
        if uart::handle_receive_interrupt() && !UART_REPORTED.swap(true, Ordering::AcqRel) {
            println!("lab-5 user uart: PASS");
        }
    } else if irq != 0 {
        println!("lab-5 unexpected external irq {} ignored", irq);
    }
    // 每个非零 claim 都必须配对一次 complete，未知 IRQ 也不例外。
    if irq != 0 {
        plic::complete(irq);
    }
}

/// 未知 trap 不能通过修改 sepc 跳过：打印现场后停机。
fn unexpected(kind: &str, code: usize) -> ! {
    arch::disable_interrupts();
    println!(
        "lab-5 unhandled {} code={} scause={:#x} sepc={:#x} stval={:#x}",
        kind,
        code,
        arch::read_scause(),
        arch::read_sepc(),
        arch::read_stval()
    );
    halt()
}

fn halt() -> ! {
    arch::disable_interrupts();
    loop {
        arch::wait_for_interrupt();
    }
}
