# ECNU OSLab 2026 实验报告：Lab 0 / Lab 1 / Lab 2 / Lab 3

> 环境：Windows 宿主机 + **WSL2 (Ubuntu 24.04 LTS)** —— 按课程要求以 WSL2 替代 VMware 虚拟机
> 技术栈：Rust 1.98.0 + rCore-Tutorial-v3 + RISC-V 64 + QEMU 8.2.2 + OpenSpec 1.11.0
> 完成日期：实验环境与 Lab 0 / 1 / 2 / 3 均已在 WSL2 中实测通过

---

## 一、总体结论

| 实验 | 验收命令 | 结果 |
|---|---|---|
| Lab 0 | `lab-0/scripts/check-env.sh ~/Work/rCore-Tutorial-v3` | **PASS**（30 项 PASS / 0 项 FAIL） |
| Lab 0 | `cd os && make run` → `Rust user shell` → `hello_world` | **PASS**（`Hello world from user mode program!`） |
| Lab 1 | `cd lab-1 && make clean && make check` | **PASS**（`lab-1 check: PASS`，0 warning） |
| Lab 2 | `cd lab-2 && make clean && make check` | **PASS**（`lab-2 check: PASS`，0 warning） |
| Lab 3 | `cd lab-3 && make clean && make check` | **PASS**（`lab-3 check: PASS`，连续 5 次复跑均通过） |
| 全部 | `openspec validate --all --strict --no-interactive` | **PASS**（4 份规格全通过） |
| 环境 | 干净 shell 下 14 个工具可达性（**不 source 任何文件**） | **PASS**（缺失命令数 0） |

工作目录：`~/Work/rCore-Tutorial-v3`
分支：`oslab-2026-base`（只读基线）、`lab-1`、`lab-2`、`lab-3`

**最终验收方式**：在**交互式登录 shell**（`bash -l -i`，等于在新终端里手工敲命令）中运行**内部零 source** 的复验脚本，所有工具与各实验验收全部通过。详见「三·补」节与「九、如何复现」。

> 说明：本文档记录了两处曾经出问题并已修复的**环境**缺陷 ——
> ① 网络受限导致的 github 依赖（第三节）；② 登录 shell 的 PATH 缺失导致 `cargo: command not found`（第三·补节）。
> 两处都有“现象 → 根因 → 修复 → 复验证据”的完整记录。
> 另外还记录了一处 Lab 3 期间的**实现**缺陷（UART 回显与 `println!` 输出交错，见 7.5 节与第十节排障表）。

---

## 二、WSL2 替代 VMware 的说明

课程《实验环境配置》第一～三节是 VMware + Ubuntu ISO 的装机流程。本实验不做这三节，改为直接使用 WSL2 中已有的 Ubuntu 24.04，其余章节（第四～八节）命令逐条在 WSL2 里执行。

对应关系：

| 课程原文 | 本实验做法 |
|---|---|
| 第一节 安装 VMware Workstation Pro | 跳过（不需要 hypervisor 软件） |
| 第二节 下载 Ubuntu 24.04 ISO | 跳过（`wsl -l -v` 已存在 Ubuntu-24.04，`VERSION 2`） |
| 第三节 在 VMware 中创建虚拟机 | 跳过（WSL2 发行版即虚拟机环境） |
| 4.1 检查系统、磁盘、网络 | 执行，`uname -m` = `x86_64`，`/` 可用 955 GiB（要求 ≥ 20 GiB） |
| 4.2 安装基础工具 | 执行，`apt` 装齐 build-essential/git/qemu-system-misc/ripgrep 等 |
| 4.3 配置 Git 身份与换行符 | 执行，`core.autocrlf=input` |
| **4.4 安装 VMware 客户机工具** | **跳过**：WSL2 自带 9p 文件系统与 `wslpath` 集成，不需要 `open-vm-tools`；该节原文也说明“不参与内核编译，可跳过但需记录原因” |
| 4.5 单独验收 QEMU | 执行，QEMU 8.2.2 ≥ 7.0 |
| 第五～八节 | 全部执行并验收 |

WSL2 环境事实（证据）：

```text
$ wsl -l -v                → Ubuntu-24.04   Stopped   2
$ uname -a                 → Linux DESKTOP-7ITPUC2 6.18.40.1-microsoft-standard-WSL2 ... x86_64 GNU/Linux
$ uname -m                 → x86_64
$ lsb_release -ds          → Ubuntu 24.04.5 LTS
$ df -h "$HOME"            → /dev/sdd 1007G 1.4G 955G 1% /
$ ps -p 1 -o comm=         → systemd
$ cat /etc/wsl.conf        → [boot] systemd=true ; [user] default=midnight
```

一层重要区分：`uname -m` 的 `x86_64` 是**宿主（Ubuntu）架构**，只用来编译；内核产物面向的是 **`riscv64gc-unknown-none-elf`**（RISC-V 64 位裸机），由 QEMU 模拟运行。

---

## 三、网络受限与镜像方案（本次环境的关键处理）

本机 WSL2 直连 `github.com` 被网络限制（TCP 443 直接拒绝），而课程材料多处依赖 GitHub：

```text
github.com          DNS-OK  TCP443-FAIL      ← 直连不可达
gitee.com           DNS-OK  TCP443-OK
static.rust-lang.org DNS-OK TCP443-OK
sh.rustup.rs        DNS-OK  TCP443-FAIL      ← rustup 官方安装脚本不可达
raw.githubusercontent.com      不可达
```

采取的替代路径（**不改变任何课程要求的版本与 commit**）：

| 环节 | 官方做法 | 本实验做法 | 结果一致性 |
|---|---|---|---|
| rustup 安装 | `sh.rustup.rs` | 从 `mirrors.ustc.edu.cn/rust-static` 取 `rustup-init` | rustup 1.29.1，工具链 1.98.0 |
| Rust 工具链 | `static.rust-lang.org` | `RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static` | rustc/cargo 1.98.0 |
| crates.io 依赖 | crates.io | `~/.cargo/config.toml` 指向 `sparse+https://rsproxy.cn/index/` | 包版本不变 |
| rCore 源码 | `github.com/rcore-os/rCore-Tutorial-v3` | `ghfast.top` 代理浅克隆 | **commit 精确等于** `c91bd3752b53ff48555aef4e3c7b8d5ddc8ee6e1` |
| Cargo git 依赖<br>（`virtio-drivers`、`lose-net-stack`） | github.com | `git config --global url."https://gh-proxy.com/https://github.com/".insteadOf "https://github.com/"` | 两个依赖均能解析到固定 rev |
| nvm 安装脚本 | `raw.githubusercontent.com` | Gitee 镜像 `gitee.com/mirrors/nvm/raw/v0.40.3/install.sh` | nvm 0.40.3 |
| Node.js 分发 | `nodejs.org` | `NVM_NODEJS_ORG_MIRROR=https://mirrors.tuna.tsinghua.edu.cn/nodejs-release/` | Node v22.16.0（≥ 20.19） |
| npm 包 | registry.npmjs.org | `registry.npmmirror.com` | OpenSpec 1.11.0 |

选用 `ghfast.top` 克隆 rCore 的原因：该镜像的仓库 HEAD 恰好就是课程指定 commit，因此一次浅克隆即可得到精确版本；`ghproxy.net` 在本网络多次出现 `curl 18 / 56` 传输中断，`gitclone.com` 返回 502，Gitee 上的 rCore 镜像历史里**没有**该 commit（已实测确认）。

---

## 三·补、Shell 环境与 PATH 修正（关键修复）

这一节记录一个**最初被漏掉、后经复现并修复**的环境问题。它不属于课程讲义条目，但会直接导致“验收脚本全 PASS、学生手工执行却报 `cargo: command not found`”。

### 问题现象

在新开的 WSL 终端里进入 `lab-2` 执行 `cargo fmt --check`，得到：

```text
Command 'cargo' not found, but can be installed with:
sudo snap install rustup
sudo apt  install cargo
```

### 根因

WSL 里 `wsl` / `wsl -d Ubuntu-24.04` 无参数启动的，是 bash 的**交互式登录 shell**：

```text
交互式登录 shell  → 读 /etc/profile → ~/.profile（以及 /etc/profile.d/*.sh）
                  → 不读 ~/.bashrc
交互式非登录 shell → 读 ~/.bashrc
非交互 shell       → 两个都不读
```

而两套工具链的 PATH 恰好都没写进登录 shell 会读的文件：

| 工具 | 原来的写法 | 登录 shell 是否生效 |
|---|---|---|
| Rust / cargo | rustup-init 用了 `--no-modify-path`，**任何点文件里都没有 PATH 行** | ✗ |
| Node / npm / openspec | nvm 安装器只把加载语句写进了 `~/.bashrc` | ✗（登录 shell 不读 `.bashrc`） |

