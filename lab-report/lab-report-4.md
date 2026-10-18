# 实验报告 Lab 4：首个用户进程

**实验环境**：WSL2（Ubuntu 24.04.3 LTS）· Rust 1.98.0 · `riscv64gc-unknown-none-elf` · QEMU 8.2.2（`-smp 2`）

## 一、实验目的

在 Lab 3 的分页与中断之上装载首个 RISC-V ELF 用户程序，建立独立用户页表，区分 TaskContext 与 TrapContext，经 trampoline 的 `sret` 进入 U-mode，并在用户态完成 ecall、时钟和 UART 中断的往返。

## 二、实验内容

在 Lab 3 工程上覆盖 starter，完成 5 个骨架文件的 9 处 `TODO`，并改动 `main.rs`、`mm/memory_set.rs`、`arch.rs`、`sync.rs`、`uart.rs`、`trap.rs`。

- **ELF 装载**：越界安全地校验 ELF 头与程序头（魔数、ELF64、小端、RISC-V、`ET_EXEC`、范围、`filesz ≤ memsz`、入口在可执行段、拒绝重叠与 W|X），按 `PT_LOAD` 逐页申请帧、按 `p_flags|U` 建映射并复制 `filesz` 字节；出错时由 `Drop` 归还页帧。
- **用户地址空间**：`from_elf` 建立独立页表，把 `user_vector` 页按 R|X 映射到 `TRAMPOLINE`，按 `p_vaddr` 装载代码，在 `heap_top=0x1010` 后留无 PTE 的 guard page，映射一页 R|W|U 栈（`sp=0x4000`），`TRAP_CONTEXT` 映射为 R|W 且无 U/X 并申请内核栈；内核页表用 `kernel_map` 映射同一 trampoline 与 context 页，`layout_check` 核对权限、guard 为空、UART/PLIC 不可见。
- **两种上下文与 trap 往返**：`TaskContext`（112 字节）以清零 current 调用 `__switch` 进入 `trap_return`；`UserTrapContext`（37 字）初始化 `sepc=0x1000`、`x[2]=0x4000`、清 `SPP`、置 `SPIE`。`user_vector` 保存全部用户寄存器后切内核页表，`user_restore` 切回用户 satp 并 `sret`，`trap_return` 跳到 `TRAMPOLINE + (user_restore - user_vector)`。
- **syscall 与设备中断**：`scause=8` 先 `sepc += 4`，`a7=1` 打印 hello 并回写 `a0=0`，第二次输出 syscall PASS；`scause=1` 清 SSIP，第 5 次输出 timer PASS；`scause=9` 经 PLIC claim、排空 UART、complete，收到 `K` 输出 uart PASS；未知 trap 打印现场后停机。

## 三、实验结果

```text
$ cd lab-4 && cargo fmt --check && make clean && make check
lab-4 elf rollback test: PASS
lab-4 first process: pid=0 entry=0x1000 segments=1 heap_top=0x1010 user_sp=0x4000 stack_pages=1 pages=2
lab-4 address-space test: PASS
proczero: hello world!
proczero: hello world!
lab-4 user syscall: PASS
Klab-4 user uart: PASS byte=K
lab-4 user timer: PASS ticks=5
lab-4 check: PASS
```

用户程序从 0x1000 进入 U-mode，两次 `ecall` 输出 hello 与 syscall PASS，第 5 次用户态时钟中断输出 timer PASS，注入 `K` 后由用户态外部中断回显并输出 uart PASS。`user_vector` 在 0x80001000、`user_restore` 在 0x80001066，二者同页；trampoline 内含 `csrw satp`、`sfence.vma` 与 `sret`。

![Lab 4 make check 输出](images/lab4-1.png)

**图 1**：`make clean && make check` 的完整输出——各条自检 PASS，末行 `lab-4 check: PASS`。

![Lab 4 用户态运行](images/lab4-2.png)

**图 2**：`make qemu` 下用户程序的运行输出——从 0x1000 进入 U-mode 后两次 hello、syscall PASS，以及第 5 次用户态时钟中断的 timer PASS。
