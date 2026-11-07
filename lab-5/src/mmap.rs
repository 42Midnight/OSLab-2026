//! mmap region 固定节点仓库，以及 mmap/munmap 的区间与页表维护。
//!
//! 节点仓库由自旋锁保护，供两个 hart 做并发自检；真正的 mmap 调用只由运行
//! 用户进程的 hart 0 发起，region 链表本身是单进程私有的。

use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use crate::{
    mm::{frame_alloc, PteFlags, VirtPageNum, PAGE_SIZE},
    process::{self, UserProcess, MAX_OWNED_FRAMES},
    sync::SpinLock,
    user_vm::{MMAP_BEGIN, MMAP_END},
};

pub const N_MMAP: usize = 256;

#[derive(Copy, Clone)]
pub struct MmapRegion {
    pub start: usize,
    pub pages: usize,
    pub next: Option<usize>,
}

impl MmapRegion {
    const EMPTY: Self = Self {
        start: 0,
        pages: 0,
        next: None,
    };

    fn end(self) -> usize {
        self.start + self.pages * PAGE_SIZE
    }
}

static NODES_LOCK: SpinLock = SpinLock::new();
static mut NODES: [MmapRegion; N_MMAP] = [MmapRegion::EMPTY; N_MMAP];
static mut FREE_HEAD: usize = usize::MAX;
static mut FREE_NODES: usize = 0;
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 节点仓库自检使用的跨 hart 归属计数。
static OWNERS: [AtomicUsize; N_MMAP] = [const { AtomicUsize::new(0) }; N_MMAP];
static BARRIERS: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
static POOL_OK: AtomicBool = AtomicBool::new(false);

#[inline]
unsafe fn read_node(index: usize) -> MmapRegion {
    // Safety：调用者保证 index < N_MMAP，且节点仓库已初始化。
    unsafe { *(core::ptr::addr_of!(NODES) as *const MmapRegion).add(index) }
}

#[inline]
unsafe fn write_node(index: usize, value: MmapRegion) {
    // Safety：调用者保证 index < N_MMAP。
    unsafe {
        *(core::ptr::addr_of_mut!(NODES) as *mut MmapRegion).add(index) = value;
    }
}

pub fn init() {
    if INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    let _guard = NODES_LOCK.lock();
    if INITIALIZED.load(Ordering::Relaxed) {
        return;
    }

    unsafe {
        for index in 0..N_MMAP {
            write_node(
                index,
                MmapRegion {
                    start: 0,
                    pages: 0,
                    next: if index + 1 < N_MMAP {
                        Some(index + 1)
                    } else {
                        None
                    },
                },
            );
        }
        FREE_HEAD = 0;
        FREE_NODES = N_MMAP;
    }
    INITIALIZED.store(true, Ordering::Release);
}

fn alloc_node() -> Option<usize> {
    let _guard = NODES_LOCK.lock();
    if !INITIALIZED.load(Ordering::Relaxed) {
        return None;
    }

    unsafe {
        let head = FREE_HEAD;
        if head == usize::MAX || head >= N_MMAP {
            return None;
        }
        let node = read_node(head);
        FREE_HEAD = node.next.unwrap_or(usize::MAX);
        FREE_NODES -= 1;
        write_node(head, MmapRegion::EMPTY);
        Some(head)
    }
}

fn free_node(index: usize) -> Result<(), &'static str> {
    if index >= N_MMAP {
        return Err("mmap: node index out of range");
    }
    let _guard = NODES_LOCK.lock();
    if !INITIALIZED.load(Ordering::Relaxed) {
        return Err("mmap: node pool is not initialized");
    }

    unsafe {
        // 扫描空闲链表，检测重复释放与损坏的 next。
        let mut cursor = FREE_HEAD;
        let mut steps = 0usize;
        while cursor != usize::MAX {
            if cursor >= N_MMAP || steps > N_MMAP {
                return Err("mmap: corrupted free list");
            }
            if cursor == index {
                return Err("mmap: double free of a region node");
            }
            cursor = read_node(cursor).next.unwrap_or(usize::MAX);
            steps += 1;
        }

        write_node(
            index,
            MmapRegion {
                start: 0,
                pages: 0,
                next: if FREE_HEAD == usize::MAX {
                    None
                } else {
                    Some(FREE_HEAD)
                },
            },
        );
        FREE_HEAD = index;
        FREE_NODES += 1;
    }
    Ok(())
}

pub fn free_list(head: Option<usize>) {
    let mut current = head;
    let mut steps = 0usize;
    while let Some(index) = current {
        if index >= N_MMAP || steps > N_MMAP {
            return;
        }
        // Safety：index 来自区域链表，仓库已初始化。
        let next = unsafe { read_node(index) }.next;
        let _ = free_node(index);
        current = next;
        steps += 1;
    }
}