实测证据（修复前）：

```text
$ bash -l  -c 'command -v cargo'   → NOT_FOUND
$ bash -l  -c 'command -v node'    → NOT_FOUND
$ bash -i  -c 'command -v node'    → /home/midnight/.nvm/.../node     ← 只有交互式非登录才有
$ grep cargo ~/.profile ~/.bashrc  → 两处都没有
$ grep nvm.sh ~/.profile           → 没有
```

### 修复

把 rustup 官方 PATH 片段与 nvm 加载逻辑，同时写入 `~/.bashrc` 与 `~/.profile`，且全部写成幂等（重复执行不会重复追加）：

```bash
# ~/.profile 追加
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi

export NVM_DIR="$HOME/.nvm"
if [ -s "$NVM_DIR/nvm.sh" ]; then
  . "$NVM_DIR/nvm.sh"
  nvm use default --silent >/dev/null 2>&1 || true
fi
```

`~/.bashrc` 中同样保留 `cargo/env` 的加载，以覆盖“交互式非登录 shell”这一类。

修复后（`bash -l -i`，即用户终端类型）：

```text
cargo    -> /home/midnight/.cargo/bin/cargo
rustc    -> /home/midnight/.cargo/bin/rustc
rustup   -> /home/midnight/.cargo/bin/rustup
node     -> /home/midnight/.nvm/versions/node/v22.16.0/bin/node
npm      -> /home/midnight/.nvm/versions/node/v22.16.0/bin/npm
openspec -> /home/midnight/.nvm/versions/node/v22.16.0/bin/openspec
缺失命令数: 0
```

### 验收方式也一并纠正

原验收脚本内部执行了 `source "$HOME/.cargo/env"`，**用脚本自己补上了 PATH，从而掩盖了这个缺陷**。现在改为：

- 验收在 `bash -l -i`（交互式登录 shell，等同用户终端）中运行；
- 验收脚本**内部不再 source 任何 env 文件、不修改 PATH**；
- 脚本开头先自证 PATH 来源，避免“自我兜底”式的假通过。

对应脚本：`default-workspace/oslab-scripts/75-strict-verify.sh`。

### 仍需注意的例外

非交互、非登录的调用方式（例如 `wsl -e bash -c '...'`）**按 bash 设计本来就不读任何点文件**，这类场景需要显式加载：

```bash
source "$HOME/.cargo/env"
export NVM_DIR="$HOME/.nvm"; . "$NVM_DIR/nvm.sh"; nvm use default
```

课程讲义要求的 `make build` / `make run` / `make check` 都在交互式终端里执行，因此修复后无需任何额外操作。

---


## 四、Lab 0：课程导论与 rCore 编译

### 4.1 任务 1 — 工具链验收

```text
Ubuntu 24.04.5 LTS
rustc 1.98.0 (88d9e12ae 2026-08-18)          ← 课程指定 1.98.0
cargo 1.98.0 (797e8a9bc 2026-08-05)
rustup override: 1.98.0-x86_64-unknown-linux-gnu (directory override for '.../rCore-Tutorial-v3')
riscv64gc-unknown-none-elf                    ← 已安装
llvm-tools / rust-src / rustfmt / clippy      ← 已安装
rust-objcopy / rust-objdump / rust-nm         ← cargo-binutils 0.4.0
QEMU emulator version 8.2.2 (Debian 1:8.2.2+ds-0ubuntu1.18)   ← ≥ 7.0
node v22.16.0 / npm 10.9.2 / openspec 1.11.0
```

### 4.2 任务 2 — 固定源码基线

```text
branch: oslab-2026-base
commit: c91bd3752b53ff48555aef4e3c7b8d5ddc8ee6e1     ← 与课程指定一致
git status --short: 空                                ← 基线未被改动
RustSBI: OK            (bootloader/rustsbi-qemu.bin 存在)
```

Makefile 变量手工展开与 `make -pn` 机器展开一致：

```text
TARGET           = riscv64gc-unknown-none-elf
MODE             = release
KERNEL_ELF       = target/riscv64gc-unknown-none-elf/release/os
KERNEL_BIN       = target/riscv64gc-unknown-none-elf/release/os.bin
FS_IMG           = ../user/target/riscv64gc-unknown-none-elf/release/fs.img
SBI              = rustsbi   (?= 默认值)
BOARD            = qemu
BOOTLOADER       = ../bootloader/rustsbi-qemu.bin
KERNEL_ENTRY_PA  = 0x80200000
```

### 4.3 任务 3 — 编译与 ELF 检查

```text
$ file target/.../release/os
ELF 64-bit LSB executable, UCB RISC-V, RVC, double-float ABI, statically linked

$ rust-objdump -f target/.../release/os
architecture: riscv64
start address: 0x0000000080200000

$ rust-nm -n target/.../release/os | grep -E '(_start|rust_main)$'
0000000080200000 T _start
0000000080218876 T rust_main

主要段：.text @0x80200000  .rodata @0x8022d000  .data @0x80238000  .bss @0x80239000
os (ELF) = 7.6M   os.bin (裸二进制) = 225K
```

启动链路在源码中的对应关系均已核对：

```text
linker-qemu.ld : ENTRY(_start) + BASE_ADDRESS = 0x80200000
entry.asm      : _start → la sp, boot_stack_top → call rust_main
main.rs        : #![no_std] #![no_main] + #[unsafe(no_mangle)] pub fn rust_main(...) -> !
mm/mod.rs      : mm::init() → heap_allocator::init_heap()
lang_items.rs  : 唯一一处 #[panic_handler]
console.rs     : 内核自己的 macro_rules! print / println
Cargo.toml     : lazy_static { features = ["spin_no_std"] } + buddy_system_allocator
```

### 4.4 任务 4 — QEMU 首次启动

`make run` 成功启动 RustSBI 并进入内核，串口输出（节选）：

```text
[rustsbi] RustSBI version 0.3.1, adapting to RISC-V SBI v1.0.0
[rustsbi] Platform SMP       : 1
[rustsbi] Platform Memory    : 0x80000000..0x88000000
[rustsbi] Supervisor Address : 0x80200000
...
/**** APPS ****
forktree ... hello_world ... user_shell ... (共 30 个用户程序)
Rust user shell
>> hello_world
Hello world from user mode program!
>> ls
...
```

关键证据全部命中：`Rust user shell`、`Hello world from user mode program!`、`/**** APPS ****`。
退出后 `pgrep -af qemu-system-riscv64` 无输出，`6200/6201` 端口空闲。

### 4.5 任务 5 — OpenSpec

```text
$ openspec init --tools none --language zh-CN     → openspec/config.yaml (schema: spec-driven, Language: zh-CN)
$ openspec validate --all --strict --no-interactive
   初始化后（尚无规格）        → No items found to validate.  exit=0
   安装 Lab 0 规格后           → ✓ spec/lab-0-environment   Totals: 1 passed, 0 failed   exit=0
```

Lab 0 规格含 6 组 Requirement（Rust 工具、固定源码、构建产物、QEMU 启动、OpenSpec 校验、自动复核），全部有对应证据。

### 4.6 任务 6 — 自动检查

```text
$ ~/oslab/ecnu-oslab-2026-task/lab-0/scripts/check-env.sh ~/Work/rCore-Tutorial-v3
PASS 项数: 30    FAIL 项: 无
lab-0 environment check: PASS
```

> 说明：`check-env.sh` 同时要求「当前分支 = `oslab-2026-base`」和「Lab 0 规格已存在」。按课程分支策略，`openspec/` 随 Lab 1 一起提交，因此复核时在只读基线上以**未跟踪**方式临时提供 `openspec/config.yaml` 与 `openspec/specs/lab-0-environment/spec.md`，复核完立即删除。基线分支始终干净且精确指向 `c91bd375...`。

---

## 五、Lab 1：机器启动与并发安全输出

### 5.1 实现内容

起始工程的 8 处 TODO 全部完成，涉及 4 个文件：

