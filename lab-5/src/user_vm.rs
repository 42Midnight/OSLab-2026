//! 用户虚拟内存：brk 堆、page fault 栈增长，以及地址空间的深复制与销毁。

use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    arch,
    mm::{self, frame_alloc, FrameTracker, PageTable, PteFlags, VirtAddr, VirtPageNum, PAGE_SIZE},
    mmap,
    process::{self, UserProcess, MAX_OWNED_FRAMES, TRAMPOLINE, USER_STACK_TOP},
};

/// 匿名映射区起点。
pub const MMAP_BEGIN: usize = 0x1000_0000;
/// 匿名映射区终点（不含）。
pub const MMAP_END: usize = 0x2000_0000;
/// 一个用户栈最多允许的页数。
pub const MAX_STACK_PAGES: usize = 16;

static STACK_REPORTED: AtomicBool = AtomicBool::new(false);

/// 撤销一段刚建立的映射，并释放对应的进程帧。
fn rollback_pages(process: &mut UserProcess, start_vpn: usize, count: usize) {
    for index in (0..count).rev() {
        let vpn = VirtPageNum(start_vpn + index);
        let ppn = process.page_table().translate(vpn).map(|entry| entry.ppn());
        let _ = process.page_table_mut().unmap(vpn);
        if let Some(ppn) = ppn {
            let _ = process.take_frame(ppn);
        }
    }
}

/// `brk`：查询或调整当前进程的字节级堆顶。
pub fn set_brk(new_top: usize) -> Result<usize, &'static str> {
    let process = process::current().ok_or("brk: no current process")?;
    let old_top = process.heap_top();
    if new_top == 0 {
        return Ok(old_top);
    }
    if new_top < process.heap_base() || new_top > MMAP_BEGIN {
        return Err("brk: new heap top is outside the heap range");
    }
    if new_top == old_top {
        return Ok(old_top);
    }

    if new_top > old_top {
        let old_page = VirtAddr(old_top).ceil().0;
        let new_page = VirtAddr(new_top).ceil().0;
        let pages = new_page - old_page;
        if pages == 0 {
            process.set_heap_top(new_top);
            return Ok(new_top);
        }
        if process
            .frame_count()
            .checked_add(pages)
            .map(|total| total > MAX_OWNED_FRAMES)
            .unwrap_or(true)
        {
            return Err("brk: not enough owned-frame slots");
        }
        for index in 0..pages {
            if process
                .page_table()
                .translate(VirtPageNum(old_page + index))
                .is_some()
            {
                return Err("brk: heap growth collides with an existing mapping");
            }
        }

        let mut mapped = 0usize;
        while mapped < pages {
            let vpn = VirtPageNum(old_page + mapped);
            let frame = match frame_alloc() {
                Some(frame) => frame,
                None => {
                    rollback_pages(process, old_page, mapped);
                    return Err("brk: out of free frames");
                }
            };
            let ppn = frame.ppn();
            if let Err(error) =
                process
                    .page_table_mut()
                    .map(vpn, ppn, PteFlags::R | PteFlags::W | PteFlags::U)
            {
                // map 失败时 frame 仍由本作用域持有，drop 后会归还。
                drop(frame);
                rollback_pages(process, old_page, mapped);
                return Err(error);
            }
            if let Err(error) = process.push_frame(frame) {
                let _ = process.page_table_mut().unmap(vpn);
                rollback_pages(process, old_page, mapped);
                return Err(error);
            }
            mapped += 1;
        }
    } else {
        let old_page = VirtAddr(old_top).ceil().0;
        let new_page = VirtAddr(new_top).ceil().0;
        let pages = old_page - new_page;

        // 先验证全部页面都由本进程持有，避免释放到一半才发现异常。
        for index in 0..pages {
            let vpn = VirtPageNum(new_page + index);
            let entry = process
                .page_table()
                .translate(vpn)
                .ok_or("brk: heap page is not mapped")?;
            if !process.owns_frame(entry.ppn()) {
                return Err("brk: heap page is not owned by this process");
            }
        }
        for index in 0..pages {
            let vpn = VirtPageNum(new_page + index);
            let ppn = process.page_table().translate(vpn).map(|entry| entry.ppn());
            process.page_table_mut().unmap(vpn)?;
            if let Some(ppn) = ppn {
                let _ = process.take_frame(ppn);
            }
        }
    }

    process.set_heap_top(new_top);
    Ok(new_top)
}

