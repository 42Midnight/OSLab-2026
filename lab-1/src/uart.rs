//! QEMU virt 机器上 16550 UART 的轮询式发送驱动。

const UART0: usize = 0x1000_0000;
const THR: usize = 0;
const IER: usize = 1;
const FCR: usize = 2;
const LCR: usize = 3;
const LSR: usize = 5;

const LCR_BAUD_LATCH: u8 = 1 << 7;
const LCR_EIGHT_BITS: u8 = 3;
const FCR_FIFO_ENABLE: u8 = 1;
const FCR_FIFO_CLEAR: u8 = 3 << 1;
const LSR_TX_IDLE: u8 = 1 << 5;

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

/// 发送一个字节：先等待 LSR 的发送保持寄存器空闲位，再写入 THR。
pub fn putchar(character: u8) {
    unsafe {
        while read_register(LSR) & LSR_TX_IDLE == 0 {
            core::hint::spin_loop();
        }
        write_register(THR, character);
    }
}
