OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    . = 0x1000;
    .text : ALIGN(4K) {
        *(.text.entry)
        *(.text .text.*)
    }
    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
    }
    .data : ALIGN(4K) {
        *(.data .data.*)
    }
    .bss : ALIGN(4K) {
        *(.bss .bss.*)
        *(COMMON)
    }
    /DISCARD/ : {
        *(.eh_frame)
    }
}