/// 处理合法用户栈缺页：从 fault 页向下补到旧栈底，并更新栈元数据。
pub fn grow_stack(fault_addr: usize) -> Result<(), &'static str> {
    let process = process::current().ok_or("stack: no current process")?;
    let old_bottom = process.stack_bottom();
    if fault_addr >= old_bottom || fault_addr < MMAP_END {
        return Err("stack: fault address is outside the growable stack range");
    }

    let fault_page = VirtAddr(fault_addr).floor();
    let old_bottom_page = VirtAddr(old_bottom).floor();
    let floor_page = VirtAddr(USER_STACK_TOP - MAX_STACK_PAGES * PAGE_SIZE).floor();
    if fault_page.0 < floor_page.0 || fault_page.0 >= old_bottom_page.0 {
        return Err("stack: fault address is below the stack limit");
    }

    let pages = old_bottom_page.0 - fault_page.0;
    if process
        .stack_pages()
        .checked_add(pages)
        .map(|total| total > MAX_STACK_PAGES)
        .unwrap_or(true)
    {
        return Err("stack: too many stack pages");
    }
    if process
        .frame_count()
        .checked_add(pages)
        .map(|total| total > MAX_OWNED_FRAMES)
        .unwrap_or(true)
    {
        return Err("stack: not enough owned-frame slots");
    }
    for index in 0..pages {
        if process
            .page_table()
            .translate(VirtPageNum(fault_page.0 + index))
            .is_some()
        {
            return Err("stack: growth collides with an existing mapping");
        }
    }

    let mut mapped = 0usize;
    while mapped < pages {
        let vpn = VirtPageNum(fault_page.0 + mapped);
        let frame = match frame_alloc() {
            Some(frame) => frame,
            None => {
                rollback_pages(process, fault_page.0, mapped);
                return Err("stack: out of free frames");
            }
        };
        let ppn = frame.ppn();
        if let Err(error) =
            process
                .page_table_mut()
                .map(vpn, ppn, PteFlags::R | PteFlags::W | PteFlags::U)
        {
            drop(frame);
            rollback_pages(process, fault_page.0, mapped);
            return Err(error);
        }
        if let Err(error) = process.push_frame(frame) {
            let _ = process.page_table_mut().unmap(vpn);
            rollback_pages(process, fault_page.0, mapped);
            return Err(error);
        }
        mapped += 1;
    }

    process.grow_stack_metadata(fault_page.addr().0, pages);
    // 当前 satp 是内核页表；这里刷新一次，返回用户态时 user_restore 还会再刷新。
    arch::flush_tlb();
    if !STACK_REPORTED.swap(true, Ordering::AcqRel) {
        println!("lab-5 stack-growth test: PASS");
    }
    Ok(())
}

/// 深复制后由测试自检临时持有的地址空间。
struct CopiedSpace {
    page_table: PageTable,
    /// 由本地址空间拥有的私有叶子帧；只在这里保存所有权，不需要逐个读取。
    #[allow(dead_code)]
    frames: [Option<FrameTracker>; MAX_OWNED_FRAMES],
    mmap_head: Option<usize>,
}

impl Drop for CopiedSpace {
    fn drop(&mut self) {
        mmap::free_list(self.mmap_head.take());
    }
}

