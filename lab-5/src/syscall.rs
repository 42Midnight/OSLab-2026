//! 系统调用 ABI：从 TrapContext 读取 a0..a7，并把返回值写回 a0。
//!
//! 所有参数都来自不可信的用户态。这里只做协议层分派，地址校验、页表修改和
//! 资源回收分别由 `user_access`、`user_vm` 和 `mmap` 完成。

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::{mmap, trap_user::UserTrapContext, user_access, user_vm};

pub const SYS_COPYOUT: usize = 1;
pub const SYS_COPYIN: usize = 2;
pub const SYS_COPYINSTR: usize = 3;
pub const SYS_BRK: usize = 4;
pub const SYS_MMAP: usize = 5;
pub const SYS_MUNMAP: usize = 6;

static COPYOUT_OK: AtomicBool = AtomicBool::new(false);
static COPYIN_OK: AtomicBool = AtomicBool::new(false);
static COPY_REPORTED: AtomicBool = AtomicBool::new(false);
static BRK_SUCCESSES: AtomicUsize = AtomicUsize::new(0);
static BRK_FAIL_SEEN: AtomicBool = AtomicBool::new(false);
static BRK_REPORTED: AtomicBool = AtomicBool::new(false);
static MMAP_INVALID_SEEN: AtomicBool = AtomicBool::new(false);
static MUNMAP_INVALID_SEEN: AtomicBool = AtomicBool::new(false);
static MUNMAP_SUCCESSES: AtomicUsize = AtomicUsize::new(0);
static FINISHED: AtomicBool = AtomicBool::new(false);

/// 从 `a7` 取系统调用号，从 `a0..a5` 取参数，返回值写入 `a0`。
pub fn dispatch(context: &mut UserTrapContext) -> isize {
    let number = context.x[17];
    let a0 = context.x[10];
    let a1 = context.x[11];

    let result = match number {
        SYS_COPYOUT => sys_copyout(a0),
        SYS_COPYIN => sys_copyin(a0, a1),
        SYS_COPYINSTR => sys_copyinstr(a0),
        SYS_BRK => sys_brk(a0),
        SYS_MMAP => sys_mmap(a0, a1),
        SYS_MUNMAP => sys_munmap(a0, a1),
        // 未知编号只返回错误，不能 panic，也不能把内核地址暴露给用户。
        _ => -1,
    };
    context.x[10] = result as usize;
    result
}

fn sys_copyout(user_addr: usize) -> isize {
    let values = [1u32, 2, 3, 4, 5];
    let mut bytes = [0u8; 20];
    for (index, value) in values.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    match user_access::copy_to_user(user_addr, &bytes) {
        Ok(()) => {
            COPYOUT_OK.store(true, Ordering::Release);
            0
        }
        Err(_) => -1,
    }
}

fn sys_copyin(user_addr: usize, count: usize) -> isize {
    if count == 0 || count > 16 {
        return -1;
    }
    let bytes = match count.checked_mul(4) {
        Some(bytes) => bytes,
        None => return -1,
    };
    let mut buffer = [0u8; 64];
    if user_access::copy_from_user(user_addr, &mut buffer[..bytes]).is_err() {
        return -1;
    }

    let mut values = [0u32; 16];
    for index in 0..count {
        let base = index * 4;
        values[index] = u32::from_le_bytes([
            buffer[base],
            buffer[base + 1],
            buffer[base + 2],
            buffer[base + 3],
        ]);
    }
    if count != 5 || values[..5] != [1, 2, 3, 4, 5] {
        return -1;
    }

    println!(
        "lab-5 copyin: {} {} {} {} {}",
        values[0], values[1], values[2], values[3], values[4]
    );
    COPYIN_OK.store(true, Ordering::Release);
    0
}

fn sys_copyinstr(user_addr: usize) -> isize {
    let mut buffer = [0u8; 64];
    let length = match user_access::copy_string_from_user(user_addr, &mut buffer) {
        Ok(length) => length,
        Err(_) => return -1,
    };

    match core::str::from_utf8(&buffer[..length]) {
        Ok(text) => println!("lab-5 copyinstr: {}", text),
        Err(_) => println!("lab-5 copyinstr: <non-utf8 length={}>", length),
    }

    if COPYOUT_OK.load(Ordering::Acquire)
        && COPYIN_OK.load(Ordering::Acquire)
        && !COPY_REPORTED.swap(true, Ordering::AcqRel)
    {
        println!("lab-5 user-copy test: PASS");
    }
    0
}

fn sys_brk(new_top: usize) -> isize {
    match user_vm::set_brk(new_top) {
        Ok(top) => {
            if new_top != 0 {
                BRK_SUCCESSES.fetch_add(1, Ordering::AcqRel);
            }
            maybe_report_brk();
            top as isize
        }
        Err(_) => {
            BRK_FAIL_SEEN.store(true, Ordering::Release);
            maybe_report_brk();
            -1
        }
    }
}

fn maybe_report_brk() {
    if BRK_FAIL_SEEN.load(Ordering::Acquire)
        && BRK_SUCCESSES.load(Ordering::Acquire) >= 3
        && !BRK_REPORTED.swap(true, Ordering::AcqRel)
    {
        println!("lab-5 brk test: PASS");
    }
}

fn sys_mmap(begin: usize, len: usize) -> isize {
    match mmap::map(begin, len) {
        Ok(address) => address as isize,
        Err(_) => {
            MMAP_INVALID_SEEN.store(true, Ordering::Release);
            -1
        }
    }
}

fn sys_munmap(begin: usize, len: usize) -> isize {
    match mmap::unmap(begin, len) {
        Ok(()) => {
            MUNMAP_SUCCESSES.fetch_add(1, Ordering::AcqRel);
            maybe_finish();
            0
        }
        Err(_) => {
            MUNMAP_INVALID_SEEN.store(true, Ordering::Release);
            maybe_finish();
            -1
        }
    }
}

/// 用户态 mmap 测试全部完成后，才运行页表深复制并输出最终 PASS。
fn maybe_finish() {
    if MUNMAP_SUCCESSES.load(Ordering::Acquire) >= 4
        && MMAP_INVALID_SEEN.load(Ordering::Acquire)
        && MUNMAP_INVALID_SEEN.load(Ordering::Acquire)
        && !FINISHED.swap(true, Ordering::AcqRel)
    {
        println!("lab-5 mmap test: PASS");
        if user_vm::copy_address_space_test() {
            println!("lab-5 syscall test: PASS");
        }
    }
}