pub fn free_node_count() -> usize {
    let _guard = NODES_LOCK.lock();
    if !INITIALIZED.load(Ordering::Relaxed) {
        return 0;
    }
    unsafe { FREE_NODES }
}

fn barrier(index: usize) {
    BARRIERS[index].fetch_add(1, Ordering::AcqRel);
    while BARRIERS[index].load(Ordering::Acquire) < 2 {
        spin_loop();
    }
}

/// 两个 hart 并发申请一半节点，检查归属与最终空闲数。只由 hart 0 输出 PASS。
pub fn pool_test(hart_id: usize) -> bool {
    const HALF: usize = N_MMAP / 2;
    let mut allocated = [usize::MAX; HALF];
    let mut count = 0usize;
    while count < HALF {
        match alloc_node() {
            Some(index) => {
                allocated[count] = index;
                count += 1;
            }
            None => break,
        }
    }

    for &index in allocated[..count].iter() {
        OWNERS[index].fetch_add(1, Ordering::AcqRel);
    }
    barrier(0);

    if hart_id == 0 {
        let mut owners_ok = true;
        for owner in OWNERS.iter() {
            if owner.load(Ordering::Acquire) != 1 {
                owners_ok = false;
            }
        }
        for &index in allocated[..count].iter() {
            let _ = free_node(index);
        }
        barrier(1);

        let count_ok = free_node_count() == N_MMAP;
        let ok = count == HALF && owners_ok && count_ok;
        POOL_OK.store(ok, Ordering::Release);
        if ok {
            println!("lab-5 mmap-pool test: PASS");
        }
    } else {
        for &index in allocated[..count].iter() {
            let _ = free_node(index);
        }
        barrier(1);
    }

    barrier(2);
    POOL_OK.load(Ordering::Acquire) && count == HALF
}

fn check_overlap(process: &UserProcess, start: usize, end: usize) -> Result<(), &'static str> {
    let mut current = process.mmap_head();
    let mut steps = 0usize;
    while let Some(index) = current {
        if index >= N_MMAP || steps > N_MMAP {
            return Err("mmap: corrupted region list");
        }
        // Safety：index 来自区域链表。
        let node = unsafe { read_node(index) };
        if start < node.end() && node.start < end {
            return Err("mmap: requested range overlaps an existing region");
        }
        current = node.next;
        steps += 1;
    }
    Ok(())
}

/// 从 `MMAP_BEGIN` 开始扫描有序链表，返回第一个足够大的空洞起点。
fn first_fit(process: &UserProcess, pages: usize) -> Result<usize, &'static str> {
    let length = pages
        .checked_mul(PAGE_SIZE)
        .ok_or("mmap: requested length overflow")?;
    let mut cursor = MMAP_BEGIN;
    let mut current = process.mmap_head();
    let mut steps = 0usize;

    while let Some(index) = current {
        if index >= N_MMAP || steps > N_MMAP {
            return Err("mmap: corrupted region list");
        }
        // Safety：index 来自区域链表。
        let node = unsafe { read_node(index) };
        if node.start >= cursor && node.start - cursor >= length {
            return Ok(cursor);
        }
        if node.end() > cursor {
            cursor = node.end();
        }
        current = node.next;
        steps += 1;
    }

    if cursor <= MMAP_END && MMAP_END - cursor >= length {
        Ok(cursor)
    } else {
        Err("mmap: no free virtual range")
    }
}

fn rollback_mapping(process: &mut UserProcess, start_vpn: usize, count: usize) {
    for index in (0..count).rev() {
        let vpn = VirtPageNum(start_vpn + index);
        let ppn = process.page_table().translate(vpn).map(|entry| entry.ppn());
        let _ = process.page_table_mut().unmap(vpn);
        if let Some(ppn) = ppn {
            let _ = process.take_frame(ppn);
        }
    }
}

