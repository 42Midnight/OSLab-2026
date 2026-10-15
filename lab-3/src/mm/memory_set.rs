//! 内核地址空间：按段建立恒等映射，并把 Sv39 satp token 发布给两个 hart。

use core::sync::atomic::{AtomicUsize, Ordering};

use super::{PageTable, PteFlags, VirtAddr, PAGE_SIZE};

const MEMORY_END: usize = 0x8800_0000;
const UART_BASE: usize = 0x1000_0000;
/// PLIC 的三个 MMIO 页：优先级数组、hart 0 的 S-context 使能位、threshold/claim。
const PLIC_BASE: usize = 0x0c00_0000;
const PLIC_ENABLE_HART0: usize = 0x0c00_2000;
const PLIC_CONTEXT_HART0: usize = 0x0c20_1000;
/// Sv39 的 satp MODE 值为 8，占 satp 最高 4 位；本实验 ASID 使用 0。
const SATP_MODE_SV39: usize = 8;
static KERNEL_TOKEN: AtomicUsize = AtomicUsize::new(0);

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss();
    fn ebss();
    fn ekernel();
}

pub fn kernel_space_init() -> Result<(), &'static str> {
    // 函数符号只用于取地址，不能调用。
    let stext = stext as *const () as usize;
    let etext = etext as *const () as usize;
    let srodata = srodata as *const () as usize;
    let erodata = erodata as *const () as usize;
    let sdata = sdata as *const () as usize;
    let edata = edata as *const () as usize;
    let sbss = sbss as *const () as usize;
    let ebss = ebss as *const () as usize;
    let ekernel = ekernel as *const () as usize;

    // 再次断言链接边界顺序与页对齐，防止链接脚本改坏后仍悄悄建立页表。
    let ordered = stext <= etext
        && etext <= srodata
        && srodata <= erodata
        && erodata <= sdata
        && sdata <= edata
        && edata <= sbss
        && sbss <= ebss
        && ebss <= ekernel
        && ekernel < MEMORY_END;
    let aligned = stext % PAGE_SIZE == 0
        && etext % PAGE_SIZE == 0
        && srodata % PAGE_SIZE == 0
        && erodata % PAGE_SIZE == 0
        && sdata % PAGE_SIZE == 0
        && edata % PAGE_SIZE == 0
        && sbss % PAGE_SIZE == 0
        && ebss % PAGE_SIZE == 0
        && ekernel % PAGE_SIZE == 0;
    if !ordered {
        return Err("kernel_space_init: link boundaries are out of order");
    }
    if !aligned {
        return Err("kernel_space_init: a link boundary is not page aligned");
    }

    let mut page_table =
        PageTable::new().ok_or("kernel_space_init: cannot allocate the root page")?;

    // 顺序与链接布局一致，区间互不重叠。
    map_identity_range(&mut page_table, stext, etext, PteFlags::R | PteFlags::X)?;
    map_identity_range(&mut page_table, srodata, erodata, PteFlags::R)?;
    map_identity_range(&mut page_table, sdata, edata, PteFlags::R | PteFlags::W)?;
    map_identity_range(&mut page_table, sbss, ebss, PteFlags::R | PteFlags::W)?;
    map_identity_range(
        &mut page_table,
        ekernel,
        MEMORY_END,
        PteFlags::R | PteFlags::W,
    )?;
    map_identity_range(
        &mut page_table,
        UART_BASE,
        UART_BASE + PAGE_SIZE,
        PteFlags::R | PteFlags::W,
    )?;
    // Lab 3：PLIC 的优先级、使能和 threshold/claim 页都要能以 R|W 恒等映射访问。
    // 搬运字节的设备寄存器不需要可执行权限，也绝不能给 U 权限。
    // 任何一页遗漏，开启分页后第一次 claim 或 complete 都会变成页故障。
    for base in [PLIC_BASE, PLIC_ENABLE_HART0, PLIC_CONTEXT_HART0] {
        map_identity_range(
            &mut page_table,
            base,
            base + PAGE_SIZE,
            PteFlags::R | PteFlags::W,
        )?;
    }

    verify_kernel_mapping(&page_table)?;

    let token = (SATP_MODE_SV39 << 60) | page_table.root_ppn().0;
    KERNEL_TOKEN.store(token, Ordering::Release);
    // 页表必须在激活分页后终身有效，不能随作用域 Drop 回空闲链表。
    core::mem::forget(page_table);
    Ok(())
}

/// 建立半开区间 `[start, end)` 的恒等映射：虚拟地址与物理地址完全相同。
fn map_identity_range(
    page_table: &mut PageTable,
    start: usize,
    end: usize,
    flags: PteFlags,
) -> Result<(), &'static str> {
    if start % PAGE_SIZE != 0 || end % PAGE_SIZE != 0 {
        return Err("map_identity_range: range is not page aligned");
    }
    if start > end {
        return Err("map_identity_range: start is greater than end");
    }

    let start_vpn = VirtAddr(start).floor().0;
    let end_vpn = VirtAddr(end).floor().0;
    for vpn in start_vpn..end_vpn {
        page_table.map(super::VirtPageNum(vpn), super::PhysPageNum(vpn), flags)?;
    }
    Ok(())
}

/// 软件权限自检：代表页权限正确，且所有普通映射都满足 W^X。
fn verify_kernel_mapping(page_table: &PageTable) -> Result<(), &'static str> {
    let stext = stext as *const () as usize;
    let srodata = srodata as *const () as usize;
    let sdata = sdata as *const () as usize;
    let sbss = sbss as *const () as usize;

    // (地址, R, W, X)
    let expected = [
        (stext, true, false, true),
        (srodata, true, false, false),
        (sdata, true, true, false),
        (sbss, true, true, false),
        (UART_BASE, true, true, false),
        // Lab 3：三页 PLIC 都必须映射到自身且没有 X / U 权限。
        (PLIC_BASE, true, true, false),
        (PLIC_ENABLE_HART0, true, true, false),
        (PLIC_CONTEXT_HART0, true, true, false),
    ];
    for &(address, read, write, execute) in expected.iter() {
        let vpn = VirtAddr(address).floor();
        let entry = page_table
            .translate(vpn)
            .ok_or("verify_kernel_mapping: a required page is not mapped")?;
        let flags = entry.flags();
        if flags.contains(PteFlags::U) {
            return Err("verify_kernel_mapping: kernel page must not be user accessible");
        }
        if flags.contains(PteFlags::R) != read
            || flags.contains(PteFlags::W) != write
            || flags.contains(PteFlags::X) != execute
        {
            return Err("verify_kernel_mapping: a segment has wrong permissions");
        }
    }

    // 遍历全部已建立映射的普通页，检查不存在同时可写与可执行的页面。
    let start_vpn = VirtAddr(stext).floor().0;
    let end_vpn = VirtAddr(MEMORY_END).floor().0;
    for vpn in start_vpn..end_vpn {
        if let Some(entry) = page_table.translate(super::VirtPageNum(vpn)) {
            let flags = entry.flags();
            if flags.contains(PteFlags::W) && flags.contains(PteFlags::X) {
                return Err("verify_kernel_mapping: a page is both writable and executable");
            }
        }
    }

    Ok(())
}

pub fn kernel_token() -> usize {
    KERNEL_TOKEN.load(Ordering::Acquire)
}
