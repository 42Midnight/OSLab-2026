//! 首个用户进程：ELF 校验与装载、独立用户地址空间、内核栈与进程状态。
//!
//! Lab 5 在 Lab 4 的基础上把用户栈固定到高地址，并为堆、栈增长和 mmap 预留
//! 足够的用户帧所有权槽位。所有用户页帧都由本进程统一持有，页表只负责映射。

use crate::{
    mm::{
        self, frame_alloc, FrameTracker, PageTable, PhysAddr, PhysPageNum, PteFlags, VirtAddr,
        VirtPageNum, PAGE_SIZE,
    },
    mmap,
    task_context::TaskContext,
    trap_user::{self, UserTrapContext},
    user_vm::{MAX_STACK_PAGES, MMAP_BEGIN},
};

/// 首个用户程序的入口虚拟地址。
pub const USER_ENTRY: usize = 0x1000;
/// 用户栈顶固定在 1 GiB，与堆、mmap 区之间留出足够大的空洞。
pub const USER_STACK_TOP: usize = 0x4000_0000;
/// 首次装载时只映射一页用户栈，后续由 page fault 按页向下增长。
pub const USER_STACK_INITIAL_PAGES: usize = 1;
/// trampoline 在用户页表和内核页表中的固定虚拟地址。
pub const TRAMPOLINE: usize = 0x7fff_f000;
/// trap context 页，位于 trampoline 下方一页。
pub const TRAP_CONTEXT: usize = TRAMPOLINE - PAGE_SIZE;
/// 进程可持有的用户数据帧上限；ELF、堆、栈、mmap 和 trap context 都计入。
pub const MAX_OWNED_FRAMES: usize = 384;
/// 用户 trap 进入内核后使用的内核栈大小。
pub const KERNEL_STACK_SIZE: usize = 8 * PAGE_SIZE;

/// 单个进程的静态内核栈。栈顶按 16 字节对齐，trap 入口从该地址向下使用。
#[repr(align(4096))]
struct KernelStack([u8; KERNEL_STACK_SIZE]);

static mut USER_KERNEL_STACK: KernelStack = KernelStack([0; KERNEL_STACK_SIZE]);

/// 单个 ELF 最多装载的 LOAD 段页数（防止畸形镜像耗尽内核栈）。
const MAX_LOAD_PAGES: usize = 64;
/// 单个 ELF 允许的 PT_LOAD 段数上限。
const MAX_SEGMENTS: usize = 8;
/// Sv39 的 satp MODE 值，占 satp 最高 4 位；本实验 ASID 使用 0。
const SATP_MODE_SV39: usize = 8;

// ELF64 头与程序头的常量。只接受 64 位、小端、RISC-V 的静态可执行文件。
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const EI_CLASS: usize = 4;
const EI_DATA: usize = 5;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_RISCV: u16 = 0xf3;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

/// 一个 PT_LOAD 段在文件与用户地址空间中的位置。
#[derive(Copy, Clone)]
struct LoadSegment {
    offset: usize,
    vaddr: usize,
    filesz: usize,
    memsz: usize,
    flags: u32,
}

impl LoadSegment {
    const EMPTY: Self = Self {
        offset: 0,
        vaddr: 0,
        filesz: 0,
        memsz: 0,
        flags: 0,
    };

    fn end(&self) -> usize {
        self.vaddr + self.memsz
    }

    fn start_vpn(&self) -> usize {
        VirtAddr(self.vaddr).floor().0
    }

    fn end_vpn(&self) -> usize {
        VirtAddr(self.end()).ceil().0
    }
}

/// 通过边界检查解析出的 ELF 镜像信息。
struct ElfImage<'a> {
    data: &'a [u8],
    entry: usize,
    heap_top: usize,
    segments: [LoadSegment; MAX_SEGMENTS],
    segment_count: usize,
}

