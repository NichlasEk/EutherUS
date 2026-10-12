//! Conservative physical allocation over retained firmware mappings. No reclamation.
use crate::{BootInfo, Serial};
use core::{
    arch::asm,
    fmt::Write,
    ptr::{addr_of_mut, read_volatile},
    sync::atomic::{AtomicBool, Ordering},
};
use regis_memory::{Error, Frames, Map, PAGE};
const LIMIT: usize = 1024 * 1024 * 1024;
static mut STATES: [u8; LIMIT / 4096] = [0; LIMIT / 4096];
static INITIALIZED: AtomicBool = AtomicBool::new(false);
const ADDRESS: u64 = 0x000f_ffff_ffff_f000;
struct Tables {
    root: u64,
    seen: [u64; 4096],
    count: usize,
}
impl Tables {
    fn discover(&mut self, address: u64, level: u8, map: &Map<'_>, frames: &mut Frames<'_>) {
        if self.seen[..self.count].contains(&address) {
            return;
        }
        assert!(self.count < self.seen.len(), "page table audit capacity");
        let region = map
            .containing(address, PAGE)
            .expect("page table in RAM map");
        assert!((1..=7).contains(&region.kind), "page table must be in RAM");
        self.seen[self.count] = address;
        self.count += 1;
        frames.reserve(address, PAGE).expect("reserve page table");
        if level == 1 {
            return;
        }
        for i in 0..512 {
            // Q35 bootstrap contract: firmware page-table frames are identity mapped.
            let entry = unsafe { read_volatile((address as *const u64).add(i)) };
            if entry & 1 == 0 {
                continue;
            }
            if entry & (1 << 7) != 0 {
                assert!(level == 2 || level == 3);
                continue;
            }
            self.discover(entry & ADDRESS, level - 1, map, frames);
        }
    }
    fn identity(&self, address: u64, writable: bool) -> bool {
        let mut table = self.root;
        for level in (1..=4).rev() {
            let shift = 12 + (level - 1) * 9;
            let entry = unsafe {
                read_volatile((table as *const u64).add(((address >> shift) & 511) as usize))
            };
            let required = if writable { 3 } else { 1 };
            if entry & required != required {
                return false;
            }
            if level == 1 || (level <= 3 && entry & (1 << 7) != 0) {
                let mask = (1u64 << shift) - 1;
                return (entry & ADDRESS & !mask) | (address & mask) == address;
            }
            table = entry & ADDRESS;
            if !self.seen[..self.count].contains(&table) {
                return false;
            }
        }
        false
    }
}
fn reserve(frames: &mut Frames<'_>, name: &str, start: u64, bytes: u64) {
    frames.reserve(start, bytes).expect("reservation");
    serial!(
        r#"{{"protocol":"eutherus.memory","version":1,"event":"reserved","name":"{}","start":{},"bytes":{}}}"#,
        name,
        start,
        bytes
    );
}
#[inline(never)]
pub fn initialize(info: &BootInfo) -> Frames<'static> {
    assert!(!INITIALIZED.swap(true, Ordering::Relaxed));
    assert!(info.memory_map != 0 && info.memory_map_bytes <= 1024 * 1024);
    let raw = unsafe {
        core::slice::from_raw_parts(info.memory_map as *const u8, info.memory_map_bytes as usize)
    };
    let version = if cfg!(feature = "fault-memory-invalid") {
        0
    } else {
        info.descriptor_version
    };
    let map = Map::parse(raw, info.descriptor_size as usize, version).unwrap_or_else(|error| {
        serial!(r#"{{"protocol":"eutherus.memory","version":1,"event":"map_rejected"}}"#);
        panic!("invalid memory map: {:?}", error);
    });
    let state =
        unsafe { core::slice::from_raw_parts_mut(addr_of_mut!(STATES).cast::<u8>(), LIMIT / 4096) };
    let mut frames = Frames::from_map(state, &map);
    reserve(&mut frames, "low_memory", 0, 1024 * 1024);
    assert!(info.image_base > 0 && info.image_bytes > 0);
    reserve(
        &mut frames,
        "kernel_image_stacks_tables",
        info.image_base,
        info.image_bytes,
    );
    let old_stack = map
        .containing(
            info as *const _ as u64,
            core::mem::size_of::<BootInfo>() as u64,
        )
        .expect("retained BootInfo stack");
    reserve(
        &mut frames,
        "firmware_stack",
        old_stack.start,
        old_stack.end - old_stack.start,
    );
    let map_region = map
        .containing(info.memory_map, info.memory_map_bytes)
        .expect("retained map allocation");
    reserve(
        &mut frames,
        "memory_map_allocation",
        map_region.start,
        map_region.end - map_region.start,
    );
    if info.framebuffer.address != 0 {
        reserve(
            &mut frames,
            "framebuffer",
            info.framebuffer.address,
            info.framebuffer.bytes,
        );
    }
    let cr3: u64;
    let cr4: u64;
    unsafe {
        asm!("mov {}, cr3",out(reg) cr3,options(nomem,nostack));
        asm!("mov {}, cr4",out(reg) cr4,options(nomem,nostack));
    }
    assert!(cr4 & (1 << 12) == 0, "four-level paging required");
    let mut tables = Tables {
        root: cr3 & ADDRESS,
        seen: [0; 4096],
        count: 0,
    };
    tables.discover(tables.root, 4, &map, &mut frames);
    let mut readonly_tables = 0;
    for &table in &tables.seen[..tables.count] {
        if !tables.identity(table, true) {
            readonly_tables += 1;
        }
        assert!(tables.identity(table, false));
        assert!(frames.is_reserved(table));
    }
    // Only expose pages that are already identity-mapped and writable at every level.
    let mut excluded = 0;
    for index in 0..frames.capacity() {
        let address = index as u64 * PAGE;
        if !frames.is_reserved(address) && !tables.identity(address, true) {
            frames.reserve(address, PAGE).unwrap();
            excluded += 1;
        }
    }
    if cfg!(feature = "fault-memory-empty") {
        frames.reserve(0, LIMIT as u64).unwrap();
    }
    serial!(
        r#"{{"protocol":"eutherus.memory","version":1,"event":"allocator_ready","page_bytes":4096,"limit_bytes":{},"free_pages":{},"page_table_frames":{},"readonly_page_table_frames":{},"unmapped_or_readonly_pages":{},"reclaimed_firmware_pages":0}}"#,
        LIMIT,
        frames.free_pages(),
        tables.count,
        readonly_tables,
        excluded
    );
    frames
}
#[inline(never)]
pub fn self_test(frames: &mut Frames<'_>) {
    let initial = frames.free_pages();
    if cfg!(feature = "fault-memory-empty") {
        assert_eq!(initial, 0);
        assert_eq!(frames.allocate(1), Err(Error::Exhausted));
        serial!(
            r#"{{"protocol":"eutherus.memory","version":1,"event":"out_of_memory","free_pages":0,"requested_pages":1}}"#
        );
        crate::halt();
    }
    assert!(initial > 16);
    let first = frames.allocate(3).expect("first allocation");
    let second = frames.allocate(2).expect("second allocation");
    assert!(first + 3 * PAGE <= second || second + 2 * PAGE <= first);
    for (address, pages, pattern) in [
        (first, 3, 0xa5a5_1234_5678_9876u64),
        (second, 2, 0x5a5a_fedc_ba98_7654),
    ] {
        for i in 0..pages * 512 {
            unsafe {
                (address as *mut u64)
                    .add(i)
                    .write_volatile(pattern ^ (i as u64));
            }
        }
    }
    for (address, pages, pattern) in [
        (first, 3, 0xa5a5_1234_5678_9876u64),
        (second, 2, 0x5a5a_fedc_ba98_7654),
    ] {
        for i in 0..pages * 512 {
            assert_eq!(
                unsafe { (address as *const u64).add(i).read_volatile() },
                pattern ^ (i as u64)
            );
        }
    }
    let before = frames.free_pages();
    assert_eq!(frames.release(first + PAGE, 2), Err(Error::InvalidFree));
    assert_eq!(frames.release(first, 2), Err(Error::InvalidFree));
    assert_eq!(frames.reserve(first, PAGE), Err(Error::Busy));
    assert_eq!(frames.free_pages(), before);
    frames.release(first, 3).unwrap();
    assert_eq!(frames.release(first, 3), Err(Error::InvalidFree));
    frames.release(second, 2).unwrap();
    assert_eq!(frames.free_pages(), initial);
    // Exhaust actual eligible frame bookkeeping; writes were tested separately above.
    let mut allocated = 0;
    loop {
        match frames.allocate(1) {
            Ok(p) => {
                assert!(p >= 1024 * 1024 && p < LIMIT as u64);
                allocated += 1;
            }
            Err(Error::Exhausted) => break,
            Err(e) => panic!("unexpected allocation error: {:?}", e),
        }
    }
    assert_eq!(allocated, initial);
    assert_eq!(frames.free_pages(), 0);
    let mut released = 0;
    for i in 0..frames.capacity() {
        if frames.release(i as u64 * PAGE, 1).is_ok() {
            released += 1;
        }
    }
    assert_eq!(released, initial);
    assert_eq!(frames.free_pages(), initial);
    serial!(
        r#"{{"protocol":"eutherus.memory","version":1,"event":"self_test_pass","pattern_pages":5,"invalid_free_rejected":true,"exhausted_pages":{},"restored_pages":{}}}"#,
        allocated,
        frames.free_pages()
    );
}
#[cfg(feature = "graphics")]
pub fn graphics_buffer(frames: &mut Frames<'_>, fb: &mut crate::Framebuffer) {
    let bytes = fb.width as u64 * fb.height as u64 * 8;
    let pages = bytes.div_ceil(PAGE) as usize;
    let address = frames.allocate(pages).unwrap_or_else(|error| {
        serial!(r#"{{"protocol":"eutherus.memory","version":1,"event":"out_of_memory","free_pages":{},"requested_pages":{}}}"#,frames.free_pages(),pages);
        panic!("graphics arena allocation: {:?}", error);
    });
    fb.back_buffer = address;
    fb.back_buffer_bytes = pages as u64 * PAGE;
    serial!(
        r#"{{"protocol":"eutherus.memory","version":1,"event":"graphics_allocated","address":{},"pages":{},"free_pages":{}}}"#,
        address,
        pages,
        frames.free_pages()
    );
}