| 文件 | 实现要点 |
|---|---|
| `src/main.rs` | `start`：清 `mstatus.MPP` 并置 S-mode → `mepc = rust_main` → `satp = 0` → `pmpaddr0 = usize::MAX` / `pmpcfg0 = R\|W\|X\|A_TOR` → `tp = hart_id` → `mret`（`options(noreturn)`）。<br>`rust_main`：hart 0 初始化 UART 后 `STARTED.store(true, Release)`；其他 hart `while !STARTED.load(Acquire)`；双 hart 输出规定文本后进 `wfi`。 |
| `src/uart.rs` | `init()` 按 IER→LCR(锁存)→DLL/DLM→LCR(8位)→FCR 顺序配置；`putchar()` 先轮询 `LSR` bit5 再写 `THR`；寄存器访问全部 `read_volatile` / `write_volatile`。 |
| `src/sync.rs` | `SpinLock { locked: AtomicBool, owner: AtomicUsize }`；`lock()` 先 `push_off()` 关本 hart 中断，再 `swap(true, Acquire)` 竞争；`Drop` 里 `store(false, Release)` 后 `pop_off()`。按 hart 维护中断嵌套深度，只有深度回 0 才恢复中断；深度 0 调用 `pop_off` 视为错误。<br>`interrupt_nesting_selftest()` 检查两层进入、内层退出不提前开中断、最外层退出恢复原状态。 |
| `src/console.rs` | `static PRINT_LOCK: SpinLock`，`print()` 用 `let _guard = PRINT_LOCK.lock();` 覆盖**整次** `write_fmt`，因此锁粒度是“一条完整消息”而不是单个字符。 |

### 5.2 静态验收证据

```text
$ rust-objdump -f kernel-qemu.elf
architecture: riscv64
start address: 0x0000000080000000                    ← 入口 0x80000000

$ rust-nm -n kernel-qemu.elf | grep -E '(_entry|start|rust_main)$'
0000000080000000 T _entry
0000000080000236 T rust_main
000000008000055a T start

$ rg '<_entry>|mhartid|<start>' kernel.asm
<_entry>:  csrr a0, mhartid        ← 读 hart ID
           jalr 0x542(ra) <start>  ← 调用 Rust start

$ rg 'mstatus|mepc|satp|pmpaddr0|pmpcfg0|mret' kernel.asm
csrr a2, mstatus ; csrw mstatus, a2 ; csrw mepc, a2 ; csrw satp, a1
csrw pmpaddr0, a2 ; csrw pmpcfg0, a1 ; mret        ← 特权级切换齐全

AMO 指令 2 处：amoor.w.aq a4, a3, (a0)
fence 指令 12 处；sstatus CSR 操作 11 处（csrr/csrc/csrs sstatus）
```

> 关于 AMO：讲义明确“编译器可生成任意语义等价的 AMO 指令，本实验检查存在 AMO、fence 和 `sstatus` 操作，不要求固定生成 `amoswap`”。LLVM 为 `AtomicBool::swap(true, Acquire)` 生成 `amoor.w.aq`（带 `.aq` 的原子读-改-写），满足检查条件。

### 5.3 运行验收证据

```text
$ make clean && make check
ECNU OSLab rCore kernel entered S-mode
hart 1 says: char=B string=uart d=-2025 p=2025 x=0x1234567880000000
interrupt nesting self-test: PASS
Rust formatting: char=A string=uart d=-2025 p=2025 x=0x1234567880000000
hart 0 says: char=A string=uart d=-2025 p=2025 x=0x1234567880000000
lab-1 check: PASS          （exit code 0，0 warning）
```

五条规定输出全部出现，两条 hart 消息各出现 1 次且**行内无交错**。
`d=-2025` 验证有符号十进制、`p=2025` 验证无前缀十六进制、`x=0x1234567880000000` 验证 64 位未被截断为 32 位。

两行 hart 消息的先后顺序可变（并发调度决定），与讲义“两行先后顺序允许变化”一致。

手工启动 `make qemu` 也得到同样五条输出，并按 `Ctrl+A` → `X` 正常退出（`QEMU: Terminated`），无遗留进程。

---

## 六、Lab 2：物理内存与内核虚拟内存

### 6.1 实现内容

| 文件 | 实现要点 |
|---|---|
| `src/mm/address.rs` | 讲义已给完整实现，按要求阅读验证（floor 右移 12、ceil 先加 4095、page_offset 取低 12 位、`indexes()` 从后向前写入得到 `[VPN2,VPN1,VPN0]`）。仅补 `#[allow(dead_code)]` 标注消警告。 |
| `src/mm/frame_allocator.rs` | `AtomicBool FRAME_LOCK` + `AtomicUsize HEAD/START_PPN/END_PPN`；`lock()` 用 `swap(true, Acquire)`，`unlock()` 用 `store(false, Release)`。<br>`frame_allocator_init`：`start = kernel_end.ceil()`、`end = memory_end.floor()`，遍历 `[start,end)` 头插建链并断言页数。<br>`frame_alloc`：加锁取 next → 改 HEAD → **先解锁再清零** 4096 字节 → 返回 `FrameTracker`。<br>`FrameTracker::drop`：断言范围与页对齐 → 遍历链表查重复释放 → 头插回收。<br>另提供 `free_frame_count()`（带环/越界检测）与 `is_managed()`。 |
| `src/mm/page_table.rs` | `PageTableEntry` 补 `is_leaf()`、`is_writable_without_readable()`；`PteFlags` 补 `contains()`、`empty()`。<br>`PageTable` 用 `frames: [Option<FrameTracker>; 128]` 持有全部页表页，`frame_count` 计数。<br>`find_pte_create_ptr`：事务性创建中间页表，用最多 2 项的 `created` 数组记录 `(父页 PPN, PTE 下标)`，失败时先清父 PTE 再把 `frame_count` 回退并 `take()` 新增 tracker；遇到有效大页叶子返回错误。<br>`map` 拒绝 W-only 与不含 R/W/X 的权限、拒绝重复映射（原 PTE 不变）；`unmap` 只清叶子 PTE；`translate` 只查询、绝不分配。 |
| `src/mm/memory_set.rs` | 声明 9 个链接边界符号并断言顺序与页对齐；`map_identity_range` 做半开区间恒等映射。<br>分段权限：`[stext,etext)` R\|X、`[srodata,erodata)` R、`[sdata,edata)` R\|W、`[sbss,ebss)` R\|W、`[ekernel,0x8800_0000)` R\|W、`[UART_BASE, +4096)` R\|W，全部不含 U。<br>`verify_kernel_mapping` 查代表页权限并遍历全部映射检查 W^X。<br>通过后 `token = (8 << 60) \| root_ppn`，**先 Release 发布再 `core::mem::forget(page_table)`**。 |
| `src/mm/mod.rs` | `address_test` / `frame_test` / `page_table_test` / `run_tests`。前三项分别调用并各自输出 PASS/FAIL（不用 `&&` 短路），只有三项全 PASS 才 `kernel_space_init()`，再输出 `lab-2 kernel-space test`。 |
| `src/arch.rs` | 新增 `unsafe fn activate_page_table(token)`：`csrw satp` + `sfence.vma`。 |
| `src/main.rs` | hart 0：UART → 页帧初始化 → 四组自检；失败则打印后停在 `wfi`，**不启用分页**。hart 1：Acquire 等 `STARTED`。两 hart 都自旋等 `kernel_token()` 非零 → `activate_page_table` → 打印 `hart N paging enabled` → `wfi`。 |

### 6.2 页面生命周期与溢出安全的细节

- **头插 + 先解锁再清零**：`frame_alloc` 必须先把 `next` 读出来再清零，否则链表后继地址会被抹掉；清零时该页已脱离链表，因此无需继续持锁。
- **死循环写成 `while ppn < end_ppn`**：初始化用 `while` 而不是 `for ppn in start..end`，`ppn += 1` 不会越过 `end_ppn` 发生 `usize` 溢出 panic（裸机下 panic 即挂死）。
- **`end_ppn` 绝不写入**：只遍历 `[start_ppn, end_ppn)`，RAM 末端页不会被当作节点。
- **`FrameTracker` 不实现 `Copy`/`Clone`**：避免同一页有多个所有者被重复释放。

### 6.3 链接布局证据

```text
$ rust-nm -n kernel-qemu.elf | grep -E ' (stext|etext|...|ekernel)$'
0000000080000000 T stext
0000000080003000 T etext      = srodata
0000000080004000 R erodata    = sdata
0000000080005000 D edata      = sbss
0000000080011000 B ebss       = ekernel

顺序与页对齐均满足：0x80000000 = stext < etext = srodata <= erodata = sdata
                     <= edata = sbss <= ebss = ekernel < 0x88000000，ekernel & 0xfff = 0
```

`entry.S` 每 hart 栈步长 `16384`、保留空间 `16384 * 2`，两个 hart 各有互不重叠的 16 KiB 栈；`boot_stack` 位于 `.bss.stack`，落在 `sbss..ebss` 可读写区间内。

### 6.4 运行验收证据

```text
$ cargo fmt --check && make clean && make check
    Finished `release` profile        （0 warning）
lab-2 address test: PASS
lab-2 frame test: PASS
lab-2 page-table test: PASS
lab-2 kernel-space test: PASS
hart 1 paging enabled
hart 0 paging enabled
lab-2 check: PASS          （exit code 0）
```