fn insert_region(process: &mut UserProcess, new_index: usize, start: usize, pages: usize) -> usize {
    let mut previous: Option<usize> = None;
    let mut current = process.mmap_head();
    while let Some(index) = current {
        // Safety：index 来自区域链表。
        let node = unsafe { read_node(index) };
        if start < node.start {
            break;
        }
        previous = Some(index);
        current = node.next;
    }

    // Safety：new_index 由 alloc_node 返回，尚未插入链表。
    unsafe {
        write_node(
            new_index,
            MmapRegion {
                start,
                pages,
                next: current,
            },
        );
    }

    if let Some(previous_index) = previous {
        // Safety：previous_index 来自区域链表。
        let previous_node = unsafe { read_node(previous_index) };
        if previous_node.end() == start {
            let mut merged_pages = previous_node.pages + pages;
            let mut next = current;
            if let Some(next_index) = current {
                // Safety：next_index 来自区域链表。
                let next_node = unsafe { read_node(next_index) };
                if start + pages * PAGE_SIZE == next_node.start {
                    merged_pages += next_node.pages;
                    next = next_node.next;
                    let _ = free_node(next_index);
                }
            }
            unsafe {
                write_node(
                    previous_index,
                    MmapRegion {
                        start: previous_node.start,
                        pages: merged_pages,
                        next,
                    },
                );
            }
            let _ = free_node(new_index);
            return previous_node.start;
        }
    }

    if let Some(previous_index) = previous {
        // Safety：previous_index 来自区域链表。
        let mut previous_node = unsafe { read_node(previous_index) };
        previous_node.next = Some(new_index);
        unsafe { write_node(previous_index, previous_node) };
    } else {
        process.set_mmap_head(Some(new_index));
    }

    if let Some(next_index) = current {
        // Safety：next_index 来自区域链表。
        let next_node = unsafe { read_node(next_index) };
        if start + pages * PAGE_SIZE == next_node.start {
            unsafe {
                write_node(
                    new_index,
                    MmapRegion {
                        start,
                        pages: pages + next_node.pages,
                        next: next_node.next,
                    },
                );
            }
            let _ = free_node(next_index);
        }
    }
    start
}

/// 匿名 `U|R|W` 映射：先验证区间，再分配节点和页面，最后有序插入并合并。
pub fn map(begin: usize, len: usize) -> Result<usize, &'static str> {
    if len == 0 {
        return Err("mmap: length must be positive");
    }
    let pages = len
        .checked_add(PAGE_SIZE - 1)
        .ok_or("mmap: requested length overflow")?
        >> 12;
    if pages == 0 {
        return Err("mmap: length rounds down to zero pages");
    }
    let length = pages
        .checked_mul(PAGE_SIZE)
        .ok_or("mmap: page count overflow")?;

    let process = process::current().ok_or("mmap: no current process")?;
    let start = if begin == 0 {
        first_fit(process, pages)?
    } else {
        if begin % PAGE_SIZE != 0 {
            return Err("mmap: explicit address is not page aligned");
        }
        if begin < MMAP_BEGIN {
            return Err("mmap: explicit address is below the mmap area");
        }
        let end = begin
            .checked_add(length)
            .ok_or("mmap: explicit range overflow")?;
        if end > MMAP_END {
            return Err("mmap: explicit range exceeds the mmap area");
        }
        check_overlap(process, begin, end)?;
        begin
    };
    let end = start
        .checked_add(length)
        .ok_or("mmap: selected range overflow")?;
    if start < MMAP_BEGIN || end > MMAP_END || start % PAGE_SIZE != 0 {
        return Err("mmap: selected range is invalid");
    }
    if check_overlap(process, start, end).is_err() {
        return Err("mmap: selected range overlaps an existing region");
    }
    if process
        .frame_count()
        .checked_add(pages)
        .map(|total| total > MAX_OWNED_FRAMES)
        .unwrap_or(true)
    {
        return Err("mmap: not enough owned-frame slots");
    }
    for index in 0..pages {
        let vpn = VirtPageNum((start >> 12) + index);
        if process.page_table().translate(vpn).is_some() {
            return Err("mmap: page table already contains the selected page");
        }
    }

    let node = alloc_node().ok_or("mmap: region table is full")?;
    let start_vpn = start >> 12;
    let mut mapped = 0usize;
    while mapped < pages {
        let vpn = VirtPageNum(start_vpn + mapped);
        let frame = match frame_alloc() {
            Some(frame) => frame,
            None => {
                rollback_mapping(process, start_vpn, mapped);
                let _ = free_node(node);
                return Err("mmap: out of free frames");
            }
        };
        let ppn = frame.ppn();
        if let Err(error) =
            process
                .page_table_mut()
                .map(vpn, ppn, PteFlags::R | PteFlags::W | PteFlags::U)
        {
            drop(frame);
            rollback_mapping(process, start_vpn, mapped);
            let _ = free_node(node);
            return Err(error);
        }
        if let Err(error) = process.push_frame(frame) {
            let _ = process.page_table_mut().unmap(vpn);
            rollback_mapping(process, start_vpn, mapped);
            let _ = free_node(node);
            return Err(error);
        }
        mapped += 1;
    }

    insert_region(process, node, start, pages);
    Ok(start)
}