impl<'a> ElfImage<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.len() < EHDR_SIZE {
            return Err("elf: image is smaller than an ELF64 header");
        }
        if read_bytes::<4>(data, 0)? != ELF_MAGIC {
            return Err("elf: bad magic");
        }
        if read_u8(data, EI_CLASS)? != ELFCLASS64 {
            return Err("elf: not a 64-bit ELF");
        }
        if read_u8(data, EI_DATA)? != ELFDATA2LSB {
            return Err("elf: not little-endian");
        }
        if read_u16(data, 16)? != ET_EXEC {
            return Err("elf: not an executable image");
        }
        if read_u16(data, 18)? != EM_RISCV {
            return Err("elf: not a RISC-V image");
        }
        if read_u32(data, 20)? != 1 {
            return Err("elf: unsupported ELF version");
        }
        if read_u16(data, 52)? as usize != EHDR_SIZE {
            return Err("elf: unexpected ELF header size");
        }

        let entry = read_u64(data, 24)? as usize;
        let phoff = read_u64(data, 32)? as usize;
        let phentsize = read_u16(data, 54)? as usize;
        let phnum = read_u16(data, 56)? as usize;

        if phentsize != PHDR_SIZE {
            return Err("elf: unexpected program header size");
        }
        if phnum == 0 || phnum > MAX_SEGMENTS {
            return Err("elf: unsupported program header count");
        }
        let table_size = phentsize
            .checked_mul(phnum)
            .ok_or("elf: program header table size overflow")?;
        let table_end = phoff
            .checked_add(table_size)
            .ok_or("elf: program header table offset overflow")?;
        if table_end > data.len() {
            return Err("elf: program header table is out of range");
        }

        let mut image = Self {
            data,
            entry,
            heap_top: 0,
            segments: [LoadSegment::EMPTY; MAX_SEGMENTS],
            segment_count: 0,
        };

        for index in 0..phnum {
            let base = phoff + index * PHDR_SIZE;
            if read_u32(data, base)? != PT_LOAD {
                continue;
            }

            let flags = read_u32(data, base + 4)?;
            let offset = read_u64(data, base + 8)? as usize;
            let vaddr = read_u64(data, base + 16)? as usize;
            let filesz = read_u64(data, base + 32)? as usize;
            let memsz = read_u64(data, base + 40)? as usize;

            if filesz > memsz {
                return Err("elf: a segment has filesz larger than memsz");
            }
            if memsz == 0 {
                continue;
            }
            if flags & PF_W != 0 && flags & PF_X != 0 {
                return Err("elf: a segment is both writable and executable");
            }
            if flags & (PF_R | PF_W | PF_X) == 0 {
                return Err("elf: a segment has no access permission");
            }
            let file_end = offset
                .checked_add(filesz)
                .ok_or("elf: segment file range overflow")?;
            if file_end > data.len() {
                return Err("elf: segment data is out of range");
            }
            let segment_end = vaddr
                .checked_add(memsz)
                .ok_or("elf: segment address range overflow")?;
            if vaddr < PAGE_SIZE {
                return Err("elf: a segment overlaps the null page");
            }
            if segment_end > TRAP_CONTEXT {
                return Err("elf: a segment overlaps the trampoline area");
            }

            let segment = LoadSegment {
                offset,
                vaddr,
                filesz,
                memsz,
                flags,
            };

            // 段之间不允许共享页：否则后一段的权限会覆盖前一段。
            for existing in image.segments[0..image.segment_count].iter() {
                if segment.start_vpn() < existing.end_vpn()
                    && existing.start_vpn() < segment.end_vpn()
                {
                    return Err("elf: PT_LOAD segments overlap");
                }
            }

            image.segments[image.segment_count] = segment;
            image.segment_count += 1;
            image.heap_top = image.heap_top.max(segment_end);
        }

        if image.segment_count == 0 {
            return Err("elf: no PT_LOAD segment");
        }

        // 入口必须落在某个可执行段的范围内，否则第一次 sret 就会取指失败。
        let mut entry_is_executable = false;
        for segment in image.segments[0..image.segment_count].iter() {
            if segment.flags & PF_X != 0 && entry >= segment.vaddr && entry < segment.end() {
                entry_is_executable = true;
            }
        }
        if !entry_is_executable {
            return Err("elf: entry is not inside an executable segment");
        }

        Ok(image)
    }
}