```text
$ rg 'satp|sfence.vma' kernel.asm
8000117a: csrw    satp, a0        ← 写 satp
80001180: sfence.vma              ← 刷新 TLB
80001596: csrw    satp, a1        ← 两个 hart 各自执行
```

四条测试各出现 1 次（仅 hart 0 输出），`paging enabled` 出现 2 次（两个 hart）。分页启用后仍能输出，说明**当前 PC、内核栈、UART MMIO 和页表页本身**都已正确恒等映射。

---

## 七、Lab 3：中断与异常初步

### 7.1 实现内容

覆盖层提供 `trap.S` / `trap.rs` / `timer.rs` / `plic.rs` 四个文件；其中 `trap.S` **本实验不重写**（`cmp` 与覆盖层逐字节一致，仅按要求阅读核对），Rust 侧共 **9 处 TODO** 需要补全：`plic.rs 4` + `timer.rs 2` + `trap.rs 3`。其余改动落在 Lab 2 继承来的文件上。按讲义四个任务分列：

**任务一：S-mode trap 入口与委托**

| 文件 | 实现要点 |
|---|---|
| `src/main.rs` | 加 `mod plic; mod timer; mod trap;`，并追加 `core::arch::global_asm!(include_str!("trap.S"));`（原有 `entry.S` 语句保留）。<br>M-mode `start` 在原有 PMP / `mstatus.MPP=S` / `mepc` / `satp` / `tp` 之后、`mret` 之前写 `medeleg = 0xffff`、`mideleg = (1<<1)\|(1<<5)\|(1<<9)`。 |
| `src/arch.rs` | 新增 `write_medeleg` / `write_mideleg` / `set_stvec` 三个 `csrw` 封装，以及 `read_scause` / `read_sepc` / `read_stval` 三个诊断用 `csrr` 封装。<br>新增 `enable_supervisor_trap_interrupts()`：先 `csrs sie` 置 bit1(SSIE) 与 bit9(SEIE)，再 `csrs sstatus` 置 bit1(SIE)；**不打开未处理的 STIE**。<br>新增 `clear_supervisor_software_interrupt()`：`csrc sip, 1<<1` —— 用 `csrc` 只清 bit1，不覆盖 `sip` 其他位。 |
| `src/trap.rs` | `init_hart()` 严格按“写 `stvec` → `plic::init_hart(hart_id)` → 最后开中断开关”的顺序，保证中断不会先于入口准备完成而到达。 |

`medeleg` 委托全部标准同步异常，`mideleg` 委托 S-mode 软件(1)/时钟(5)/外部(9)中断；机器时钟中断 MTIP 不在 `mideleg` 里，它在任务三由 M-mode 入口转成 SSIP，这一点是两条路径的分工所在。

**任务二：UART 外部中断（PLIC）**

| 文件 | 实现要点 |
|---|---|
| `src/mm/memory_set.rs` | 在 Lab 2 的 UART 映射之后，复用**同一个** `map_identity_range` 以 `R\|W` 恒等映射 PLIC 三页：`0x0c00_0000`（priority）、`0x0c00_2000`（hart 0 S enable）、`0x0c20_1000`（threshold/claim）；不加 X、不加 U，也不重写页表算法。<br>`verify_kernel_mapping` 的期望表里补上这三页 `(R=true, W=true, X=false)`，并复用原有的 U 位检查。 |
| `src/plic.rs` | 四个 TODO：`init()` 把 UART IRQ 10 的 priority 写 1（寄存器地址 `PLIC + 10*4`）；`init_hart()` **仅当 `hart_id == 0`** 才写 S-context enable 的 bit10 与 threshold 0；`claim()` 用 `read_volatile` 读 `S_CLAIM_HART0`；`complete(irq)` 用 `write_volatile` 把**刚 claim 到的那个非零 irq** 写回同一地址，不写死成 10。 |
| `src/uart.rs` | 初始化末尾（DLAB 已清零）写 `IER = 1` 开接收使能；新增 `receive_byte()`：先查 `LSR` bit0 再读偏移 0 的 RHR；新增 `handle_receive_interrupt()`：`while let Some(byte) = receive_byte()` 循环读到 FIFO 为空，普通字节原样 `putchar`，`8`/`127` 输出“退格、空格、退格”，`\r`/`\n` 输出换行。 |
| `src/trap.rs` | `init_global()` 只调用一次 `plic::init()`（只由 hart 0 执行）。`kernel_trap_handler` 读 `scause`，仅当最高位为 1 且原因码为 9 时进入外部中断分支：先 `claim()`，IRQ 10 调 UART 接收处理，未知非零 IRQ 打印原因；**随后对每个非零 IRQ 执行一次 `complete(irq)`**，不漏 complete。 |

**任务三：时钟中断与 ticks**

| 文件 | 实现要点 |
|---|---|
| `src/timer.rs` | `init_machine(hart_id)`：`assert!(hart_id < MAX_HARTS)` → 读 `CLINT_MTIME=0x0200_bff8`，把 `now.wrapping_add(INTERVAL)` 写到本 hart 的 `CLINT_MTIMECMP+8*hart_id` → 取 `TIMER_SCRATCH[hart_id]` 的五字基址，槽位 3 写 `mtimecmp` 地址、槽位 4 写 `INTERVAL` → `csrw mscratch` → `csrw mtvec`（指向覆盖层已给的 `timer_vector`）→ `csrs mstatus, 1<<3`（MIE）→ `csrs mie, 1<<7`（MTIE）。两个 hart 各自使用自己的五个字，不共用。 |
| `src/main.rs` | `start(hart_id)` 在 M-mode、`mret` 之前调用一次 `timer::init_machine(hart_id)`。 |
| `src/timer.rs` | `update(hart_id)`：`hart_id == 0` 时 `TICKS.fetch_add(1, AcqRel)`，随后**两个 hart 都无条件清自己的 SSIP**；`ticks()` 用 `Acquire` 读。 |
| `src/trap.rs` | `scause` 最高位为 1 且原因码为 1 时调 `timer::update(arch::hart_id())`。 |

`INTERVAL = 1_000_000`，QEMU virt 的 `mtime` 频率为 10 MHz，因此一个 tick ≈ 0.1 s。

**任务四：启动顺序与验收输出**

| 文件 | 实现要点 |
|---|---|
| `src/main.rs` | 保留 Lab 2 的页表建立、发布与 `activate_page_table(token)`。**分页启用后**：hart 0 调 `trap::init_global()` 并 `TRAPS_READY.store(true, Release)`；hart 1 用 `while !TRAPS_READY.load(Acquire)` 等待；随后两 hart 各调 `trap::init_hart()`；最后由**仅 hart 0** 调 `uart::enable_receive_interrupt()`。<br>顺序即“PLIC 页已映射 → PLIC 全局初始化 → 每 hart 写 `stvec` → 打开中断 → UART 接收使能”。<br>每 hart 输出 `hart {hart_id} traps enabled`；hart 0 再输出 `lab-3 context test: PASS`（由覆盖层已有的 `trap::context_layout_ok()` 判断；该输出字符串按讲义原样保留，见 7.5 节末）。两 hart 最终都留在 `loop { arch::wait_for_interrupt(); }`，不从 `rust_main` 返回。 |
| `src/uart.rs` | 在接收循环内判断大写 `K`，用 `AtomicBool::swap(true, AcqRel)` 保证**仅一次**输出 `lab-3 uart test: PASS byte=K`。 |
| `src/console.rs` | **新增** `with_lock(body)`：把中断里的“循环读 + 回显”整段放进 `PRINT_LOCK`，修掉一处真实的输出交错缺陷（见 7.5 节与排障表）。 |

### 7.2 两条 trap 路径

```text
UART 输入 → PLIC(IRQ 10) → S-mode kernel_vector → kernel_trap_handler
          → scause=0x8000…09 → claim → 循环读/回显 → complete → sret

CLINT 时钟 → M-mode timer_vector → 续约 mtimecmp、置 sip.SSIP → mret
                                 → S-mode kernel_vector → scause=0x8000…01
                                 → ticks++（仅 hart 0）、清 SSIP → sret
```

两处容易写错、本实现刻意核对过的细节：

1. **`medeleg` 与 `mideleg` 的分工**：`mideleg` 只含 1/5/9 位，所以机器时钟中断不会被直接委托到 S-mode，必须先经 M-mode 的 `timer_vector` 转成 SSIP，S-mode 看到的才是“软件中断”。
2. **`mstatus.MIE` 与 `sstatus.SIE` 是两套开关**：M-mode 用 `mstatus.MIE` + `mie.MTIE` 收时钟，S-mode 用 `sstatus.SIE` + `sie.SSIE/SEIE` 收转发来的软件中断与 PLIC 外部中断，二者互不替代。
3. **`stvec` 低两位是 MODE**：用 Direct 模式（MODE=0），所以入口必须四字节对齐；`trap.S` 已用 `.align 4` 保证。
4. **初始化次序不可颠倒**：写 `sstatus.SIE` 必须是 `init_hart()` 的最后一步，`medeleg`/`mideleg` 与 `timer::init_machine` 必须都在 `mret` 之前完成。

