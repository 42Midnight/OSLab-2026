//! 用户内存访问：所有用户虚拟地址都必须按当前用户页表逐页翻译。
//!
//! 内核不能把用户地址直接转成裸指针：用户页表与内核页表的映射不同，而且相邻
//! 用户虚拟页可能对应不相邻的物理页。这里先整体校验范围与权限，再按页复制。

use crate::{
    mm::{PteFlags, VirtAddr, VirtPageNum, PAGE_SIZE},
    process,
};

/// 校验 `[user_addr, user_addr + len)` 的每一页都已映射、属于用户且权限足够。
fn validate_range(user_addr: usize, len: usize, write: bool) -> Result<(), &'static str> {
    if len == 0 {
        return Ok(());
    }
    let end = user_addr
        .checked_add(len)
        .ok_or("user_access: user range overflow")?;
    let current = process::current().ok_or("user_access: no current process")?;
    let page_table = current.page_table();

    let mut vpn = VirtAddr(user_addr).floor();
    let last_vpn = VirtAddr(end - 1).floor();
    loop {
        let entry = page_table
            .translate(vpn)
            .ok_or("user_access: user page is not mapped")?;
        let flags = entry.flags();
        if !flags.contains(PteFlags::U) {
            return Err("user_access: page is not user accessible");
        }
        if write {
            if !flags.contains(PteFlags::W) {
                return Err("user_access: user page is not writable");
            }
        } else if !flags.contains(PteFlags::R) {
            return Err("user_access: user page is not readable");
        }

        if vpn.0 == last_vpn.0 {
            break;
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }
    Ok(())
}

/// 从用户地址复制到内核缓冲区。跨页时按当前页剩余长度分段复制。
pub fn copy_from_user(user_addr: usize, dst: &mut [u8]) -> Result<(), &'static str> {
    validate_range(user_addr, dst.len(), false)?;
    let mut copied = 0usize;
    while copied < dst.len() {
        let address = user_addr
            .checked_add(copied)
            .ok_or("user_access: user address overflow")?;
        let vpn = VirtAddr(address).floor();
        let offset = address & (PAGE_SIZE - 1);
        let chunk = (PAGE_SIZE - offset).min(dst.len() - copied);

        let current = process::current().ok_or("user_access: no current process")?;
        let entry = current
            .page_table()
            .translate(vpn)
            .ok_or("user_access: user page is not mapped")?;
        let source = entry.ppn().addr().0 + offset;

        // Safety：source 指向本页内 [offset, offset + chunk) 的有效物理内存。
        unsafe {
            core::ptr::copy_nonoverlapping(
                source as *const u8,
                dst[copied..copied + chunk].as_mut_ptr(),
                chunk,
            );
        }
        copied += chunk;
    }
    Ok(())
}

/// 从内核缓冲区复制到用户地址。写前已完成整段 W|U 校验，失败不会产生部分写入。
pub fn copy_to_user(user_addr: usize, src: &[u8]) -> Result<(), &'static str> {
    validate_range(user_addr, src.len(), true)?;
    let mut copied = 0usize;
    while copied < src.len() {
        let address = user_addr
            .checked_add(copied)
            .ok_or("user_access: user address overflow")?;
        let vpn = VirtAddr(address).floor();
        let offset = address & (PAGE_SIZE - 1);
        let chunk = (PAGE_SIZE - offset).min(src.len() - copied);

        let current = process::current().ok_or("user_access: no current process")?;
        let entry = current
            .page_table()
            .translate(vpn)
            .ok_or("user_access: user page is not mapped")?;
        let destination = entry.ppn().addr().0 + offset;

        // Safety：destination 指向本页内 [offset, offset + chunk) 的有效物理内存。
        unsafe {
            core::ptr::copy_nonoverlapping(
                src[copied..copied + chunk].as_ptr(),
                destination as *mut u8,
                chunk,
            );
        }
        copied += chunk;
    }
    Ok(())
}

/// 从用户地址读取以 NUL 结尾的字符串，返回 NUL 之前的字节数。
pub fn copy_string_from_user(user_addr: usize, dst: &mut [u8]) -> Result<usize, &'static str> {
    if dst.is_empty() {
        return Err("user_access: string buffer is empty");
    }

    let mut copied = 0usize;
    while copied < dst.len() {
        let address = user_addr
            .checked_add(copied)
            .ok_or("user_access: string address overflow")?;
        let mut byte = [0u8; 1];
        copy_from_user(address, &mut byte)?;
        dst[copied] = byte[0];
        if byte[0] == 0 {
            return Ok(copied);
        }
        copied += 1;
    }
    Err("user_access: user string is not NUL terminated")
}
