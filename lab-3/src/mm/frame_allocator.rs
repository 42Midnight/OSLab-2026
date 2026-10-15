//! 物理页帧分配器：以内核镜像末尾到 RAM 末尾的完整物理页构成空闲链表。
//!
//! 每个空闲页的第一个机器字保存下一个空闲页的物理地址；`HEAD` 为链表头，
//! 值为 0 表示链表为空。

use core::{
    hint::spin_loop,
    ptr::write_bytes,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::{PhysAddr, PhysPageNum, PAGE_SIZE};

/// 保护空闲链表的自旋锁。
static FRAME_LOCK: AtomicBool = AtomicBool::new(false);
/// 空闲链表头，保存物理地址；0 表示链表为空。
static HEAD: AtomicUsize = AtomicUsize::new(0);
/// 分配器管理的起始物理页号。
static START_PPN: AtomicUsize = AtomicUsize::new(0);
/// 分配器管理的结束物理页号（不含）。
static END_PPN: AtomicUsize = AtomicUsize::new(0);

fn lock() {
    while FRAME_LOCK.swap(true, Ordering::Acquire) {
        spin_loop();
    }
}

fn unlock() {
    FRAME_LOCK.store(false, Ordering::Release);
}

pub struct FrameTracker {
    ppn: PhysPageNum,
}

impl FrameTracker {
    pub fn ppn(&self) -> PhysPageNum {
        self.ppn
    }
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        let ppn = self.ppn.0;
        let start = START_PPN.load(Ordering::Relaxed);
        let end = END_PPN.load(Ordering::Relaxed);
        assert!(
            start <= ppn && ppn < end,
            "FrameTracker::drop: ppn {:#x} outside managed range [{:#x}, {:#x})",
            ppn,
            start,
            end
        );

        let page_addr = ppn << 12;
        assert!(
            page_addr & (PAGE_SIZE - 1) == 0,
            "FrameTracker::drop: page address {:#x} is not page aligned",
            page_addr
        );

        lock();

        // 遍历空闲链表，检测重复释放。
        let mut cursor = HEAD.load(Ordering::Relaxed);
        let mut steps = 0usize;
        while cursor != 0 {
            if cursor == page_addr {
                unlock();
                panic!("FrameTracker::drop: double free of page {:#x}", page_addr);
            }
            if cursor < (start << 12) || cursor >= (end << 12) || cursor & (PAGE_SIZE - 1) != 0 {
                unlock();
                panic!("FrameTracker::drop: corrupted free list at {:#x}", cursor);
            }
            steps += 1;
            if steps > end - start {
                unlock();
                panic!("FrameTracker::drop: free list cycle detected");
            }
            cursor = unsafe { (cursor as *const usize).read_volatile() };
        }

        // 头插回收：回收时不必清零，下次分配会统一清零。
        unsafe { (page_addr as *mut usize).write_volatile(HEAD.load(Ordering::Relaxed)) };
        HEAD.store(page_addr, Ordering::Relaxed);
        unlock();
    }
}

/// # Safety
/// `kernel_end` 之后到 `memory_end` 的地址必须是可写、页对齐后可分配的 RAM。
pub unsafe fn frame_allocator_init(kernel_end: PhysAddr, memory_end: PhysAddr) {
    let start_ppn = kernel_end.ceil().0;
    let end_ppn = memory_end.floor().0;
    assert!(
        start_ppn < end_ppn,
        "frame_allocator_init: no allocatable pages in [{:#x}, {:#x})",
        kernel_end.0,
        memory_end.0
    );

    START_PPN.store(start_ppn, Ordering::Relaxed);
    END_PPN.store(end_ppn, Ordering::Relaxed);
    HEAD.store(0, Ordering::Relaxed);

    let total = end_ppn - start_ppn;
    let mut initialized = 0usize;
    let mut ppn = start_ppn;
    while ppn < end_ppn {
        let page_addr = ppn << 12;
        // 把当前链表头写入页首，再把该页设为新的链表头。
        unsafe { (page_addr as *mut usize).write_volatile(HEAD.load(Ordering::Relaxed)) };
        HEAD.store(page_addr, Ordering::Relaxed);
        initialized += 1;
        ppn += 1;
    }
    assert_eq!(
        initialized, total,
        "frame_allocator_init: initialized page count mismatch"
    );
}

pub fn frame_alloc() -> Option<FrameTracker> {
    lock();

    let head = HEAD.load(Ordering::Relaxed);
    if head == 0 {
        unlock();
        return None;
    }

    // 必须先取出 next，再清零，否则链表后继地址会被抹掉。
    let next = unsafe { (head as *const usize).read_volatile() };
    HEAD.store(next, Ordering::Relaxed);

    // 该页已脱离空闲链表，清零无需继续持有全局锁。
    unlock();

    unsafe { write_bytes(head as *mut u8, 0, PAGE_SIZE) };

    Some(FrameTracker {
        ppn: PhysPageNum(head >> 12),
    })
}

/// 统计当前空闲页数，仅供自检使用。
pub fn free_frame_count() -> usize {
    lock();

    let start = START_PPN.load(Ordering::Relaxed);
    let end = END_PPN.load(Ordering::Relaxed);
    let total = end - start;
    let min_addr = start << 12;
    let max_addr = end << 12;

    let mut count = 0usize;
    let mut cursor = HEAD.load(Ordering::Relaxed);
    while cursor != 0 {
        if cursor < min_addr
            || cursor >= max_addr
            || cursor & (PAGE_SIZE - 1) != 0
            || count >= total
        {
            unlock();
            panic!("free_frame_count: corrupted free list at {:#x}", cursor);
        }
        count += 1;
        cursor = unsafe { (cursor as *const usize).read_volatile() };
    }

    unlock();
    count
}

/// 判断某个物理页号是否由本分配器管理。
pub fn is_managed(ppn: PhysPageNum) -> bool {
    let start = START_PPN.load(Ordering::Relaxed);
    let end = END_PPN.load(Ordering::Relaxed);
    start <= ppn.0 && ppn.0 < end
}

/// 被管理区间的起始物理页号，仅供自检使用。
pub fn managed_start_ppn() -> PhysPageNum {
    PhysPageNum(START_PPN.load(Ordering::Relaxed))
}

/// 被管理区间的结束物理页号（不含），仅供自检使用。
pub fn managed_end_ppn() -> PhysPageNum {
    PhysPageNum(END_PPN.load(Ordering::Relaxed))
}