### 7.3 `trap.S` 现场与 Rust 布局的对应

`#[repr(C)] KernelTrapContext { x: [usize; 32] }` 在 RV64 上是 32 × 8 = 256 字节，`x[n]` 的偏移正好是 `8*n`。逐个核对覆盖层给的入口：

- 入口先 `addi sp, sp, -0x100`（256 字节）开现场，`sd zero, 0(sp)` 保存恒为 0 的 `x0`；
- **先存旧 `t0` 到偏移 40**，再用 `t0` 算旧 `sp`：`addi t0, sp, 0x100` 后 `sd t0, 16(sp)` —— 否则会先破坏 `t0` 就拿不到正确的旧栈指针；
- 退出时按顺序恢复通用寄存器（`x0` 无需恢复），`addi sp, sp, 0x100` 收回现场，最后 `sret`。

反汇编证据（见 7.4）中 `80000030` 起的前几条正是上述序列，`80000076` 把现场地址放进 `a0` 后 `jalr` 到 `kernel_trap_handler`，`800000be` 是整个内核里**唯一**的一条 `sret`（全镜像 `mret` 共 2 条：`start` 的与 `timer_vector` 的）。

`timer_vector` 在 M-mode 无栈运行，现场靠 `mscratch` 指向的五字数组：`csrrw a0, mscratch, a0` 交换出 scratch 基址，把 `a1/a2/a3` 存到偏移 0/8/16，再从偏移 24（`mtimecmp` 地址）与 32（`INTERVAL`）取参数续约，`li a1, 2` + `csrs sip, a1` 置 SSIP，最后恢复 `a1/a2/a3/a0` 并 `mret`。这些偏移与 `timer.rs` 的 `scratch.add(3)` / `scratch.add(4)` 一一对应。

### 7.4 静态验收证据

```text
$ cargo fmt --check && make build && make disasm
$ rg -n '<kernel_vector>|<timer_vector>|csrw.*stvec|csrw.*mtvec|csrw.*mscratch|csrs sip' kernel.asm
23:0000000080000030 <kernel_vector>:
24-80000030: 7111         	addi	sp, sp, -0x100     ← 开 256 字节现场
25-80000032: e002         	sd	zero, 0x0(sp)      ← x0 存 0
27-80000036: f416         	sd	t0, 0x28(sp)       ← 先存旧 t0
28-80000038: 10010293     	addi	t0, sp, 0x100      ← 再用 t0 算旧 sp
29-8000003c: e816         	sd	t0, 0x10(sp)       ← 存旧 sp 到偏移 16
...（x1..x31 依 8*n 偏移全部保存，最后一条是 sd t6, 0xf8(sp)）
80000076: 850a         	mv	a0, sp             ← 现场地址作为第一个参数
8000007c: b28080e7     	jalr	-0x4d8(ra) <kernel_trap_handler>
...（x1..x31 全部恢复，x0 不需要恢复）
800000bc: 6111         	addi	sp, sp, 0x100      ← 收回现场
800000be: 10200073     	sret                   ← 唯一的 sret
98:00000000800000d0 <timer_vector>:
99-800000d0: 34051573     	csrrw	a0, mscratch, a0  ← 交换出 scratch
103-800000da: 6d0c         	ld	a1, 0x18(a0)       ← 偏移 24 = mtimecmp
104-800000dc: 7110         	ld	a2, 0x20(a0)       ← 偏移 32 = INTERVAL
105-800000de: 6194         	ld	a3, 0x0(a1)        ← 读旧比较值
106-800000e0: 96b2         	add	a3, a3, a2        ← 加 INTERVAL
107-800000e2: e194         	sd	a3, 0x0(a1)        ← 续约下一次
109-800000e6: 1445a073     	csrs	sip, a1            ← 置 SSIP 交给 S-mode
114-800000f4: 30200073     	mret

$ rg -n 'csrw\s+medeleg|csrw\s+mideleg|csrw\s+stvec|csrc\s+sip|csrs\s+mstatus|csrs\s+mie|csrs\s+sie|csrs\s+sstatus|csrw\s+mtvec|csrw\s+mscratch' kernel.asm
（摘录与 trap 相关的 10 条）
575:80000620: 34051073   csrw	mscratch, a0     ← timer::init_machine（两个 hart 各执行一次）
576:80000624: 30559073   csrw	mtvec, a1        ← 指向 timer_vector
577:80000628: 3006a073   csrs	mstatus, a3      ← mstatus.MIE
579:80000630: 30452073   csrs	mie, a0          ← mie.MTIE
1237:80000d06: 14453073  csrc	sip, a0          ← timer::update 清 SSIP
1963:80001568: 10551073  csrw	stvec, a0        ← trap::init_hart 写入口
1967:80001574: 10452073  csrs	sie, a0          ← SSIE|SEIE
1969:8000157a: 10052073  csrs	sstatus, a0      ← sstatus.SIE（最后一步）
2422:80001aa4: 30261073  csrw	medeleg, a2      ← M-mode 委托异常
2424:80001aac: 30359073  csrw	mideleg, a1      ← M-mode 委托中断
```

`scripts/check.sh` 用 `grep -E` 检查的 8 条静态项（`stvec`/`mtvec`/`mscratch`/`sip` 四组 CSR、`sret`、`mret`、`<kernel_vector>:`、`<timer_vector>:`）逐条实测命中。该脚本自己用 `$OBJDUMP -d`（默认 `rust-objdump`）反汇编到临时文件比对，不依赖仓库里是否留着 `kernel.asm`。

### 7.5 两处必须记录的实现决定

**(1) `console.rs` 新增 `with_lock()`：修掉真实的输出交错**

第一次完整验收失败，报 `missing expected output: hart 1 traps enabled`，日志里实际是：

```text
hart K1 traps enabled          ← `K` 插进了 hart 1 的消息中间
```

根因：`PRINT_LOCK` 只在 `print()` 里被持有，而 UART 中断里的回显直接调用 `putchar`，**不持锁**；当 hart 0 正在回显脚本注入的 `K`、hart 1 正在 `println!("hart 1 traps enabled")` 时，两者逐字节交错。

修复方式是把中断里的“循环读 + 回显”整段放进同一把锁（新增 `console::with_lock`）。两点关键理由：

- `SpinLock::lock()` 会先关**本 hart** 的 S-mode 中断，所以“中断处理里持锁”不会自死锁；
- 这把锁**不可重入**，因此中断处理**不能**在临界区内再调用 `println!`。大字 `K` 的测试标志因此改成：循环内只记录“是否见到 `K`”，退出临界区后再 `println!`。

修复后连续 5 次复跑均为 `lab-3 check: PASS`，且 `lab-3 uart test: PASS byte=K` 每次都只出现 1 次。

**(2) 时钟等待阈值由 3 提到 5**

讲义只要求“`ticks` 单调增长到至少 3”。本实现把 hart 0 的等待条件写成 `while timer::ticks() < 5`，输出仍是规定的 `lab-3 timer test: PASS ticks=5`（`ticks` 具体数值本就不固定，讲义允许大于 3）。这样“M-mode 每轮续约 `mtimecmp` + S-mode 每轮清 SSIP”必须**连续成立 5 轮**才算通过：若 `mtimecmp` 没有续约、或 SSIP 没被清掉，计数会停在 1，测试就不会通过。也就是说，把讲义里“只出现一个 tick”这条排障风险变成了可观测的验收条件。

另外，讲义给定的 `lab-3 context test: {PASS/FAIL}` 输出被保留原样，没有为了“更好看”而改写，因为 `scripts/check.sh` 用 `grep -F` 精确匹配该字符串。

### 7.6 运行验收证据

```text
$ cargo fmt --check && make clean && make check
   Compiling lab1-rcore v0.1.0 (/home/midnight/Work/rCore-Tutorial-v3/lab-3)
    Finished `release` profile [optimized] target(s) in 1.08s    ← 0 warning
cp target/riscv64gc-unknown-none-elf/release/lab1-rcore kernel-qemu.elf
QEMU="qemu-system-riscv64" OBJDUMP="rust-objdump" ./scripts/check.sh kernel-qemu.elf
lab-2 address test: PASS          ← Lab 2 自检仍全通过（回归）
lab-2 frame test: PASS
lab-2 page-table test: PASS
lab-2 kernel-space test: PASS
hart 1 traps enabled
hart 0 traps enabled
lab-3 context test: PASS
lab-3 timer test: PASS ticks=5
Klab-3 uart test: PASS byte=K     ← 前导 K 是回显，后接测试标志
lab-3 check: PASS                 （exit code 0）
```

