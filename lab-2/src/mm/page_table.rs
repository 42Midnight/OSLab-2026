//! 三级 Sv39 页表：只支持 4 KiB 叶子映射，不支持 1 GiB / 2 MiB 大页。

use super::{frame_alloc, FrameTracker, PhysPageNum, VirtPageNum};

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct PageTableEntry(pub usize);

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct PteFlags(pub usize);

impl PteFlags {
    pub const V: Self = Self(1 << 0);
    pub const R: Self = Self(1 << 1);
    pub const W: Self = Self(1 << 2);
    pub const X: Self = Self(1 << 3);
    pub const U: Self = Self(1 << 4);

    /// 不含任何权限位的空标志，用于测试“非法叶子权限”这条错误路径。
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> usize {
        self.0
    }

    /// 判断当前标志是否包含 `flag` 的全部位。
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

impl core::ops::BitOr for PteFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl PageTableEntry {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn new(ppn: PhysPageNum, flags: PteFlags) -> Self {
        Self((ppn.0 << 10) | flags.bits())
    }

    pub fn ppn(self) -> PhysPageNum {
        PhysPageNum(self.0 >> 10)
    }

    pub fn flags(self) -> PteFlags {
        PteFlags(self.0 & 0x3ff)
    }

    pub fn is_valid(self) -> bool {
        self.0 & PteFlags::V.bits() != 0
    }

    /// R、W、X 三位中至少一位为 1 表示叶子（非叶子页表项只有 V 位）。
    pub fn is_leaf(self) -> bool {
        self.0 & (PteFlags::R.bits() | PteFlags::W.bits() | PteFlags::X.bits()) != 0
    }

    /// W=1 且 R=0 是 RISC-V 规定的非法组合。
    ///
    /// `map` 自己用的是等价的位测试，这里保留讲义要求的判断方法供阅读与后续复用。
    #[allow(dead_code)]
    pub fn is_writable_without_readable(self) -> bool {
        self.flags().contains(PteFlags::W) && !self.flags().contains(PteFlags::R)
    }
}

const MAX_PAGE_TABLE_FRAMES: usize = 128;

/// 页表页的 PTE 数组视图；`4096 / size_of::<PageTableEntry>() = 512`。
///
/// # Safety
/// `ppn` 必须是由页帧分配器返回、且由某个 `PageTable` 持有的页表页。
unsafe fn pte_array(ppn: PhysPageNum) -> &'static [PageTableEntry; 512] {
    unsafe { &*(ppn.addr().0 as *const [PageTableEntry; 512]) }
}

/// # Safety
/// 同 [`pte_array`]；调用者必须保证引用存活期间对应 tracker 不被释放。
unsafe fn pte_array_mut(ppn: PhysPageNum) -> &'static mut [PageTableEntry; 512] {
    unsafe { &mut *(ppn.addr().0 as *mut [PageTableEntry; 512]) }
}

pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: [Option<FrameTracker>; MAX_PAGE_TABLE_FRAMES],
    frame_count: usize,
}

impl PageTable {
    pub fn new() -> Option<Self> {
        let root = frame_alloc()?;
        let root_ppn = root.ppn();
        let mut frames = core::array::from_fn(|_| None);
        frames[0] = Some(root);
        Some(Self {
            root_ppn,
            frames,
            frame_count: 1,
        })
    }

    pub fn root_ppn(&self) -> PhysPageNum {
        self.root_ppn
    }

    /// 记录一个新分配的页表页；容量不足时返回 `false`，由调用者回滚。
    fn push_frame(&mut self, frame: FrameTracker) -> bool {
        if self.frame_count >= MAX_PAGE_TABLE_FRAMES {
            // frame 在此处离开作用域，页帧自动回收。
            return false;
        }
        self.frames[self.frame_count] = Some(frame);
        self.frame_count += 1;
        true
    }

    /// 事务性回滚：清空本次新建的父 PTE，并释放本次新增的页表页。
    fn rollback(&mut self, created: &[(PhysPageNum, usize)], previous_count: usize) {
        for &(parent_ppn, index) in created.iter().rev() {
            // Safety：parent_ppn 是本次遍历中见过的有效页表页。
            unsafe { pte_array_mut(parent_ppn)[index] = PageTableEntry::empty() };
        }
        while self.frame_count > previous_count {
            self.frame_count -= 1;
            // take() 让 FrameTracker 被 Drop，页帧回到空闲链表。
            self.frames[self.frame_count].take();
        }
    }

    pub fn map(
        &mut self,
        vpn: VirtPageNum,
        ppn: PhysPageNum,
        flags: PteFlags,
    ) -> Result<(), &'static str> {
        if flags.contains(PteFlags::W) && !flags.contains(PteFlags::R) {
            return Err("map: W=1 with R=0 is an illegal leaf permission");
        }
        if !(flags.contains(PteFlags::R)
            || flags.contains(PteFlags::W)
            || flags.contains(PteFlags::X))
        {
            return Err("map: leaf permission must include at least one of R/W/X");
        }

