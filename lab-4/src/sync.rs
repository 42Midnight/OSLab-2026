//! 关闭 S-mode 中断的嵌套计数、自旋锁与 RAII 守卫。
//!
//! 每个 hart 拥有独立的中断嵌套计数，并记录最外层临界区之前中断是否开启。
//! 这一点对 Lab 4 很关键：用户 trap handler 运行在 `sstatus.SIE = 0` 下，
//! 而它内部会调用 `println!` 获取这把锁；如果退出临界区时无条件开中断，
//! 内核态就会以 trampoline 为入口重入，破坏正在处理的用户现场。

use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use crate::arch;

/// 两个 hart 的中断嵌套计数，下标即 hart ID。
static INTERRUPT_DEPTH: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// 每个 hart 进入最外层临界区之前，S-mode 中断是否开启。
static INTERRUPTS_WERE_ON: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// 进入临界区：先记录并关闭当前 hart 的 S-mode 中断，再增加嵌套计数。
fn push_off() {
    let was_on = arch::interrupts_enabled();
    arch::disable_interrupts();
    let hart = arch::hart_id();
    let depth = INTERRUPT_DEPTH[hart].load(Ordering::Relaxed);
    if depth == 0 {
        INTERRUPTS_WERE_ON[hart].store(was_on, Ordering::Relaxed);
    }
    INTERRUPT_DEPTH[hart].store(depth + 1, Ordering::Relaxed);
}

/// 离开临界区：只有嵌套计数回到 0 才恢复进入前的中断状态。
fn pop_off() -> bool {
    let hart = arch::hart_id();
    let depth = INTERRUPT_DEPTH[hart].load(Ordering::Relaxed);
    if depth == 0 {
        // 深度为 0 时调用 pop_off 属于内核错误：不得凭空开启中断。
        return false;
    }
    let remaining = depth - 1;
    INTERRUPT_DEPTH[hart].store(remaining, Ordering::Relaxed);
    if remaining == 0 && INTERRUPTS_WERE_ON[hart].load(Ordering::Relaxed) {
        arch::enable_interrupts();
    }
    true
}

/// 当前 hart 的中断嵌套深度。
#[allow(dead_code)]
pub fn interrupt_depth() -> usize {
    INTERRUPT_DEPTH[arch::hart_id()].load(Ordering::Relaxed)
}

/// 关闭中断的自旋锁。
pub struct SpinLock {
    locked: AtomicBool,
    owner: AtomicUsize,
}

// Safety：锁的临界区由原子 `locked` 与关闭中断共同保护。
unsafe impl Sync for SpinLock {}

impl SpinLock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            owner: AtomicUsize::new(usize::MAX),
        }
    }

    /// 获取锁；获取前关闭当前 hart 的中断，成功后记录 owner hart ID。
    pub fn lock(&self) -> SpinLockGuard<'_> {
        push_off();
        while self.locked.swap(true, Ordering::Acquire) {
            spin_loop();
        }
        self.owner.store(arch::hart_id(), Ordering::Relaxed);
        SpinLockGuard { lock: self }
    }

    /// 释放锁：以 Release 顺序发布解锁，使其他 hart 能看到临界区内的写入。
    fn unlock(&self) {
        self.owner.store(usize::MAX, Ordering::Relaxed);
        self.locked.store(false, Ordering::Release);
    }

    /// 当前持有锁的 hart ID；未被持有时返回 `usize::MAX`。
    #[allow(dead_code)]
    pub fn owner(&self) -> usize {
        self.owner.load(Ordering::Relaxed)
    }
}

/// RAII 守卫：离开作用域时自动释放锁并逐层恢复中断状态。
pub struct SpinLockGuard<'a> {
    lock: &'a SpinLock,
}

impl Drop for SpinLockGuard<'_> {
    fn drop(&mut self) {
        self.lock.unlock();
        pop_off();
    }
}

/// 自检两层 push_off / pop_off：
///
/// 1. 进入前中断开启；
/// 2. 第一层进入后中断关闭；
/// 3. 第二层进入后仍然关闭；
/// 4. 内层退出不得提前开启中断；
/// 5. 最外层退出后恢复为进入前的状态。
///
/// Lab 1 的 `rust_main` 调用它并输出 `interrupt nesting self-test: PASS`；
/// Lab 2 起不再调用，保留实现以便回归验证锁的中断语义。
#[allow(dead_code)]
pub fn interrupt_nesting_selftest() -> bool {
    let initial = arch::interrupts_enabled();
    let depth_before = interrupt_depth();

    push_off();
    let off_once = !arch::interrupts_enabled() && interrupt_depth() == depth_before + 1;

    push_off();
    let off_twice = !arch::interrupts_enabled() && interrupt_depth() == depth_before + 2;

    // 内层退出：深度减一，但中断必须仍然关闭。
    let inner_ok = pop_off() && !arch::interrupts_enabled();
    let depth_after_inner = interrupt_depth() == depth_before + 1;

    // 最外层退出：恢复进入前的状态。
    let outer_ok = pop_off();
    let restored = arch::interrupts_enabled() == initial;
    let depth_after_outer = interrupt_depth() == depth_before;

    off_once
        && off_twice
        && inner_ok
        && depth_after_inner
        && outer_ok
        && restored
        && depth_after_outer
}