> 末尾两条 `lab-3 timer test` / `lab-3 uart test` 的**先后顺序不固定**：时钟测试在约 0.5 s 完成，而串口字节由脚本在 `sleep 1` 后注入，两者互不依赖。多次运行中两种顺序都出现过，`check.sh` 也只按“是否包含”`grep -F` 断言、不约束顺序，因此顺序差异不属于抖动。

`lab-3/scripts/check.sh` 自行启动 QEMU（`-machine virt -bios none -m 128M -smp 2 -nographic`），在 `sleep 1` 后通过 FIFO 写入单个 `K`，随后最多轮询 20 秒等待 `lab-3 timer test: PASS` 与 `lab-3 uart test: PASS byte=K` 同时出现；无需手工按键或退出，脚本结束时由 `trap ... EXIT` 收尾杀掉 QEMU 与写管道进程。

**可重复性**：连续 5 次 `make build && ./scripts/check.sh kernel-qemu.elf` 全部 `lab-3 check: PASS`（每次 8 条静态 grep + 5 条 `require_output` 全部命中），未观察到抖动（末尾两条 PASS 的顺序差异见上）。

说明：`check.sh` 断言的是上表 8 条静态项与 5 条 Lab 3 运行输出（`lab-3 context test: PASS`、`hart 0/1 traps enabled`、`lab-3 timer test: PASS`、`lab-3 uart test: PASS byte=K`）。日志里那 4 条 `lab-2 * test: PASS` 不在脚本断言范围内，是本实现保留 Lab 2 自检后随之可见的**回归证据**——它同时说明内存与页表功能没有被中断改动破坏。

**补充用例**（讲义要求自行补充，输入 `a b c Backspace d Enter K`）：

```text
$ printf 'abc\bd\rK' | 注入 QEMU 串口
内核实际输出（cat -v）：abc^H ^Hd
                        K
```

逐项对照：普通字符 `abc` 原样回显；Backspace 输出 `^H ^H`（即 退格、空格、退格，终端上表现为擦掉一列）；回车经 `\r` 规范化成一个换行；`K` 回显并触发一次测试标志。**连续时钟中断**由 `ticks=5` 覆盖（5 轮不间断，见 7.5(2)）。

**未知 trap 的表现**：本实验不实现用户态 trap、系统调用与页故障恢复。handler 对未实现的同步异常与其他中断码调用 `report_unknown_trap()` 打印 `kind/code/scause/sepc/stval` 后进入 `wfi` 停机；**不清 `sepc`、也不把 `sepc` 加 4 后返回**，避免把异常当成“跳过一条指令”处理。本次验收路径没有触发该分支。

### 7.7 验收命令

```bash
cd ~/Work/rCore-Tutorial-v3
test -f openspec/config.yaml
mkdir -p openspec/specs/lab-3-trap-interrupt
cp ~/oslab/ecnu-oslab-2026-task/lab-3/lab-3-trap.spec.md \
   openspec/specs/lab-3-trap-interrupt/spec.md
openspec validate --all --strict --no-interactive      # 4 passed, 0 failed

cd lab-3
cargo fmt && cargo fmt --check
rg -n TODO src                                         # 期望无输出（当前 15 个文件全为 0）
make clean && make check                               # 期望 lab-3 check: PASS
```

---

## 八、分支与提交

```text
$ git log --oneline --graph --all --decorate
* 5205433  (lab-2) docs: 补充登录 shell PATH 缺失问题的根因、修复与严格复验方式
* b2ba99f  docs: 修正实验报告文件权限为 644
* e2ed216  docs: 添加 Lab 0/1/2 实验报告（WSL2 环境、验收证据、排障记录）
* 42229d3  lab-2: 完成物理内存与内核虚拟内存
| * 078f1d4  (lab-1) lab-1: 为排错用接口标注 allow(dead_code)，消除构建警告
|/
* e7b6d40  lab-1: 完成机器启动与并发安全输出
* c91bd37  (grafted, oslab-2026-base, main, origin/main) fix(build): skip installed cargo-binutils (#175)
   ↑ oslab-2026-base = c91bd3752b53ff48555aef4e3c7b8d5ddc8ee6e1（课程固定 commit，只读，无任何新增提交）
```

| 分支 | HEAD | 说明 |
|---|---|---|
| `oslab-2026-base` | `c91bd37` | 只读基线，**无任何新增提交**，工作区干净 |
| `lab-1` | `078f1d4` | Lab 1 工程 + `openspec/`（含 Lab 0 与 Lab 1 规格） |
| `lab-2` | `5205433` | Lab 2 工程（Lab 1 工程 + 覆盖层 + 内存实现）+ Lab 2 规格 + 报告的三次 `docs:` 提交 |
| `lab-3` | `5205433` + 报告 `docs:` 提交 | 从 `lab-2` 切出（`lab-2` 与 `lab-3` 一度同点）。本报告的 Lab 3 章节已作为 `docs:` 提交落在 `lab-3` 上（`git log -1 --oneline lab-3` 可查其哈希）；**Lab 3 的工程与规格尚未提交**，见下 |

各段实测差异（`refs/heads/` 前缀用于消歧，见第十节排障表）：

| 区间 | 实测 |
|---|---|
| `oslab-2026-base` → `lab-1` | 17 files changed, 748 insertions(+)（源码提交 `e7b6d40` 735 行 + `078f1d4` 15 增 2 删） |
| `lab-1` → `lab-2` | 22 files changed, 2162 insertions(+), 15 deletions(-)，其中源码提交 `42229d3` 为 20 files / 1597 insertions，其余 3 个是报告 `docs:` 提交（425 + 144 增 6 删 + 1 个仅改权限） |
| `lab-2` → `lab-3` | **0 差异**（Lab 3 的代码与规格不在版本库里，只存在于工作区） |

提交只暂存了明确路径（`git add lab-1` / `git add lab-2 openspec/...`），没有使用 `git add .`；`os/target/`、`user/target/`、`kernel-qemu.elf`、`kernel.asm` 等构建产物由各工程的 `.gitignore` 排除，未进入版本库。

Lab 3 的交付方式与前两个实验**不同**：按本人选择，`lab-3/` 与 `openspec/specs/lab-3-trap-interrupt/` 保留为**未跟踪的工作区交付物**，只有本报告这一节作为 `docs:` 提交落在 `lab-3` 分支上。因此 `lab-3` 的 HEAD 里没有 Lab 3 代码；要把它纳入版本库，按「九、如何复现」中的“Lab 3 的工作区状态（未提交）与自行提交步骤”两条命令即可。

---

## 九、如何复现

```bash
# 进入 WSL2
wsl -d Ubuntu-24.04

# 0) 工具链已在 ~/.bashrc 与 ~/.profile 中配好 PATH，
#    打开 WSL 终端（交互式登录 shell）后 cargo / node / openspec 直接可用，
#    无需手工 source。若你用了不读点文件的调用方式（如 wsl -e bash -c），
#    再显式执行一次：
#      source "$HOME/.cargo/env"
#      export NVM_DIR="$HOME/.nvm"; . "$NVM_DIR/nvm.sh"; nvm use default

cd ~/Work/rCore-Tutorial-v3

# 1) Lab 0：核对基线 + 构建 + 启动
git switch oslab-2026-base
git rev-parse HEAD                     # 期望 c91bd3752b53ff48555aef4e3c7b8d5ddc8ee6e1
cd os && make build && make run        # 进入 Rust user shell 后输入 hello_world、ls，Ctrl+C 退出
~/oslab/ecnu-oslab-2026-task/lab-0/scripts/check-env.sh ~/Work/rCore-Tutorial-v3

# 2) Lab 1
cd ~/Work/rCore-Tutorial-v3 && git switch lab-1 && cd lab-1
cargo fmt --check && make clean && make check      # 期望 lab-1 check: PASS

# 3) Lab 2
cd ~/Work/rCore-Tutorial-v3 && git switch lab-2 && cd lab-2
cargo fmt --check && make clean && make check      # 期望 lab-2 check: PASS

# 4) Lab 3
cd ~/Work/rCore-Tutorial-v3 && git switch lab-3 && cd lab-3
cargo fmt --check && make clean && make check      # 期望 lab-3 check: PASS

# 5) OpenSpec 严格校验
cd ~/Work/rCore-Tutorial-v3
openspec validate --all --strict --no-interactive  # 期望 4 passed, 0 failed
```

### Lab 3 的工作区状态（未提交）与自行提交步骤