/// 越界安全的定长读取：先检查长度，再做整数转换。
fn read_bytes<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], &'static str> {
    let end = offset.checked_add(N).ok_or("elf: field offset overflow")?;
    let slice = data
        .get(offset..end)
        .ok_or("elf: field read past the end of the image")?;
    let mut bytes = [0u8; N];
    bytes.copy_from_slice(slice);
    Ok(bytes)
}

fn read_u8(data: &[u8], offset: usize) -> Result<u8, &'static str> {
    Ok(read_bytes::<1>(data, offset)?[0])
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(read_bytes::<2>(data, offset)?))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(read_bytes::<4>(data, offset)?))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64, &'static str> {
    Ok(u64::from_le_bytes(read_bytes::<8>(data, offset)?))
}

/// 把 ELF `p_flags` 转成页表权限；`U` 由调用方统一追加。
fn segment_flags(segment: &LoadSegment) -> PteFlags {
    let mut flags = PteFlags::empty();
    if segment.flags & PF_R != 0 {
        flags = flags | PteFlags::R;
    }
    if segment.flags & PF_W != 0 {
        flags = flags | PteFlags::W;
    }
    if segment.flags & PF_X != 0 {
        flags = flags | PteFlags::X;
    }
    flags
}

/// trampoline 所在的物理页：取链接符号 `user_vector` 的地址后向下按页对齐。
fn trampoline_ppn() -> PhysPageNum {
    extern "C" {
        fn user_vector();
    }
    // 函数符号只用于取地址，不能调用。
    PhysAddr(user_vector as *const () as usize).floor()
}

/// 首个用户进程。
pub struct UserProcess {
    pid: usize,
    page_table: PageTable,
    heap_base: usize,
    heap_top: usize,
    stack_bottom: usize,
    stack_pages: usize,
    user_sp: usize,
    entry: usize,
    segment_count: usize,
    user_token: usize,
    /// ELF 段、用户栈、堆、mmap 与 trap context 的物理帧，随进程一起释放。
    frames: [Option<FrameTracker>; MAX_OWNED_FRAMES],
    frame_count: usize,
    trap_context: FrameTracker,
    mmap_head: Option<usize>,
    task_context: TaskContext,
}

impl UserProcess {
    /// 校验 ELF，建立独立用户页表并装载首个用户程序。
    pub fn from_elf(data: &[u8]) -> Result<Self, &'static str> {
        let image = ElfImage::parse(data)?;
        let mut page_table =
            PageTable::new().ok_or("from_elf: cannot allocate the user page table")?;

        // trampoline 与内核共享同一个物理页，用户页表只多一条 R|X 映射。
        page_table.map(
            VirtAddr(TRAMPOLINE).floor(),
            trampoline_ppn(),
            PteFlags::R | PteFlags::X,
        )?;

        let mut frames: [Option<FrameTracker>; MAX_OWNED_FRAMES] = core::array::from_fn(|_| None);
        let mut frame_count = 0usize;
        for index in 0..image.segment_count {
            load_segment(
                &mut page_table,
                &mut frames,
                &mut frame_count,
                &image.segments[index],
                image.data,
            )?;
        }

        // 用户栈固定在高地址，避免 brk 增长后与栈相撞。
        let stack_bottom = USER_STACK_TOP - USER_STACK_INITIAL_PAGES * PAGE_SIZE;
        let stack_bottom_vpn = VirtAddr(stack_bottom).floor();
        for page in 0..USER_STACK_INITIAL_PAGES {
            let frame = frame_alloc().ok_or("from_elf: out of free frames for the user stack")?;
            page_table.map(
                VirtPageNum(stack_bottom_vpn.0 + page),
                frame.ppn(),
                PteFlags::R | PteFlags::W | PteFlags::U,
            )?;
            frames[frame_count] = Some(frame);
            frame_count += 1;
        }
        let user_sp = USER_STACK_TOP;

