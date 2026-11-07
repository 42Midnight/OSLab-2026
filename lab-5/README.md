# Lab 5 系统调用与用户虚拟内存

本目录在 Lab 4 首个用户进程的基础上完成：

- `a7/a0..a5` 系统调用 ABI 与 `sret` 返回路径；
- `copyin/copyout/copyinstr` 的逐页 `U/R/W` 校验与跨页复制；
- `brk` 堆增长/缩小、page fault 驱动的用户栈增长；
- 256 节点 mmap region 仓库、相邻合并、`munmap` 四种区间拆分；
- 用户页表、私有物理帧和 mmap 节点的深复制与销毁；
- 双 hart QEMU 验收：`make check` 输出 `lab-5 check: PASS`。

## 构建与运行

```sh
make build          # 先构建 user/，再把用户 ELF 嵌入内核
make check          # 双 hart QEMU 自动验收
make qemu           # 手工运行，Ctrl+A 后按 X 退出
make disasm         # 生成 kernel.asm
```

用户栈固定在 `0x4000_0000`，堆位于 ELF 数据末端之后，mmap 区为
`[0x1000_0000, 0x2000_0000)`。进程内核栈和每个 hart 的启动栈均为 32 KiB。
