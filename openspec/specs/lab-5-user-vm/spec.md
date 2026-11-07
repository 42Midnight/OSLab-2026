# Lab 5 系统调用与用户虚拟内存规格

## Purpose

本实验在首个用户进程上建立安全系统调用参数传递，并实现堆、自动增长栈、离散映射以及用户地址空间复制和销毁。

## Requirements

### Requirement: 用户数据复制安全
内核 MUST 按当前用户页表逐页验证地址、范围和权限，不能直接解引用用户虚拟地址。
#### Scenario: 跨页复制
- **WHEN** 合法用户缓冲区跨越两个不连续物理页
- **THEN** copyin 和 copyout 完整复制数据并保持顺序
#### Scenario: 无效用户范围
- **WHEN** 地址溢出、页面未映射或权限不足
- **THEN** 系统调用返回错误且内核不 panic、不产生部分副作用

### Requirement: 堆和栈边界明确
brk MUST 原子地增长或缩小用户堆；合法栈缺页 MUST 按页扩展且不得越过 mmap 边界。
#### Scenario: brk 分配失败
- **WHEN** 堆增长过程中物理页不足或新边界越界
- **THEN** 新映射全部回滚且 heap top 保持原值
#### Scenario: 合法栈缺页
- **WHEN** load/store page fault 位于允许增长的用户栈范围
- **THEN** 内核映射所需页面并返回原指令重试

### Requirement: mmap 元数据与页表一致
mmap 和 munmap MUST 同步维护有序 region、PTE、物理帧和固定容量节点仓库。
#### Scenario: 相邻映射合并
- **WHEN** 新映射与前驱或后继区间相邻
- **THEN** region 合并且多余节点归还仓库
#### Scenario: 中间取消映射
- **WHEN** munmap 删除一个 region 的中间子区间
- **THEN** 原 region 拆成两个有序节点且仅目标页面被释放

### Requirement: 地址空间可以复制和销毁
系统 MUST 深复制所有私有用户页，并在失败或销毁时完整回收已拥有资源。
#### Scenario: 父子隔离
- **WHEN** 复制地址空间后修改子进程数据页
- **THEN** 父页内容不变且父子 PPN 不同
#### Scenario: 销毁副本
- **WHEN** 销毁刚复制的地址空间
- **THEN** 空闲帧数和 mmap 节点数恢复到复制前状态

### Requirement: 自动验收可重复
项目 MUST 提供格式、反汇编、OpenSpec 和双 hart QEMU 检查。
#### Scenario: 干净验收
- **WHEN** 执行 `make clean && make check`
- **THEN** 所有 Lab 5 测试通过并输出 `lab-5 check: PASS`
