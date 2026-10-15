//! Owned copy of the bootstrap hierarchy, retaining leaf policy except guard pages.
use crate::{Serial, interrupts};
use core::{
    arch::asm,
    fmt::Write,
    ptr::{addr_of_mut, read_volatile, write_volatile},
};
use regis_memory::{Frames, PAGE};
const ADDRESS: u64 = 0x000f_ffff_ffff_f000;
const MAX: usize = 4096;
struct Copy {
    old: [u64; MAX],
    new: [u64; MAX],
    levels: [u8; MAX],
    count: usize,
}
static mut COPY: Copy = Copy {
    old: [0; MAX],
    new: [0; MAX],
    levels: [0; MAX],
    count: 0,
};
impl Copy {
    fn page(&mut self, frames: &mut Frames<'_>) -> u64 {
        assert!(self.count < MAX, "owned hierarchy capacity");
        let address = frames.allocate(1).expect("owned page-table allocation");
        unsafe {
            core::ptr::write_bytes(address as *mut u8, 0, 4096);
        }
        self.new[self.count] = address;
        self.count += 1;
        address
    }
    fn clone_tree(&mut self, old: u64, level: u8, frames: &mut Frames<'_>) -> u64 {
        if let Some(i) = self.old[..self.count].iter().position(|p| *p == old) {
            assert_eq!(self.levels[i], level, "recursive/ambiguous firmware table");
            return self.new[i];
        }
        assert!(self.count < MAX, "owned hierarchy capacity");
        let new = self.page(frames);
        self.old[self.count - 1] = old;
        self.levels[self.count - 1] = level;
        for i in 0..512 {
            let mut entry = unsafe { read_volatile((old as *const u64).add(i)) };
            if entry & 1 != 0 && level > 1 && entry & (1 << 7) == 0 {
                entry = (entry & !ADDRESS) | self.clone_tree(entry & ADDRESS, level - 1, frames);
            } else if entry & 1 != 0 && level == 4 {
                panic!("invalid PML4 huge entry");
            }
            unsafe {
                write_volatile((new as *mut u64).add(i), entry);
            }
        }
        new
    }
    fn audit(&self) {
        for i in 0..self.count {
            assert!(
                !self.old[..self.count].contains(&self.new[i]),
                "owned table overlaps firmware table"
            );
            assert!(
                !self.new[..i].contains(&self.new[i]),
                "duplicate owned frame"
            );
            if self.levels[i] <= 1 {
                continue;
            }
            for slot in 0..512 {
                let entry = unsafe { read_volatile((self.new[i] as *const u64).add(slot)) };
                if entry & 1 != 0 && entry & (1 << 7) == 0 {
                    assert!(
                        self.new[..self.count].contains(&(entry & ADDRESS)),
                        "firmware table reference remains"
                    );
                }
            }
        }
    }
    fn remove_guard(&mut self, root: u64, address: u64, frames: &mut Frames<'_>) {
        let mut table = root;
        for level in (1..=4).rev() {
            let index = ((address >> (12 + (level - 1) * 9)) & 511) as usize;
            let slot = unsafe { (table as *mut u64).add(index) };
            let entry = unsafe { read_volatile(slot) };
            assert!(entry & 1 != 0, "guard initially mapped");
            if level == 1 {
                unsafe { write_volatile(slot, 0) };
                return;
            }
            if entry & (1 << 7) != 0 {
                let child = self.page(frames);
                self.levels[self.count - 1] = level - 1;
                for i in 0..512 {
                    let value = regis_memory::paging::split_leaf(entry, level, i)
                        .expect("large leaf split");
                    unsafe {
                        write_volatile((child as *mut u64).add(i), value);
                    }
                }
                let flags = entry & (0x3f | (1 << 63));
                unsafe {
                    write_volatile(slot, child | flags);
                }
                table = child;
            } else {
                table = entry & ADDRESS;
            }
            assert!(self.new[..self.count].contains(&table));
        }
    }
}
/// Normalize effective translation/rights for comparison across huge-leaf splits.
fn mapping(root: u64, address: u64) -> Option<(u64, u64)> {
    let mut table = root;
    let mut rw = true;
    let mut user = true;
    let mut nx = false;
    for level in (1..=4).rev() {
        let shift = 12 + (level - 1) * 9;
        let entry = unsafe {
            read_volatile((table as *const u64).add(((address >> shift) & 511) as usize))
        };
        if entry & 1 == 0 {
            return None;
        }
        rw &= entry & 2 != 0;
        user &= entry & 4 != 0;
        nx |= entry & (1 << 63) != 0;
        if level == 1 || entry & (1 << 7) != 0 {
            let mask = (1u64 << shift) - 1;
            let physical = (entry & ADDRESS & !mask) | (address & mask);
            let pat = entry & if level == 1 { 1 << 7 } else { 1 << 12 } != 0;
            let rights = (u64::from(rw) << 1)
                | (u64::from(user) << 2)
                | (u64::from(nx) << 63)
                | (u64::from(pat) << 7)
                | (entry & (0x118 | (0xf << 59)));
            return Some((physical, rights));
        }
        table = entry & ADDRESS;
    }
    None
}
fn present(root: u64, address: u64) -> bool {
    mapping(root, address).is_some()
}
#[inline(never)]
pub fn install(frames: &mut Frames<'_>) {
    let old_cr3: u64;
    let cr4: u64;
    unsafe {
        asm!("mov {}, cr3",out(reg) old_cr3,options(nomem,nostack));
        asm!("mov {}, cr4",out(reg) cr4,options(nomem,nostack));
    }
    assert!(
        cr4 & ((1 << 12) | (1 << 17)) == 0,
        "four-level non-PCID bootstrap required"
    );
    let old = old_cr3 & ADDRESS;
    let before = frames.free_pages();
    let copy = unsafe { &mut *addr_of_mut!(COPY) };
    assert_eq!(copy.count, 0, "paging initialized once");
    let root = copy.clone_tree(old, 4, frames);
    assert_ne!(root, old);

    let guards = interrupts::guards();
    #[cfg(feature = "tasks")]
    let guards = {
        let task = crate::tasks::guards();
        [
            guards[0], guards[1], guards[2], guards[3], task[0], task[1], task[2], task[3],
        ]
    };
    for guard in guards {
        assert_eq!(guard % PAGE, 0);
        copy.remove_guard(root, guard, frames);
        assert!(!present(root, guard));
        assert!(present(old, guard));
    }
    copy.audit();
    for guard in guards {
        for adjacent in [guard - 1, guard + PAGE] {
            if guards
                .iter()
                .any(|g| *g <= adjacent && adjacent < *g + PAGE)
            {
                continue;
            }
            assert_eq!(
                mapping(root, adjacent),
                mapping(old, adjacent),
                "guard neighbor changed"
            );
        }
    }
    assert_eq!(before - frames.free_pages(), copy.count);
    // Retain the previous hierarchy forever for now. No allocator free/reclamation.
    // Clear PGE while switching: CR3 alone does not invalidate global translations.
    unsafe {
        asm!("cli", options(nostack));
        asm!("mov cr4, {noglobal}", "mov cr3, {root}", "mov cr4, {restore}",
            noglobal=in(reg) cr4&!(1<<7),root=in(reg) root,restore=in(reg) cr4,options(nostack));
    }
    let active: u64;
    unsafe {
        asm!("mov {}, cr3",out(reg) active,options(nomem,nostack));
    }
    assert_eq!(active & ADDRESS, root);
    for guard in guards {
        assert!(!present(root, guard));
        assert!(present(old, guard));
    }
    serial!(
        r#"{{"protocol":"eutherus.paging","version":1,"event":"paging_ready","old_cr3":{},"cr3":{},"table_pages":{},"free_pages":{},"stack_low":{},"stack_high":{},"emergency_low":{},"emergency_high":{},"old_guards_still_present":true,"all_tables_owned":true,"neighbor_mappings_preserved":true}}"#,
        old,
        root,
        copy.count,
        frames.free_pages(),
        guards[0],
        guards[1],
        guards[2],
        guards[3]
    );
    // The real PIT must continue delivering interrupts after the CR3 switch.
    let ticks = interrupts::ticks();
    interrupts::wait_ticks(3);
    assert!(interrupts::ticks().wrapping_sub(ticks) >= 3);
    serial!(
        r#"{{"protocol":"eutherus.paging","version":1,"event":"timer_after_switch","ticks":{}}}"#,
        interrupts::ticks()
    );
    let fault = if cfg!(feature = "fault-guard-low") {
        Some(guards[0])
    } else if cfg!(feature = "fault-guard-high") {
        Some(guards[1])
    } else if cfg!(feature = "fault-emergency-low") {
        Some(guards[2])
    } else if cfg!(feature = "fault-emergency-high") {
        Some(guards[3])
    } else {
        None
    };
    if let Some(address) = fault {
        unsafe {
            write_volatile(address as *mut u64, 0xfeed);
        }
        panic!("guard write did not fault");
    }
    if cfg!(feature = "fault-stack-overflow") {
        // Force a push over the lower boundary. #PF delivery cannot use this RSP;
        // hardware escalates to #DF, whose IST1 stack remains usable.
        unsafe {
            asm!("cli", "mov rsp, {edge}", "push rax", "ud2",edge=in(reg) guards[0]+4096, options(noreturn));
        }
    }
}
