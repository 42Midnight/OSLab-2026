#![no_std]
#![no_main]

//! Lab 5 用户态验收程序：复制、brk、栈增长、mmap/munmap。
//!
//! 所有断言失败时都停在原地等待 QEMU 超时，避免错误地打印 PASS。

use core::{arch::asm, panic::PanicInfo};

const SYS_COPYOUT: usize = 1;
const SYS_COPYIN: usize = 2;
const SYS_COPYINSTR: usize = 3;
const SYS_BRK: usize = 4;
const SYS_MMAP: usize = 5;
const SYS_MUNMAP: usize = 6;
const PAGE_SIZE: usize = 4096;

#[inline(never)]
fn syscall(number: usize, arg0: usize, arg1: usize) -> isize {
    let mut result = arg0;
    unsafe {
        asm!("ecall", inlateout("a0") result, in("a1") arg1, in("a7") number);
    }
    result as isize
}

fn fail() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

fn check(condition: bool) {
    if !condition {
        fail();
    }
}

#[inline(never)]
fn test_copy() {
    let mut values = [0u32; 5];
    check(syscall(SYS_COPYOUT, values.as_mut_ptr() as usize, 0) == 0);
    check(values == [1, 2, 3, 4, 5]);

    check(syscall(SYS_COPYIN, values.as_ptr() as usize, 5) == 0);
    check(syscall(SYS_COPYINSTR, b"hello, world\0".as_ptr() as usize, 0) == 0);

    // 代码页是 R|X，没有 W；copyout 必须拒绝而不是改写指令。
    let code_page = _start as *const () as usize;
    check(syscall(SYS_COPYOUT, code_page, 0) == -1);
}

#[inline(never)]
fn test_cross_page_copy(top: usize) {
    // 选一个严格位于堆内部、且跨页的 20 字节缓冲区。
    let boundary = (top - PAGE_SIZE) & !(PAGE_SIZE - 1);
    let base = (boundary - 10) as *mut [u32; 5];
    let pattern = [1u32, 2, 3, 4, 5];

    for _ in 0..2 {
        check(syscall(SYS_COPYOUT, base as usize, 0) == 0);
        check(unsafe { *base } == pattern);
    }
}

#[inline(never)]
fn test_brk() {
    let heap = syscall(SYS_BRK, 0, 0);
    check(heap > 0);

    let grown = heap as usize + 9 * PAGE_SIZE;
    check(syscall(SYS_BRK, grown, 0) == grown as isize);
    check(syscall(SYS_BRK, grown, 0) == grown as isize);
    test_cross_page_copy(grown);

    let shrunk = heap as usize + 4 * PAGE_SIZE;
    check(syscall(SYS_BRK, shrunk, 0) == shrunk as isize);
    check(syscall(SYS_BRK, 0x3000_0000, 0) == -1);
}

#[inline(never)]
fn test_stack_growth() {
    let sp: usize;
    unsafe { asm!("mv {}, sp", out(reg) sp) };

    let address = (sp - 4 * PAGE_SIZE) as *mut u8;
    unsafe { address.write_volatile(0x5a) };
    check(unsafe { address.read_volatile() } == 0x5a);

    let address2 = (sp - 3 * PAGE_SIZE) as *mut u8;
    unsafe { address2.write_volatile(0xa5) };
    check(unsafe { address2.read_volatile() } == 0xa5);
}

#[inline(never)]
fn test_mmap() {
    // 先验证相邻映射会合并：只有合并成一个 region，跨两页解除才会成功。
    let left = syscall(SYS_MMAP, 0, PAGE_SIZE);
    check(left > 0);
    let right = syscall(SYS_MMAP, left as usize + PAGE_SIZE, PAGE_SIZE);
    check(right == left + PAGE_SIZE as isize);
    check(syscall(SYS_MUNMAP, left as usize, 2 * PAGE_SIZE) == 0);

    // 长度 4097 必须向上取整为两页。
    let first = syscall(SYS_MMAP, 0, PAGE_SIZE + 1);
    check(first > 0);
    check(first as usize % PAGE_SIZE == 0);
    unsafe {
        (first as *mut u8).write_volatile(0x11);
        ((first as usize + PAGE_SIZE) as *mut u8).write_volatile(0x22);
    }
    check(syscall(SYS_MUNMAP, first as usize, 2 * PAGE_SIZE) == 0);

    let base = syscall(SYS_MMAP, 0, 3 * PAGE_SIZE);
    check(base > 0);
    check(base as usize % PAGE_SIZE == 0);
    unsafe {
        (base as *mut u8).write_volatile(0x33);
        ((base as usize + 2 * PAGE_SIZE) as *mut u8).write_volatile(0x44);
    }

    // 与已有 region 重叠必须失败。
    check(syscall(SYS_MMAP, base as usize + PAGE_SIZE, PAGE_SIZE) == -1);

    // 解中间页：region 应拆成两段，首末页仍可读写。
    check(syscall(SYS_MUNMAP, base as usize + PAGE_SIZE, PAGE_SIZE) == 0);
    check(unsafe { (base as *const u8).read_volatile() } == 0x33);
    check(unsafe { ((base as usize + 2 * PAGE_SIZE) as *const u8).read_volatile() } == 0x44);

    check(syscall(SYS_MUNMAP, base as usize, PAGE_SIZE) == 0);
    check(syscall(SYS_MUNMAP, base as usize + 2 * PAGE_SIZE, PAGE_SIZE) == 0);
    // 重复解除必须失败；内核在此时运行页表深复制自检并输出最终 PASS。
    check(syscall(SYS_MUNMAP, base as usize, PAGE_SIZE) == -1);
}

#[unsafe(link_section = ".text.entry")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    test_copy();
    test_brk();
    test_stack_growth();
    test_mmap();

    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    fail()
}