/// 复制全部私有叶子页；trampoline 是共享只读页，只复制映射不复制物理帧。
fn clone_address_space(source: &UserProcess) -> Result<CopiedSpace, &'static str> {
    let mut page_table = PageTable::new().ok_or("copy: cannot allocate a child page table")?;
    let mut frames: [Option<FrameTracker>; MAX_OWNED_FRAMES] = core::array::from_fn(|_| None);
    let mut frame_count = 0usize;
    let mut error: Option<&'static str> = None;

    source.page_table().for_each_leaf(|vpn, entry| {
        if error.is_some() {
            return false;
        }

        let flags = entry.flags();
        if vpn.addr().0 == TRAMPOLINE {
            if let Err(message) = page_table.map(vpn, entry.ppn(), flags) {
                error = Some(message);
                return false;
            }
            return true;
        }

        if frame_count >= MAX_OWNED_FRAMES {
            error = Some("copy: too many child frames");
            return false;
        }
        let frame = match frame_alloc() {
            Some(frame) => frame,
            None => {
                error = Some("copy: out of free frames");
                return false;
            }
        };
        let ppn = frame.ppn();
        // Safety：源页与目标页都是 4 KiB 对齐的独占物理页。
        unsafe {
            core::ptr::copy_nonoverlapping(
                entry.ppn().addr().0 as *const u8,
                ppn.addr().0 as *mut u8,
                PAGE_SIZE,
            );
        }
        if let Err(message) = page_table.map(vpn, ppn, flags) {
            drop(frame);
            error = Some(message);
            return false;
        }
        frames[frame_count] = Some(frame);
        frame_count += 1;
        true
    });

    if let Some(message) = error {
        return Err(message);
    }
    let mmap_head = mmap::clone_list(source.mmap_head())?;
    Ok(CopiedSpace {
        page_table,
        frames,
        mmap_head,
    })
}

/// 父子隔离检查：同一虚拟页必须对应不同物理页，改子页不能影响父页。
fn isolation_check(parent: &UserProcess, child: &CopiedSpace) -> Result<(), &'static str> {
    let vpn = VirtAddr(USER_STACK_TOP - PAGE_SIZE).floor();
    let parent_entry = parent
        .page_table()
        .translate(vpn)
        .ok_or("copy: parent stack page is missing")?;
    let child_entry = child
        .page_table
        .translate(vpn)
        .ok_or("copy: child stack page is missing")?;
    if parent_entry.ppn() == child_entry.ppn() {
        return Err("copy: parent and child share a writable page");
    }

    let parent_addr = parent_entry.ppn().addr().0;
    let child_addr = child_entry.ppn().addr().0;
    // Safety：两页都由各自的 PageTable 映射，且都是 4 KiB 独占页。
    unsafe {
        for offset in 0..PAGE_SIZE {
            if *(parent_addr as *const u8).add(offset) != *(child_addr as *const u8).add(offset) {
                return Err("copy: child content differs from parent");
            }
        }
        let original = *(parent_addr as *const u8);
        *(child_addr as *mut u8) = original ^ 0xff;
        if *(parent_addr as *const u8) != original {
            return Err("copy: modifying the child changed the parent");
        }
    }
    Ok(())
}

/// 地址空间复制与销毁自检。返回前会恢复测试临时映射和全部帧/节点计数。
pub fn copy_address_space_test() -> bool {
    let all_frames_before = mm::free_frame_count();
    let all_nodes_before = mmap::free_node_count();

    // 保证当前进程至少有一个 mmap region，使节点深复制也被覆盖。
    let temp_start = match mmap::map(0, 2 * PAGE_SIZE) {
        Ok(start) => start,
        Err(_) => {
            println!("lab-5 page-table-copy test: FAIL");
            return false;
        }
    };
    let frames_before = mm::free_frame_count();
    let nodes_before = mmap::free_node_count();

    let cloned_ok = {
        match process::current() {
            Some(parent) => match clone_address_space(parent) {
                Ok(child) => {
                    let isolation = isolation_check(parent, &child);
                    if let Err(message) = isolation {
                        println!("lab-5 page-table-copy: {}", message);
                    }
                    let ok = isolation.is_ok();
                    drop(child);
                    ok
                }
                Err(message) => {
                    println!("lab-5 page-table-copy: {}", message);
                    false
                }
            },
            None => false,
        }
    };

    let copied_restored = cloned_ok
        && mm::free_frame_count() == frames_before
        && mmap::free_node_count() == nodes_before;
    let _ = mmap::unmap(temp_start, 2 * PAGE_SIZE);
    let all_restored =
        mm::free_frame_count() == all_frames_before && mmap::free_node_count() == all_nodes_before;
    let passed = copied_restored && all_restored;

    println!(
        "lab-5 page-table-copy test: {}",
        if passed { "PASS" } else { "FAIL" }
    );
    passed
}