Lab 3 的代码与规格目前**只在工作区**（本报告的 Lab 3 章节已提交），`git status` 显示为两个未跟踪项：

```text
$ cd ~/Work/rCore-Tutorial-v3 && git status --short
?? lab-3/
?? openspec/specs/lab-3-trap-interrupt/
```

`lab-3` 分支已经建好（从 `lab-2` 切出），本地目录 `lab-2/` 与 `lab-3/` 同时存在，且 `lab-3/` 里已完成全部 TODO（`rg -n TODO src` 无输出）并通过验收。若要把 Lab 3 提交进版本库，按 Lab 1 / Lab 2 的既有约定执行（只暂存明确路径，不用 `git add .`）：

```bash
cd ~/Work/rCore-Tutorial-v3
git add lab-3 openspec/specs/lab-3-trap-interrupt
git status --short                 # 确认只有这两项被暂存
git commit -m 'lab-3: 完成中断与异常初步'
```

注意：`lab-3/` 下的 `target/`、`kernel-qemu.elf`、`kernel.asm` 由各工程自带的 `.gitignore` 排除，不会进入提交；`Cargo.lock` 也被仓库根 `.gitignore` 的 `**/Cargo.lock` 排除（与 Lab 2 提交时的既有行为一致）。

### 工具可用性自检（不需要 source 任何文件）

```bash
# 在 WSL 终端里直接执行；应全部输出路径，缺失数为 0
for c in rustup rustc cargo rust-objcopy rust-objdump rust-nm \
         qemu-system-riscv64 node npm openspec; do
  printf '%-22s' "$c"; command -v "$c" || echo 'NOT FOUND'
done
```

### 本次使用的验收方式（重要）

验收脚本**内部不 source 任何 env 文件**，并在**交互式登录 shell** 中运行，
以真实反映“用户在新终端里敲命令”的可用性：

```bash
wsl.exe -u midnight -e bash -l -i -c 'bash .../oslab-scripts/75-strict-verify.sh'
```

修复 PATH 前的严格复验会出现 `cargo/node/openspec NOT_FOUND`；
修复后该脚本输出：

```text
PASS: PATH 已由登录 shell 提供 ~/.cargo/bin
PASS: PATH 已由登录 shell 提供 nvm node 目录
缺失命令数: 0
lab-3 check: PASS          ← Lab 3：context / uart / timer 三条 + Lab 2 回归四项
lab-2 check: PASS
lab-1 check: PASS
Totals: 4 passed, 0 failed (4 items)
missing=0  lab-1 fmt=0 check=0  lab-2 fmt=0 check=0  lab-3 fmt=0 check=0  openspec=0
STRICT VERIFY (no env sourcing, interactive login shell): PASS
```

一键复核脚本（Windows 侧 `default-workspace/oslab-scripts/`，可经 `/mnt/c/...` 在 WSL2 里直接执行）：

- `40-all-labs-verify.sh` —— **四个实验**的一致性复核：Lab 0 用材料自带 `check-env.sh`（在只读基线上以未跟踪方式临时提供规格与 `config.yaml`，跑完立即清理）、Lab 1 / Lab 2 / Lab 3 各自 `cargo fmt --check` + `make clean && make check`、四份规格的 OpenSpec 严格校验，最后打印 `ALL LABS (0,1,2,3) VERIFICATION: PASS` 并汇总工作区清洁度（Lab 3 未提交交付物单独计数，不计入“脏”）。
- `75-strict-verify.sh` —— 干净 shell 下的严格复验：内部**零 source**、由登录 shell 提供 PATH，检查 14 个工具可达性、版本，并顺序跑 Lab 1 / Lab 2 / **Lab 3** 的验收与 OpenSpec 严格校验，最后打印 `STRICT VERIFY ...: PASS`。

**前置条件**：切换分支前工作区里的跟踪文件不能有未提交修改（未跟踪的 `lab-3/` 与 Lab 3 规格不受影响）。若报告本身还有未提交修改，`git switch` 会拒绝执行以免覆盖改动；此时先提交报告或 `git stash`，跑完再 `git stash pop`。第十节排障表记录了这条实测现象。

两个脚本的实际输出（Lab 3 章节定稿后各跑一次，均退出码 0）：

```text
$ bash oslab-scripts/40-all-labs-verify.sh
lab-0 environment check: PASS            ← 30 项 PASS / 0 项 FAIL
Totals: 4 passed, 0 failed (4 items)     ← 四份规格严格校验
lab-1 check: PASS   （warnings=0）
lab-2 check: PASS   （warnings=0）
lab-3 check: PASS   （warnings=0，含 timer ticks=5 与 uart byte=K）
工作区改动条目数: 24（其中 Lab 3 未提交交付物 24 项，其余 0 项）
ALL LABS (0,1,2,3) VERIFICATION: PASS

$ bash -l -i -c 'bash oslab-scripts/75-strict-verify.sh'
缺失命令数: 0
missing=0  lab-1 fmt=0 check=0  lab-2 fmt=0 check=0  lab-3 fmt=0 check=0  openspec=0
STRICT VERIFY (no env sourcing, interactive login shell): PASS
```

---

## 十、排障记录（假设 → 检查命令 → 证据 → 结论）

