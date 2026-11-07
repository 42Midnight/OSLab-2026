mod address;
mod frame_allocator;
mod memory_set;
mod page_table;

pub use address::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
pub use frame_allocator::{
    frame_alloc, frame_allocator_init, free_frame_count, is_managed, managed_end_ppn,
    managed_start_ppn, FrameTracker,
};
pub use memory_set::{kernel_map, kernel_space_init, kernel_token};
// `PageTableEntry` 只在 mm 模块内部使用，按讲义提示不重新导出，避免 unused import 警告。
pub use page_table::{PageTable, PteFlags};

fn pass_text(passed: bool) -> &'static str {
    if passed {
        "PASS"
    } else {
        "FAIL"
    }
}

/// 地址类型与 Sv39 索引自检。
fn address_test() -> bool {
    let va = VirtAddr(0x12345);
    va.floor().0 == 0x12
        && va.ceil().0 == 0x13
        && va.page_offset() == 0x345
        && VirtPageNum(0x12345).indexes() == [0x0, 0x91, 0x145]
}

/// 页帧生命周期自检：分配、清零、回收、计数恢复。
fn frame_test() -> bool {
    let before = free_frame_count();
    if before < 3 {
        return false;
    }

    let a = match frame_alloc() {
        Some(frame) => frame,
        None => return false,
    };
    let b = match frame_alloc() {
        Some(frame) => frame,
        None => return false,
    };

    if a.ppn() == b.ppn() {
        return false;
    }
    if !is_managed(a.ppn()) || !is_managed(b.ppn()) {
        return false;
    }
    let start = managed_start_ppn().0;
    let end = managed_end_ppn().0;
    if !(start <= a.ppn().0 && a.ppn().0 < end) || !(start <= b.ppn().0 && b.ppn().0 < end) {
        return false;
    }

    // 用 volatile 写把 A 页首字节改成非零，验证后续分配会清零。
    unsafe { (a.ppn().addr().0 as *mut u8).write_volatile(0x5a) };
    let a_ppn = a.ppn();
    drop(a);

    let c = match frame_alloc() {
        Some(frame) => frame,
        None => return false,
    };

    // 头插回收使 C 取得刚归还的 A 页，且分配路径已把整页清零。
    let recycled = c.ppn() == a_ppn;
    let cleared = unsafe { (c.ppn().addr().0 as *const u8).read_volatile() } == 0;

    drop(b);
    drop(c);
    let restored = free_frame_count() == before;

    recycled && cleared && restored
}

/// 三级页表 map / translate / unmap 与错误路径自检。
fn page_table_test() -> bool {
    let before = free_frame_count();

    let result = (|| -> bool {
        let mut page_table = match PageTable::new() {
            Some(page_table) => page_table,
            None => return false,
        };
        let target = match frame_alloc() {
            Some(frame) => frame,
            None => return false,
        };
        let other = match frame_alloc() {
            Some(frame) => frame,
            None => return false,
        };

        let vpn = VirtPageNum(0x12345);

        // 初始状态必须没有映射。
        if page_table.translate(vpn).is_some() {
            return false;
        }

        // 建立映射后查询应返回同一个 PPN。
        if page_table
            .map(vpn, target.ppn(), PteFlags::R | PteFlags::W)
            .is_err()
        {
            return false;
        }
        let saved = match page_table.translate(vpn) {
            Some(entry) => entry,
            None => return false,
        };
        if saved.ppn() != target.ppn() {
            return false;
        }

        // 重复映射必须失败，且原 PTE 保持不变。
        if page_table
            .map(vpn, other.ppn(), PteFlags::R | PteFlags::W)
            .is_ok()
        {
            return false;
        }
        match page_table.translate(vpn) {
            Some(entry) if entry.0 == saved.0 => {}
            _ => return false,
        }

        // 非法权限：W-only 与不含 R/W/X。
        if page_table
            .map(VirtPageNum(0x12346), other.ppn(), PteFlags::W)
            .is_ok()
        {
            return false;
        }
        if page_table
            .map(VirtPageNum(0x12346), other.ppn(), PteFlags::empty())
            .is_ok()
        {
            return false;
        }

        // 取消映射后查询为空，再次取消必须失败。
        if page_table.unmap(vpn).is_err() {
            return false;
        }
        if page_table.translate(vpn).is_some() {
            return false;
        }
        if page_table.unmap(vpn).is_ok() {
            return false;
        }

        true
    })();

    // 测试对象离开作用域后，页表页、目标页和临时页都应回到空闲链表。
    result && free_frame_count() == before
}

/// 依次运行四组自检，并发布内核页表 token。
pub fn run_tests() -> bool {
    let address_ok = address_test();
    println!("lab-2 address test: {}", pass_text(address_ok));

    let frame_ok = frame_test();
    println!("lab-2 frame test: {}", pass_text(frame_ok));

    let page_table_ok = page_table_test();
    println!("lab-2 page-table test: {}", pass_text(page_table_ok));

    let prerequisites_ok = address_ok && frame_ok && page_table_ok;
    let kernel_ok = prerequisites_ok && kernel_space_init().is_ok();
    println!("lab-2 kernel-space test: {}", pass_text(kernel_ok));

    prerequisites_ok && kernel_ok
}
