# Lab 4 starter

本目录是安装在已完成 Lab 3 工程上的覆盖层。先复制 lab-3 为 lab-4，再覆盖本目录：

    cp -R lab-3 lab-4
    cp -R "$OSLAB_MATERIALS/starter/." lab-4/

覆盖层提供用户进程模块、内核上下文切换汇编、用户 trap 汇编骨架、用户 ELF 接入占位和
验收脚本。
Lab 3 的页表、帧分配、PLIC、时钟、UART 和内核 trap 代码继续使用；不要复制第二份。
首次 make build 只检查覆盖不破坏 Lab 3。按 lab-4.md 声明模块后才会编译这些骨架。

刚安装覆盖层时保留的 TODO 会使 `make check` 失败，这是预期的起点状态。完成 lab-4.md
任务 2–7 后，`rg -n TODO src` 应无输出，再运行 `make check`，并以 `lab-4 check: PASS`
结束。
