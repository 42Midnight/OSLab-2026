    # 用户态 trap 的入口与返回位于同一页，并同时映射在用户页表与内核页表的
    # TRAMPOLINE 处：切换 satp 之后仍能继续取指。不要改动入口符号名。
    .section .text.trampoline
    .align 12
    .globl user_vector
    .globl user_restore

    .altmacro
    .macro SAVE_GP n
        sd x\n, \n*8(sp)
    .endm
    .macro LOAD_GP n
        ld x\n, \n*8(sp)
    .endm

user_vector:
    # 进入 S-mode 时 sp 仍是用户栈指针，sscratch 保存 trap context 的地址。
    csrrw sp, sscratch, sp
    # 现在 sp 指向 TRAP_CONTEXT，sscratch 保存用户栈指针。
    # x2 稍后从 sscratch 取出再保存，其余寄存器按固定偏移写入现场。
    sd x1, 1*8(sp)
    sd x3, 3*8(sp)
    sd x4, 4*8(sp)
    .set n, 5
    .rept 27
        SAVE_GP %n
        .set n, n+1
    .endr

    # 保存 sstatus、sepc；t0 已写入现场，可以自由使用。
    csrr t0, sstatus
    sd t0, 32*8(sp)
    csrr t0, sepc
    sd t0, 33*8(sp)
    csrr t0, sscratch
    sd t0, 2*8(sp)

    # 把 trap context 的虚拟地址交给 Rust handler。TRAP_CONTEXT 在用户页表和
    # 内核页表中映射到同一个物理页，所以切换 satp 之后这个指针仍然有效。
    mv a0, sp

    ld t0, 34*8(sp)     # kernel_satp
    ld t1, 35*8(sp)     # kernel_sp
    ld t2, 36*8(sp)     # trap_handler
    csrw satp, t0
    sfence.vma
    mv sp, t1
    jr t2

user_restore:
    # a0 = TRAP_CONTEXT，a1 = 用户页表 token。
    csrw satp, a1
    sfence.vma
    csrw sscratch, a0
    mv sp, a0

    # 恢复 sstatus/sepc：SPP=0 让 sret 回到 U-mode，SPIE=1 恢复用户态中断。
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
    csrw sstatus, t0
    csrw sepc, t1

    # 恢复除 x2 外的全部通用寄存器，最后用现场里的用户栈指针复位 sp。
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    ld x4, 4*8(sp)
    .set n, 5
    .rept 27
        LOAD_GP %n
        .set n, n+1
    .endr
    ld sp, 2*8(sp)
    sret