        // 创建型遍历会建立起全部缺失的中间页表，并返回最后一级 PTE。
        // Safety：返回指针指向由 self.frames 持有的页表页，且在帧不被释放时保持有效。
        let leaf_ptr = unsafe { self.find_pte_create_ptr(vpn)? };
        // Safety：沿用 find_pte_create_ptr 的前置条件。
        let leaf = unsafe { &mut *leaf_ptr };
        if leaf.is_valid() {
            // 重复映射：保持原 PTE 不变并报错。
            return Err("map: the virtual page is already mapped");
        }
        *leaf = PageTableEntry::new(ppn, flags | PteFlags::V);
        Ok(())
    }

    pub fn unmap(&mut self, vpn: VirtPageNum) -> Result<(), &'static str> {
        // Safety：引用存活期间 self.frames 不移除页表页。
        let leaf = unsafe { self.find_pte_mut(vpn) };
        match leaf {
            Some(entry) if entry.is_valid() && entry.is_leaf() => {
                *entry = PageTableEntry::empty();
                Ok(())
            }
            Some(_) => Err("unmap: the entry is not a valid leaf"),
            None => Err("unmap: the virtual page is not mapped"),
        }
    }

    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        // Safety：引用存活期间 self.frames 不移除页表页。
        let leaf = unsafe { self.find_pte(vpn) }?;
        if leaf.is_valid() && leaf.is_leaf() {
            Some(*leaf)
        } else {
            None
        }
    }

    /// 只查询遍历：任何无效项都立即返回 `None`，绝不分配页表页。
    ///
    /// # Safety
    /// 页表页由 `self.frames` 持有，返回的引用存活期间不能 Drop 对应 tracker。
    unsafe fn find_pte(&self, vpn: VirtPageNum) -> Option<&PageTableEntry> {
        let indexes = vpn.indexes();
        let mut current = self.root_ppn;

        for &index in &indexes[0..2] {
            // Safety：current 始终是有效页表页的 PPN。
            let parent = unsafe { pte_array(current) }[index];
            if !parent.is_valid() || parent.is_leaf() {
                return None;
            }
            current = parent.ppn();
        }

        // Safety：同 pte_array 的前置条件。
        Some(&unsafe { pte_array(current) }[indexes[2]])
    }

    /// 只查询遍历的可变版本，供 `unmap` 使用。
    ///
    /// # Safety
    /// 调用者持有 `&mut self`；返回的可变引用存活期间不能再用 `&self` 访问同一页表。
    unsafe fn find_pte_mut(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let indexes = vpn.indexes();
        let mut current = self.root_ppn;

        for &index in &indexes[0..2] {
            // Safety：current 始终是有效页表页的 PPN。
            let parent = unsafe { pte_array(current) }[index];
            if !parent.is_valid() || parent.is_leaf() {
                return None;
            }
            current = parent.ppn();
        }

        // Safety：同 pte_array 的前置条件。
        Some(&mut unsafe { pte_array_mut(current) }[indexes[2]])
    }

    /// 创建型遍历：返回最后一级 PTE 的原始指针，缺失的中间页表会被建立。
    unsafe fn find_pte_create_ptr(
        &mut self,
        vpn: VirtPageNum,
    ) -> Result<*mut PageTableEntry, &'static str> {
        let indexes = vpn.indexes();
        let mut current = self.root_ppn;
        let previous_count = self.frame_count;
        let mut created: [(PhysPageNum, usize); 2] = [(PhysPageNum(0), 0); 2];
        let mut created_len = 0usize;

        for level in 0..2 {
            let index = indexes[level];
            // Safety：current 始终是有效页表页的 PPN。
            let parent = unsafe { pte_array(current) }[index];

            if parent.is_valid() {
                if parent.is_leaf() {
                    // 不支持大页；本次新建的页表页全部回滚。
                    self.rollback(&created[0..created_len], previous_count);
                    return Err("find_pte_create: unexpected huge-page leaf");
                }
                current = parent.ppn();
                continue;
            }

            let frame = match frame_alloc() {
                Some(frame) => frame,
                None => {
                    self.rollback(&created[0..created_len], previous_count);
                    return Err("find_pte_create: out of free frames");
                }
            };
            let next_ppn = frame.ppn();
            if !self.push_frame(frame) {
                self.rollback(&created[0..created_len], previous_count);
                return Err("find_pte_create: page-table frame array is full");
            }

            // 中间页表项只有 V 位，表示指向下一级页表。
            unsafe { pte_array_mut(current)[index] = PageTableEntry::new(next_ppn, PteFlags::V) };
            created[created_len] = (current, index);
            created_len += 1;
            current = next_ppn;
        }

        // Safety：current 是最后一级页表页的 PPN。
        Ok(&mut unsafe { pte_array_mut(current) }[indexes[2]] as *mut PageTableEntry)
    }
}