/// 取消映射：验证范围完整落在某个 region 内，并区分前缀、后缀、中间和完整删除。
pub fn unmap(begin: usize, len: usize) -> Result<(), &'static str> {
    if len == 0 {
        return Err("munmap: length must be positive");
    }
    if begin % PAGE_SIZE != 0 || begin < MMAP_BEGIN {
        return Err("munmap: address is not a valid mmap address");
    }
    let pages = len
        .checked_add(PAGE_SIZE - 1)
        .ok_or("munmap: requested length overflow")?
        >> 12;
    let length = pages
        .checked_mul(PAGE_SIZE)
        .ok_or("munmap: page count overflow")?;
    let end = begin
        .checked_add(length)
        .ok_or("munmap: requested range overflow")?;
    if end > MMAP_END {
        return Err("munmap: requested range exceeds the mmap area");
    }

    let process = process::current().ok_or("munmap: no current process")?;
    let mut previous: Option<usize> = None;
    let mut current = process.mmap_head();
    let mut found: Option<(Option<usize>, usize, MmapRegion)> = None;
    let mut steps = 0usize;
    while let Some(index) = current {
        if index >= N_MMAP || steps > N_MMAP {
            return Err("munmap: corrupted region list");
        }
        // Safety：index 来自区域链表。
        let node = unsafe { read_node(index) };
        if begin >= node.start && end <= node.end() {
            found = Some((previous, index, node));
            break;
        }
        previous = Some(index);
        current = node.next;
        steps += 1;
    }
    let (previous, index, node) = found.ok_or("munmap: range is not fully mapped")?;

    // 取消任何 PTE 之前先验证全部页面和帧所有权。
    for offset in 0..pages {
        let vpn = VirtPageNum((begin >> 12) + offset);
        let entry = process
            .page_table()
            .translate(vpn)
            .ok_or("munmap: page is not mapped")?;
        let flags = entry.flags();
        if !flags.contains(PteFlags::U) || !flags.contains(PteFlags::W) {
            return Err("munmap: page does not have U|W permissions");
        }
        if !process.owns_frame(entry.ppn()) {
            return Err("munmap: page is not owned by this process");
        }
    }

    let left_pages = (begin - node.start) / PAGE_SIZE;
    let right_pages = node.pages - left_pages - pages;
    let split = if left_pages > 0 && right_pages > 0 {
        Some(alloc_node().ok_or("munmap: region table is full for split")?)
    } else {
        None
    };

    for offset in 0..pages {
        let vpn = VirtPageNum((begin >> 12) + offset);
        let ppn = process.page_table().translate(vpn).map(|entry| entry.ppn());
        process.page_table_mut().unmap(vpn)?;
        if let Some(ppn) = ppn {
            let _ = process.take_frame(ppn);
        }
    }

    if left_pages == 0 && right_pages == 0 {
        if let Some(previous_index) = previous {
            // Safety：previous_index 来自区域链表。
            let mut previous_node = unsafe { read_node(previous_index) };
            previous_node.next = node.next;
            unsafe { write_node(previous_index, previous_node) };
        } else {
            process.set_mmap_head(node.next);
        }
        let _ = free_node(index);
    } else if left_pages == 0 {
        unsafe {
            write_node(
                index,
                MmapRegion {
                    start: end,
                    pages: right_pages,
                    next: node.next,
                },
            );
        }
    } else if right_pages == 0 {
        unsafe {
            write_node(
                index,
                MmapRegion {
                    start: node.start,
                    pages: left_pages,
                    next: node.next,
                },
            );
        }
    } else {
        let split_index = split.expect("split node was allocated");
        // Safety：split_index 来自 alloc_node。
        unsafe {
            write_node(
                split_index,
                MmapRegion {
                    start: end,
                    pages: right_pages,
                    next: node.next,
                },
            );
            write_node(
                index,
                MmapRegion {
                    start: node.start,
                    pages: left_pages,
                    next: Some(split_index),
                },
            );
        }
    }
    Ok(())
}

/// 复制一条完整的 region 链表，返回新链表头；失败时释放所有新节点。
pub fn clone_list(head: Option<usize>) -> Result<Option<usize>, &'static str> {
    let mut new_head = None;
    let mut tail: Option<usize> = None;
    let mut current = head;
    let mut steps = 0usize;

    while let Some(index) = current {
        if index >= N_MMAP || steps > N_MMAP {
            free_list(new_head);
            return Err("mmap: corrupted region list");
        }
        // Safety：index 来自区域链表。
        let node = unsafe { read_node(index) };
        let new_index = match alloc_node() {
            Some(index) => index,
            None => {
                free_list(new_head);
                return Err("mmap: region table is full while copying");
            }
        };
        unsafe {
            write_node(
                new_index,
                MmapRegion {
                    start: node.start,
                    pages: node.pages,
                    next: None,
                },
            );
        }
        if let Some(tail_index) = tail {
            // Safety：tail_index 来自本函数刚分配的节点。
            let mut tail_node = unsafe { read_node(tail_index) };
            tail_node.next = Some(new_index);
            unsafe { write_node(tail_index, tail_node) };
        } else {
            new_head = Some(new_index);
        }
        tail = Some(new_index);
        current = node.next;
        steps += 1;
    }
    Ok(new_head)
}
