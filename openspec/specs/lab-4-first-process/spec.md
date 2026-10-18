# Lab 4 首个用户进程规格

## Purpose

本实验把一个最小 RISC-V ELF 程序装入独立用户地址空间，使其通过 ecall、时钟和 UART 中断与内核交互，并可由 QEMU 重复验证。

## Requirements

### Requirement: ELF 装载安全
内核 MUST 校验 ELF 头、program header、段大小和入口，并按 LOAD flags 建立最小用户权限。
#### Scenario: 合法首个程序
- **WHEN** 输入合法 RISC-V ELF
- **THEN** 代码被复制到用户页，BSS 清零，entry 位于可执行段
#### Scenario: 畸形 ELF
- **WHEN** 魔数、范围、大小或入口不合法
- **THEN** 装载返回错误且已申请帧全部回收

### Requirement: 用户地址空间隔离
用户页表 MUST 包含代码、用户栈、guard page、trap context 和 trampoline；用户不得访问内核设备。
#### Scenario: 栈越界
- **WHEN** 用户栈向 guard page 下方增长
- **THEN** 产生 page fault，guard page 保持未映射

### Requirement: 用户态陷阱往返
系统 MUST 保存完整用户寄存器、sepc 和内核返回信息，并以 sret 返回 U-mode。
#### Scenario: 用户 ecall
- **WHEN** 用户执行 ecall
- **THEN** 内核处理 syscall、将 sepc 增加 4、恢复现场并回到下一条用户指令

### Requirement: 首个 syscall 和设备中断
系统 MUST 响应 SYS_HELLOWORLD，并在用户态继续处理时钟和 UART 中断。
#### Scenario: hello world
- **WHEN** 首个用户程序执行两次 SYS_HELLOWORLD
- **THEN** 输出两次 proczero: hello world!
#### Scenario: 用户态设备中断
- **WHEN** 用户态收到时钟或 UART 中断
- **THEN** handler 完成处理后返回原用户执行流

### Requirement: 可重复验收
项目 MUST 提供格式、反汇编、OpenSpec 和双 hart QEMU 验收。
#### Scenario: 干净验收
- **WHEN** 执行 make clean && make check
- **THEN** 输出 lab-4 check: PASS
