# ECNU OSLab 2026 实验仓库

华东师范大学操作系统课程 2026 年实验的个人完成版。

- 实验环境：**WSL2 (Ubuntu 24.04 LTS)** 替代课程默认的 VMware 虚拟机
- 技术栈：Rust + rCore-Tutorial-v3 + RISC-V 64 + QEMU + OpenSpec
- 完成范围：Lab 0、Lab 1、Lab 2、Lab 3

## 实验报告在哪里

[OSLab-2026-Lab0-1-2-实验报告.md](OSLab-2026-Lab0-1-2-实验报告.md)

仓库根目录下的这一个文件就是全部实验报告，包含 Lab 0–Lab 3 的环境配置、WSL2 替代 VMware 的说明、各实验的验收证据与排障记录。

| 实验 | 报告章节 |
|---|---|
| Lab 0 课程导论与 rCore 编译 | 四、Lab 0：课程导论与 rCore 编译 |
| Lab 1 机器启动与并发安全输出 | 五、Lab 1：机器启动与并发安全输出 |
| Lab 2 物理内存与内核虚拟内存 | 六、Lab 2：物理内存与内核虚拟内存 |
| Lab 3 中断与异常初步 | 七、Lab 3：中断与异常初步 |

报告还包含「分支与提交」「如何复现」「排障记录」「交付物清单」四节，可直接据此复现全部实验。

## 目录说明

| 路径 | 内容 |
|---|---|
| `OSLab-2026-Lab0-1-2-实验报告.md` | 实验报告（Lab 0–Lab 3） |
| `lab-1/` | Lab 1 内核工程 |
| `lab-2/` | Lab 2 内核工程 |
| `lab-3/` | Lab 3 内核工程 |
| `openspec/specs/` | 各实验的 OpenSpec 行为规格 |
| `os/`、`user/`、`easy-fs/` 等 | 课程固定的 rCore 基线代码，供 Lab 0 使用 |

## 环境与验收

环境为 WSL2 (Ubuntu 24.04 LTS)，Rust stable + `riscv64gc-unknown-none-elf` 目标、QEMU 8.2.2、`rust-objdump`、`gdb-multiarch`。

各实验的验收命令与实测结果见报告「一、总体结论」「九、如何复现」两节；课程材料的验收要求以 Gitee 课程仓库 `ecnu-oslab-2026-task` 为准。