        let trap_context =
            frame_alloc().ok_or("from_elf: out of free frames for the trap context")?;

        // trap context 是 S-mode 的私有页：用户页表只给 R|W，绝不给 U 或 X。
        page_table.map(
            VirtAddr(TRAP_CONTEXT).floor(),
            trap_context.ppn(),
            PteFlags::R | PteFlags::W,
        )?;

        let user_token = (SATP_MODE_SV39 << 60) | page_table.root_ppn().0;

        // 内核页表也要能看到 trampoline 与同一个 context 页：trap_return 设置
        // stvec 后跳到 TRAMPOLINE，trap 入口切换到内核页表后 handler 仍按
        // TRAP_CONTEXT 读写现场。放在所有可能失败的分配之后，避免留下悬空映射。
        mm::kernel_map(TRAMPOLINE, trampoline_ppn(), PteFlags::R | PteFlags::X)?;
        mm::kernel_map(TRAP_CONTEXT, trap_context.ppn(), PteFlags::R | PteFlags::W)?;

        Ok(Self {
            pid: 0,
            page_table,
            heap_base: image.heap_top,
            heap_top: image.heap_top,
            stack_bottom,
            stack_pages: USER_STACK_INITIAL_PAGES,
            user_sp,
            entry: image.entry,
            segment_count: image.segment_count,
            user_token,
            frames,
            frame_count,
            trap_context,
            mmap_head: None,
            task_context: TaskContext::zero(),
        })
    }

    pub fn pid(&self) -> usize {
        self.pid
    }

    pub fn entry(&self) -> usize {
        self.entry
    }

    pub fn segment_count(&self) -> usize {
        self.segment_count
    }

    pub fn heap_base(&self) -> usize {
        self.heap_base
    }

    pub fn heap_top(&self) -> usize {
        self.heap_top
    }

    pub fn set_heap_top(&mut self, value: usize) {
        self.heap_top = value;
    }

    pub fn stack_bottom(&self) -> usize {
        self.stack_bottom
    }

    pub fn stack_pages(&self) -> usize {
        self.stack_pages
    }

    /// 成功映射新栈页后更新栈边界与页数。
    pub fn grow_stack_metadata(&mut self, new_bottom: usize, added_pages: usize) {
        self.stack_bottom = new_bottom;
        self.stack_pages += added_pages;
    }

    pub fn user_sp(&self) -> usize {
        self.user_sp
    }

    pub fn loaded_pages(&self) -> usize {
        self.frame_count
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    pub fn page_table(&self) -> &PageTable {
        &self.page_table
    }

    pub fn page_table_mut(&mut self) -> &mut PageTable {
        &mut self.page_table
    }

    pub fn mmap_head(&self) -> Option<usize> {
        self.mmap_head
    }

    pub fn set_mmap_head(&mut self, head: Option<usize>) {
        self.mmap_head = head;
    }

    /// 把一个新分配的物理帧交给进程持有。
    pub fn push_frame(&mut self, frame: FrameTracker) -> Result<(), &'static str> {
        if self.frame_count >= MAX_OWNED_FRAMES {
            return Err("process: too many owned frames");
        }
        self.frames[self.frame_count] = Some(frame);
        self.frame_count += 1;
        Ok(())
    }

    /// 按物理页号取回并释放一个由本进程持有的帧。
    pub fn take_frame(&mut self, ppn: PhysPageNum) -> Option<FrameTracker> {
        for index in 0..self.frame_count {
            let matches = self.frames[index]
                .as_ref()
                .map(|frame| frame.ppn() == ppn)
                .unwrap_or(false);
            if !matches {
                continue;
            }

            let taken = self.frames[index].take();
            let last = self.frame_count - 1;
            self.frames[index] = self.frames[last].take();
            self.frame_count = last;
            return taken;
        }
        None
    }

    pub fn owns_frame(&self, ppn: PhysPageNum) -> bool {
        self.frames[0..self.frame_count]
            .iter()
            .any(|frame| frame.as_ref().map(|f| f.ppn() == ppn).unwrap_or(false))
    }

    /// 用户页表的 satp token。
    pub fn token(&self) -> usize {
        self.user_token
    }

    pub fn task_context_mut(&mut self) -> &mut TaskContext {
        &mut self.task_context
    }

    /// 首次进入用户态前初始化 TrapContext 与内核 TaskContext。
    pub fn init_trap_context(&mut self, kernel_satp: usize) {
        let kernel_sp = kernel_stack_top();
        let context = UserTrapContext::new(
            self.entry,
            self.user_sp,
            kernel_satp,
            kernel_sp,
            trap_user::handler_address(),
        );
        // Safety：trap context 页由 self.trap_context 独占，且按物理地址恒等映射。
        unsafe { self.trap_context_ptr().write(context) };
        self.task_context.goto_trap_return(kernel_sp);
    }

    /// trap context 的物理地址视图：内核页表恒等映射整个 RAM，因此可直接解引用。
    fn trap_context_ptr(&self) -> *mut UserTrapContext {
        self.trap_context.ppn().addr().0 as *mut UserTrapContext
    }

    /// 地址空间自检：逐页检查权限，任何一项失败都先打印出错的虚拟地址。
    pub fn layout_check(&self) -> bool {
        let mut ok = true;

        if !page_flags_ok(&self.page_table, USER_ENTRY, true, false, true, true) {
            println!(
                "lab-5 layout: code page {:#x} has wrong permissions",
                USER_ENTRY
            );
            ok = false;
        }
        if !page_flags_ok(
            &self.page_table,
            USER_STACK_TOP - PAGE_SIZE,
            true,
            true,
            false,
            true,
        ) {
            println!(
                "lab-5 layout: user stack page {:#x} has wrong permissions",
                USER_STACK_TOP - PAGE_SIZE
            );
            ok = false;
        }
        if self
            .page_table
            .translate(VirtAddr(USER_STACK_TOP).floor())
            .is_some()
        {
            println!(
                "lab-5 layout: page above user stack {:#x} must stay unmapped",
                USER_STACK_TOP
            );
            ok = false;
        }
        if self
            .page_table
            .translate(VirtAddr(self.stack_bottom - PAGE_SIZE).floor())
            .is_some()
        {
            println!(
                "lab-5 layout: page below user stack {:#x} must stay unmapped",
                self.stack_bottom - PAGE_SIZE
            );
            ok = false;
        }
        if !page_flags_ok(&self.page_table, TRAP_CONTEXT, true, true, false, false) {
            println!(
                "lab-5 layout: trap context page {:#x} has wrong permissions",
                TRAP_CONTEXT
            );
            ok = false;
        }
        if !page_flags_ok(&self.page_table, TRAMPOLINE, true, false, true, false) {
            println!(
                "lab-5 layout: trampoline page {:#x} has wrong permissions",
                TRAMPOLINE
            );
            ok = false;
        }
        for device in [0x1000_0000usize, 0x0c00_0000] {
            if self
                .page_table
                .translate(VirtAddr(device).floor())
                .is_some()
            {
                println!(
                    "lab-5 layout: user page table must not map device {:#x}",
                    device
                );
                ok = false;
            }
        }
        if self.heap_top > MMAP_BEGIN {
            println!(
                "lab-5 layout: heap top {:#x} reaches the mmap area",
                self.heap_top
            );
            ok = false;
        }
        if self.stack_pages > MAX_STACK_PAGES {
            println!(
                "lab-5 layout: stack has {} pages, more than {}",
                self.stack_pages, MAX_STACK_PAGES
            );
            ok = false;
        }
        if self.user_sp % 16 != 0 {
            println!(
                "lab-5 layout: user stack pointer {:#x} is not 16-byte aligned",
                self.user_sp
            );
            ok = false;
        }
        ok
    }
}

