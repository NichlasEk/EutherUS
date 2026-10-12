#![no_std]
#![no_main]
#![cfg_attr(feature = "interrupts", feature(abi_x86_interrupt))]

use core::{
    arch::asm,
    fmt::{self, Write},
    panic::PanicInfo,
};
use uefi::{boot, mem::memory_map::MemoryMap, prelude::*};

struct Serial;
unsafe fn out(port: u16, byte: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") byte, options(nomem, nostack, preserves_flags))
    };
}
fn serial_init() {
    // Single CPU, firmware-selected x86 PC with a 16550 UART at COM1.
    unsafe {
        out(0x3f9, 0);
        out(0x3fb, 0x80);
        out(0x3f8, 1);
        out(0x3f9, 0);
        out(0x3fb, 3);
        out(0x3fa, 0xc7);
        out(0x3fc, 0x0b);
    }
}
impl Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            // Bounded wait: absent/broken UART must never deadlock a panic.
            for _ in 0..100_000 {
                let ready: u8;
                unsafe {
                    asm!("in al, dx", in("dx") 0x3fdu16, out("al") ready, options(nomem, nostack, preserves_flags))
                };
                if ready & 0x20 != 0 {
                    break;
                }
                core::hint::spin_loop();
            }
            unsafe { out(0x3f8, byte) };
        }
        Ok(())
    }
}
macro_rules! serial { ($($arg:tt)*) => { let _ = writeln!(Serial, $($arg)*); }; }

#[cfg(feature = "graphics")]
mod graphics;
#[cfg(feature = "interrupts")]
mod interrupts;
#[cfg(feature = "memory")]
mod memory;
#[cfg(feature = "paging")]
mod paging;

#[cfg(feature = "memory")]
/// Framebuffer portion of the internal handoff; v3 allocates its arena after exit.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Framebuffer {
    pub address: u64,
    pub bytes: u64,
    pub back_buffer: u64,
    pub back_buffer_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32, // 0: RGB bytes, 1: BGR bytes. Other formats rejected.
}

/// Internal handoff: v1 serial baseline, v3 memory/framebuffer ownership.
/// The UEFI entry and kernel share one PE image.
/// All addresses are identity-mapped; memory remains reserved for this image.
#[repr(C)]
struct BootInfo {
    magic: [u8; 8],
    version: u32,
    size: u32,
    memory_map: u64,
    memory_map_bytes: u64,
    descriptor_size: u64,
    descriptor_version: u32,
    flags: u32,
    #[cfg(feature = "memory")]
    framebuffer: Framebuffer,
    #[cfg(feature = "memory")]
    image_base: u64,
    #[cfg(feature = "memory")]
    image_bytes: u64,
}

const _: () =
    assert!(core::mem::size_of::<BootInfo>() == if cfg!(feature = "memory") { 112 } else { 48 });

#[entry]
fn main() -> Status {
    serial_init();
    serial!(r#"{{"protocol":"eutherus.boot","version":1,"event":"uefi_entry"}}"#);
    boot::set_watchdog_timer(0, 0, None).expect("watchdog disable");
    #[cfg(feature = "graphics")]
    let framebuffer = graphics::prepare();
    #[cfg(all(feature = "memory", not(feature = "graphics")))]
    let framebuffer = Framebuffer::default();
    #[cfg(feature = "memory")]
    let (image_base, image_bytes) = {
        let image = boot::open_protocol_exclusive::<uefi::proto::loaded_image::LoadedImage>(
            boot::image_handle(),
        )
        .expect("loaded image");
        image.info()
    };
    // No firmware protocol borrows or asynchronous callbacks remain. The
    // returned map owns firmware-allocated LOADER_DATA. It is never dropped.
    let map = unsafe { boot::exit_boot_services(None) };
    unsafe { asm!("cli", options(nomem, nostack)) };
    let meta = map.meta();
    let info = BootInfo {
        magic: *b"EUTHERUS",
        version: if cfg!(feature = "memory") { 3 } else { 1 },
        size: core::mem::size_of::<BootInfo>() as u32,
        memory_map: map.buffer().as_ptr() as u64,
        memory_map_bytes: map.buffer().len() as u64,
        descriptor_size: meta.desc_size as u64,
        descriptor_version: meta.desc_version,
        flags: u32::from(cfg!(feature = "graphics")),
        #[cfg(feature = "memory")]
        framebuffer,
        #[cfg(feature = "memory")]
        image_base: image_base as u64,
        #[cfg(feature = "memory")]
        image_bytes,
    };
    core::mem::forget(map);
    #[cfg(feature = "interrupts")]
    unsafe {
        interrupts::enter(&info)
    }
    #[cfg(not(feature = "interrupts"))]
    kernel_main(&info)
}

#[unsafe(no_mangle)]
#[inline(never)]
extern "C" fn kernel_main(info: &BootInfo) -> ! {
    assert!(
        info.magic == *b"EUTHERUS" && info.version == if cfg!(feature = "memory") { 3 } else { 1 }
    );
    assert_eq!(info.size as usize, core::mem::size_of::<BootInfo>());
    assert_eq!(info.flags, u32::from(cfg!(feature = "graphics")));
    serial!("EutherUS 0.0.1 — System Regis kernel initialized");
    serial!(r#"{{"protocol":"eutherus.boot","version":1,"event":"boot_services_exited"}}"#);
    assert!(info.memory_map_bytes > 0 && info.descriptor_size >= 40);
    serial!(
        r#"{{"protocol":"eutherus.boot","version":1,"event":"memory_map","bytes":{},"descriptor_size":{}}}"#,
        info.memory_map_bytes,
        info.descriptor_size
    );
    #[cfg(feature = "interrupts")]
    {
        interrupts::init();
        interrupts::inject_faults();
    }
    #[cfg(feature = "memory")]
    let mut frames = memory::initialize(info);
    #[cfg(feature = "memory")]
    memory::self_test(&mut frames);
    #[cfg(feature = "paging")]
    paging::install(&mut frames);
    if cfg!(feature = "fault-panic") {
        panic!("injected panic");
    }
    if cfg!(feature = "fault-timeout") {
        halt();
    }
    if cfg!(feature = "fault-crash") {
        // Deliberate triple fault: zero-length IDT, then software interrupt.
        let idtr = [0u8; 10];
        unsafe { asm!("lidt [{}]", "int3", in(reg) idtr.as_ptr(), options(nostack)) };
    }
    #[cfg(feature = "graphics")]
    let framebuffer = {
        let mut fb = info.framebuffer;
        memory::graphics_buffer(&mut frames, &mut fb);
        fb
    };
    serial!(
        r#"{{"protocol":"eutherus.boot","version":1,"event":"boot_success","kernel":"0.0.1","handoff":{}}}"#,
        info.version
    );
    #[cfg(feature = "graphics")]
    graphics::run(&framebuffer, frames.free_pages());
    #[cfg(all(feature = "interrupts", not(feature = "graphics")))]
    interrupts::idle();
    #[cfg(not(feature = "interrupts"))]
    halt()
}
fn halt() -> ! {
    loop {
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe { asm!("cli", options(nostack)) };
    serial!("EutherUS panic detail: {}", info);
    serial!(
        r#"{{"protocol":"eutherus.boot","version":1,"event":"panic","reason":"kernel_panic"}}"#
    );
    halt()
}