| 现象 | 假设 | 检查 | 证据 | 结论 |
|---|---|---|---|---|
| **新终端里 `cargo fmt --check` 报 `Command 'cargo' not found`** | PATH 没有在“用户实际使用的 shell 类型”里生效 | 对 `bash -l` / `bash -i` / `bash -c` 三类 shell 分别探测 `command -v cargo` | 登录 shell 与普通 shell 都 `NOT_FOUND`；`grep cargo ~/.profile ~/.bashrc` 两处都没有 | rustup-init 当初用了 `--no-modify-path`，没写入任何 PATH 行。**已修复**：把 rustup 官方 PATH 片段写入 `~/.bashrc` 与 `~/.profile` |
| **新终端里 `node` / `openspec` 也找不到** | nvm 加载语句只在 `~/.bashrc`，而 WSL 默认启动的是**登录 shell**，它不读 `~/.bashrc` | 比较 `bash -l` 与 `bash -i` 下的 `command -v node`，并检查点文件与 `/etc/profile` | `bash -l` → `node NOT_FOUND`；`bash -i` → 找到；`grep nvm.sh ~/.profile` 为空 | WSL 无参数启动的是**交互式登录 shell**（读 `/etc/profile` → `~/.profile`，**不读** `~/.bashrc`）。**已修复**：把 nvm 加载 + `nvm use default` 也写入 `~/.profile` |
| 我的验收脚本全 PASS，用户手工执行却失败 | 脚本自己 `source ~/.cargo/env`，把环境缺陷掩盖了 | 对比脚本内是否 source，再用**内部零 source** 的脚本重跑 | 加 source 的脚本 PASS；不加 source 的脚本修复前有多项 `NOT_FOUND` | 验收必须在“干净 shell”里做。修正为 `wsl -e bash -l -i -c`（等同用户终端）且脚本内部零 source；修复后 `missing=0`、全部验收通过 |
| `make build` 报 `failed to get lose-net-stack` | Cargo 的 git 依赖要走 github，而 github 被墙 | `getent hosts github.com` + `/dev/tcp` 探测 | `github.com TCP443-FAIL`；`gh-proxy.com` 对两个依赖仓库 `ls-remote OK` | 用 `url.<base>.insteadOf` 把 github 重写到 `gh-proxy.com`，依赖正常解析（`git@github.com:` 形式也一并重写） |
| rCore 完整克隆反复 `early EOF` / `curl 18,56` | 代理链路不稳定或镜像缺该 commit | 对多个代理做 `git ls-remote`，检查是否含目标 commit；对 Gitee 镜像 `fetch --unshallow` 后 `cat-file -e` | `ghfast.top` 的 HEAD **就是** `c91bd375...`；Gitee 镜像 42 个 refs 中**没有**该 commit | 改用 `ghfast.top` 做 `--depth 1` 浅克隆，一次成功且 commit 精确匹配 |
| `sh.rustup.rs` 下载卡住 | 官方安装脚本域名被墙 | `curl -I` 探测 + 检查 rustup 日志停在 `downloading installer` | `sh.rustup.rs TCP443-FAIL`；`mirrors.ustc.edu.cn/rust-static` 206 且 170 KB/s | 从 USTC 镜像取 `rustup-init`，并设置 `RUSTUP_DIST_SERVER` / `RUSTUP_UPDATE_ROOT` |
| nvm 安装脚本克隆 `nvm-sh/nvm` 失败 | 脚本内部仍走 github | 读 install.sh 行为 | `unable to access 'https://github.com/nvm-sh/nvm.git'` | 用 Gitee 镜像脚本 + 上面的 git 重写规则，nvm 0.40.3 安装成功 |
| 起始工程构建出 16 个 `never used` 警告 | 未完成的 TODO 尚未引用这些接口 | `make build` | 警告全部指向 `arch.rs` / `sync.rs` 中待使用的封装 | 属讲义明确说明的预期现象；完成 TODO 后警告消失 |
| Lab 2 报 `PteFlags::empty` 不存在 | 骨架未提供该常量 | 编译错误指向 `src/mm/mod.rs:140` | `no associated function or constant named 'empty'` | 在 `PteFlags` 补 `pub const fn empty() -> Self`（测试“不含 R/W/X 权限”这条非法路径需要它） |
| Lab 2 报 `unused import: PageTableEntry` | 重导出未被外部使用 | 编译警告 | `src/mm/mod.rs:12` | 按讲义提示移除未使用的重导出，保留类型本身 |
| 复核脚本切分支失败 | 上一分支遗留的未跟踪构建产物会被覆盖 | `git status --short --untracked-files=all` | `?? lab-1/kernel-qemu.elf`、`?? lab-2/kernel-qemu.elf` | 切换前删除这些未跟踪产物；**绝不使用 `git clean -xfd` 或 `rm -rf openspec`**（曾误删跟踪文件，已用 `git restore` 恢复并改进脚本） |
| **一键复核脚本在 `git switch oslab-2026-base` 处中止** | 被跟踪的**实验报告**有未提交修改，而基线分支里没有这个文件 | 在临时仓库里按同样条件复现（文件存在于当前分支、目标分支不含它、且有本地修改） | `error: Your local changes to the following files would be overwritten by checkout: REPORT.md` → `Please commit your changes or stash them before you switch branches.` → `Aborting`；分支停在原处、修改未丢失 | git 拒绝“删除带本地修改的文件”，所以脚本的第一步就中止。跑一键复核脚本前先提交报告（或 `git stash`，跑完 `git stash pop`）。未跟踪的 `lab-3/` 与 Lab 3 规格不受影响，可正常跨分支切换 |
| `git diff lab-1 lab-2` 报 ambiguous argument | `lab-1`/`lab-2` 同时是分支名和目录名 | git 报错原文 | `both revision and filename` | 引用分支时加前缀：`refs/heads/lab-1`，或使用 `--` 分隔路径 |
| **Lab 3 首次验收报 `missing expected output: hart 1 traps enabled`，日志里是 `hart K1 traps enabled`** | 输入字符的回显没有和 `println!` 走同一把串口锁，回显字符插进了另一个 hart 正在打印的消息中间 | 读 `console.rs`：`PRINT_LOCK` 只被 `print()` 持有；`uart.rs` 的回显直接调 `putchar`，不持锁 | 日志 `hart K1 traps enabled`：`K` 恰好落在 `hart ` 与 `1 traps enabled` 之间 | **已修复**：`console.rs` 新增 `with_lock()`，把中断里“循环读 + 回显”整段放进同一把锁。`SpinLock::lock()` 先关本 hart 的 S-mode 中断，所以中断处理持锁不会自死锁；锁不可重入，故测试标志改到临界区之外打印。修复后连续 5 次复跑均 PASS |
| `timer test` 只到 `ticks=1` 就不再增长（假设性风险） | `mtimecmp` 没有续约，或 `sip.SSIP` 没被清除 | 读 `trap.S` 的 `timer_vector`：`ld a1,24(a0)` 取 `mtimecmp` 地址、`add a3,a3,a2` 加 `INTERVAL`、`sd` 写回；核对 `timer.rs` 的 scratch 偏移 24/32 | 反汇编 `800000de..800000e2` 正是“读旧值 → 加 INTERVAL → 写回”；`800000e6` 为 `csrs sip`（置 SSIP），`800000d06` 为 `csrc sip`（清 SSIP） | 两条路径都成立。为把该风险变成可观测验收，等待阈值由讲义的下限 3 提到 5：必须连续 5 轮“续约 + 清 SSIP”都成立才打印 PASS |
| PLIC 三页漏映射一页就会在开启分页后页故障（假设性风险） | 讲义明确警告：分页后访问 PLIC 会发生页故障 | 核对 `memory_set.rs` 的映射循环与 `verify_kernel_mapping` 的期望表 | 三页 `0x0c00_0000`/`0x0c00_2000`/`0x0c20_1000` 均以 R\|W 恒等映射，且都在权限自检表里（要求 R、W 为真，X、U 为假） | 三页齐全；每轮验收里 `lab-3 timer test` 与 `lab-3 uart test` 同时通过，说明时钟（不经 PLIC）与外部中断（经 PLIC）两条路径都可达 |
| Lab 0 自动检查报分支/commit 不符 | 在 `lab-2` 分支上跑了检查脚本 | `grep FAIL` 日志 | `expected branch oslab-2026-base, got lab-2` | 必须先 `git switch oslab-2026-base`；规格文件以未跟踪方式临时提供 |

---

## 十一、交付物清单

**学生仓库 `~/Work/rCore-Tutorial-v3`**

```text
oslab-2026-base  (只读基线 c91bd375...)
lab-1/                                     ← Lab 1 独立起始工程（8 处 TODO 已实现）
  ├── src/{main.rs, arch.rs, uart.rs, console.rs, sync.rs, entry.S}
  ├── scripts/check.sh   Makefile   kernel.ld   Cargo.toml   .cargo/config.toml
lab-2/                                     ← Lab 2（Lab 1 工程 + 覆盖层 + 内存实现）
  ├── src/{main.rs, arch.rs, uart.rs, console.rs, sync.rs, entry.S}
  ├── src/mm/{address.rs, frame_allocator.rs, page_table.rs, memory_set.rs, mod.rs}
  ├── scripts/check.sh   Makefile   kernel.ld
lab-3/                                     ← Lab 3（Lab 2 工程 + 覆盖层 + 中断实现）
  ├── src/{main.rs, arch.rs, uart.rs, console.rs, sync.rs, entry.S}
  ├── src/{trap.S, trap.rs, timer.rs, plic.rs}     ← 覆盖层提供的四个文件（TODO 已补全）
  ├── src/mm/{address.rs, frame_allocator.rs, page_table.rs, memory_set.rs, mod.rs}
  ├── scripts/check.sh   Makefile   kernel.ld
openspec/
  ├── config.yaml                          （schema: spec-driven, Language: zh-CN）
  ├── specs/lab-0-environment/spec.md      （6 Requirement / 6 Scenario）
  ├── specs/lab-1-machine-boot/spec.md     （8 Requirement / 9 Scenario）
  ├── specs/lab-2-memory/spec.md           （7 Requirement / 9 Scenario）
  └── specs/lab-3-trap-interrupt/spec.md   （5 Requirement / 6 Scenario）
OSLab-2026-Lab0-1-2-实验报告.md             ← 本报告（Lab 3 章节已提交为 docs: 1554ac0）
```

**Lab 1 源码规模**：`arch.rs 80` + `console.rs 35` + `entry.S 20` + `main.rs 87` + `sync.rs 129` + `uart.rs 48` = 399 行
**Lab 2 新增/替换源码**：`mm/address.rs 72` + `mm/frame_allocator.rs 192` + `mm/page_table.rs 284` + `mm/memory_set.rs 164` + `mm/mod.rs 181` = 893 行；另有 `arch.rs`、`main.rs`、`sync.rs` 的改动。
**Lab 3 新增/替换源码**：`trap.S 100` + `trap.rs 90` + `timer.rs 58` + `plic.rs 44` = 292 行（覆盖层四个文件，TODO 已补全）；`arch.rs 166`（+67）、`main.rs 154`（+60）、`uart.rs 120`（+74）、`console.rs 47`（+12）、`mm/memory_set.rs 183`（+19）。
相对 `lab-2` 的源码净变化：**9 files changed, 522 insertions(+), 2 deletions(-)**（不含 `target/`、`kernel-qemu.elf`、`kernel.asm`）。

**Lab 3 提交状态**：本报告的 Lab 3 章节已作为 `docs:` 提交落在 `lab-3` 分支；Lab 3 的**代码与规格尚未提交**（按本人选择保留在工作区）——`lab-3/` 与 `openspec/specs/lab-3-trap-interrupt/` 在 `git status` 中均为未跟踪（`??`）。自行提交步骤见「九、如何复现」中的“Lab 3 的工作区状态（未提交）与自行提交步骤”。

课程材料目录 `~/oslab/ecnu-oslab-2026-task` 全程保持只读（各实验的覆盖层都是用 `cp -R` 复制出来的，材料目录本身未被修改），`git status` 干净，HEAD 仍为 `6d57ab93b07a34a80e7c6cdfa7394095abc48134`。