impl Drop for UserProcess {
    fn drop(&mut self) {
        // mmap 节点属于全局仓库：进程销毁时必须逐个归还。
        mmap::free_list(self.mmap_head.take());
    }
}

/// 用户 trap 内核栈的栈顶地址。静态数组按页对齐，栈顶再按 16 字节对齐。
pub fn kernel_stack_top() -> usize {
    let base = unsafe { core::ptr::addr_of_mut!(USER_KERNEL_STACK.0) as usize };
    (base + KERNEL_STACK_SIZE) & !0xf
}

/// 按 PT_LOAD 逐页申请、复制并映射一个段。
fn load_segment(
    page_table: &mut PageTable,
    frames: &mut [Option<FrameTracker>; MAX_OWNED_FRAMES],
    frame_count: &mut usize,
    segment: &LoadSegment,
    data: &[u8],
) -> Result<(), &'static str> {
    let flags = segment_flags(segment) | PteFlags::U;
    for vpn in segment.start_vpn()..segment.end_vpn() {
        if *frame_count == MAX_LOAD_PAGES {
            return Err("elf: too many LOAD pages");
        }
        let frame = frame_alloc().ok_or("elf: out of free frames")?;
        let ppn = frame.ppn();
        copy_segment_page(ppn, vpn, segment, data)?;
        page_table.map(VirtPageNum(vpn), ppn, flags)?;
        frames[*frame_count] = Some(frame);
        *frame_count += 1;
    }
    Ok(())
}

