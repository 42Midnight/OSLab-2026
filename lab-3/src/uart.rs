//! QEMU virt 机器上 16550 UART 的轮询式发送与中断式接收驱动。

use core::sync::atomic::{AtomicBool, Ordering};

use crate::console;

const UART0: usize = 0x1000_0000;
const RHR: usize = 0;
const THR: usize = 0;
const IER: usize = 1;
const FCR: usize = 2;
const LCR: usize = 3;
const LSR: usize = 5;

const LCR_BAUD_LATCH: u8 = 1 << 7;
const LCR_EIGHT_BITS: u8 = 3;
const FCR_FIFO_ENABLE: u8 = 1;
const FCR_FIFO_CLEAR: u8 = 3 << 1;
/// LSR bit 0：接收保持寄存器已就绪。
const LSR_RX_READY: u8 = 1;
const LSR_TX_IDLE: u8 = 1 << 5;
/// IER bit 0：接收数据可用中断。
const IER_RX_ENABLE: u8 = 1;

const BACKSPACE: u8 = 8;
const DELETE: u8 = 127;
/// 验收字符：输入 `K` 时必须且只输出一次测试标志。
const TEST_BYTE: u8 = b'K';

/// 测试标志只允许打印一次，即使 `K` 被输入多次。
static TEST_REPORTED: AtomicBool = AtomicBool::new(false);

/// 写设备寄存器必须使用 `write_volatile`，否则编译器可能合并或删除该次访问。
#[inline]
unsafe fn write_register(offset: usize, value: u8) {
    (UART0 as *mut u8).add(offset).write_volatile(value);
}

/// 读设备寄存器必须使用 `read_volatile`，每次读取都要真正访问 MMIO。
#[inline]
unsafe fn read_register(offset: usize) -> u8 {
    (UART0 as *const u8).add(offset).read_volatile()
}

/// 初始化 UART：关闭中断、设置 8 位数据格式并复位 FIFO。
pub fn init() {
    unsafe {
        write_register(IER, 0);
        write_register(LCR, LCR_BAUD_LATCH);
        write_register(0, 0x03);
        write_register(1, 0);
        write_register(LCR, LCR_EIGHT_BITS);
        write_register(FCR, FCR_FIFO_ENABLE | FCR_FIFO_CLEAR);
    }
}

/// 打开接收中断。
///
/// 必须在 UART 初始化、DLAB 清零之后调用；本实验只有 hart 0 开启 UART 接收，
/// 这样每个输入字符只会让 hart 0 进入一次外部中断。
pub fn enable_receive_interrupt() {
    unsafe { write_register(IER, IER_RX_ENABLE) };
}

/// 发送一个字节：先等待 LSR 的发送保持寄存器空闲位，再写入 THR。
pub fn putchar(character: u8) {
    unsafe {
        while read_register(LSR) & LSR_TX_IDLE == 0 {
            core::hint::spin_loop();
        }
        write_register(THR, character);
    }
}

/// 读取一个已到达的字符；`None` 表示 RX FIFO 已空。
#[inline]
fn receive_byte() -> Option<u8> {
    if unsafe { read_register(LSR) } & LSR_RX_READY != 0 {
        Some(unsafe { read_register(RHR) })
    } else {
        None
    }
}

/// 回显一个字符。
///
/// 普通字符原样输出；Backspace 依次输出“退格、空格、退格”擦掉一个终端字符；
/// 回车与换行都规范化为换行。
fn echo(byte: u8) {
    match byte {
        BACKSPACE | DELETE => {
            putchar(BACKSPACE);
            putchar(b' ');
            putchar(BACKSPACE);
        }
        b'\r' | b'\n' => putchar(b'\n'),
        byte => putchar(byte),
    }
}

/// 处理一次 UART 接收中断。
///
/// 一次外部中断可能对应多个已经排队的字节，所以必须循环读到 FIFO 为空，
/// 而不是只读一个字符。整段回显在串口锁内完成，避免和其他 hart 的 `println!`
/// 交错；锁不可重入，因此测试标志的打印放在临界区之外。
pub fn handle_receive_interrupt() {
    let mut saw_test_byte = false;
    console::with_lock(|| {
        while let Some(byte) = receive_byte() {
            echo(byte);
            if byte == TEST_BYTE {
                saw_test_byte = true;
            }
        }
    });

    if saw_test_byte && !TEST_REPORTED.swap(true, Ordering::AcqRel) {
        println!("lab-3 uart test: PASS byte=K");
    }
}
