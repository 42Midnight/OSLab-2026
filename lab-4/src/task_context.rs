//! 内核上下文切换用的 TaskContext。
//!
//! 它只保存内核中被调用者保存的寄存器，与保存完整用户现场的
//! [`crate::trap_user::UserTrapContext`] 是两种不同的结构，不能合并。

use crate::trap_user::trap_return;

/// `__switch(current, next)` 保存与恢复的寄存器集合。
///
/// 偏移必须与 `switch.S` 一致：`ra` 在 0、`sp` 在 8、`s0`～`s11` 从 16 开始。
#[repr(C)]
pub struct TaskContext {
    pub ra: usize,
    pub sp: usize,
    pub s: [usize; 12],
}

impl TaskContext {
    /// 一个清零的内核上下文，作为 `__switch` 的 `current` 使用。
    pub const fn zero() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 12],
        }
    }

    /// 让首次 `__switch` 恢复到 `trap_return`，并使用该进程的内核栈。
    pub fn goto_trap_return(&mut self, kernel_sp: usize) {
        self.ra = trap_return as *const () as usize;
        self.sp = kernel_sp;
    }

    /// 核对汇编需要的结构大小与字段偏移。
    pub fn layout_ok() -> bool {
        core::mem::size_of::<Self>() == 14 * core::mem::size_of::<usize>()
            && core::mem::offset_of!(Self, ra) == 0
            && core::mem::offset_of!(Self, sp) == 8
            && core::mem::offset_of!(Self, s) == 16
    }
}