/// 把段中落在某一页的文件内容复制到该页的物理地址，其余字节保持为零。
fn copy_segment_page(
    ppn: PhysPageNum,
    vpn: usize,
    segment: &LoadSegment,
    data: &[u8],
) -> Result<(), &'static str> {
    let page_start = VirtPageNum(vpn).addr().0;
    let page_end = page_start + PAGE_SIZE;
    let copy_start = page_start.max(segment.vaddr);
    let copy_end = page_end.min(segment.vaddr + segment.filesz);
    if copy_end <= copy_start {
        return Ok(());
    }

    let length = copy_end - copy_start;
    let source = segment
        .offset
        .checked_add(copy_start - segment.vaddr)
        .ok_or("elf: segment file offset overflow")?;
    // Safety：source 与 length 已由 ElfImage::parse 的范围检查保证在 data 内；
    // 目标页由 frame_alloc 清零后独占。
    unsafe {
        core::ptr::copy_nonoverlapping(
            data.as_ptr().add(source),
            (ppn.addr().0 + (copy_start - page_start)) as *mut u8,
            length,
        );
    }
    Ok(())
}

/// 判断某一页是否存在且权限恰好匹配期望值。
fn page_flags_ok(
    page_table: &PageTable,
    address: usize,
    read: bool,
    write: bool,
    execute: bool,
    user: bool,
) -> bool {
    match page_table.translate(VirtAddr(address).floor()) {
        Some(entry) => {
            let flags = entry.flags();
            flags.contains(PteFlags::R) == read
                && flags.contains(PteFlags::W) == write
                && flags.contains(PteFlags::X) == execute
                && flags.contains(PteFlags::U) == user
        }
        None => false,
    }
}

/// 当前进程的全局句柄。单进程阶段用它提供 `myproc` 等价访问。
static mut CURRENT: Option<UserProcess> = None;

pub fn set_current(process: UserProcess) {
    // Safety：启动阶段只有 hart 0 写这个句柄，写完之后只读访问。
    unsafe { *core::ptr::addr_of_mut!(CURRENT) = Some(process) };
}

pub fn current() -> Option<&'static mut UserProcess> {
    // Safety：同一个 hart 内串行访问；引用不在中断之间共享。
    unsafe { (*core::ptr::addr_of_mut!(CURRENT)).as_mut() }
}
