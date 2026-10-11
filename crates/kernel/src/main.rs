#![no_std]
#![no_main]

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

/// Internal handoff: v1 prefix, with a v2 framebuffer extension for graphics.
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
    reserved: u32,
    #[cfg(feature = "graphics")]
    framebuffer: graphics::Framebuffer,
}

const _: () =
    assert!(core::mem::size_of::<BootInfo>() == if cfg!(feature = "graphics") { 96 } else { 48 });

#[entry]
fn main() -> Status {
    serial_init();
    serial!(r#"{{"protocol":"eutherus.boot","version":1,"event":"uefi_entry"}}"#);
    boot::set_watchdog_timer(0, 0, None).expect("watchdog disable");
    #[cfg(feature = "graphics")]
    let framebuffer = graphics::prepare();
    // No firmware protocol borrows or asynchronous callbacks remain. The
    // returned map owns firmware-allocated LOADER_DATA. It is never dropped.
    let map = unsafe { boot::exit_boot_services(None) };
    unsafe { asm!("cli", options(nomem, nostack)) };
    let meta = map.meta();
    let info = BootInfo {
        magic: *b"EUTHERUS",
        version: if cfg!(feature = "graphics") { 2 } else { 1 },
        size: core::mem::size_of::<BootInfo>() as u32,
        memory_map: map.buffer().as_ptr() as u64,
        memory_map_bytes: map.buffer().len() as u64,
        descriptor_size: meta.desc_size as u64,
        descriptor_version: meta.desc_version,
        reserved: 0,
        #[cfg(feature = "graphics")]
        framebuffer,
    };
    core::mem::forget(map);
    kernel_main(&info)
}

#[unsafe(no_mangle)]
#[inline(never)]
extern "C" fn kernel_main(info: &BootInfo) -> ! {
    assert!(
        info.magic == *b"EUTHERUS"
            && info.version == if cfg!(feature = "graphics") { 2 } else { 1 }
    );
    assert_eq!(info.size as usize, core::mem::size_of::<BootInfo>());
    serial!("EutherUS 0.0.1 — System Regis kernel initialized");
    serial!(r#"{{"protocol":"eutherus.boot","version":1,"event":"boot_services_exited"}}"#);
    assert!(info.memory_map_bytes > 0 && info.descriptor_size >= 40);
    serial!(
        r#"{{"protocol":"eutherus.boot","version":1,"event":"memory_map","bytes":{},"descriptor_size":{}}}"#,
        info.memory_map_bytes,
        info.descriptor_size
    );
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
    serial!(
        r#"{{"protocol":"eutherus.boot","version":1,"event":"boot_success","kernel":"0.0.1","handoff":{}}}"#,
        info.version
    );
    #[cfg(feature = "graphics")]
    graphics::run(&info.framebuffer);
    #[cfg(not(feature = "graphics"))]
    halt()
}
fn halt() -> ! {
    loop {
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    serial!(
        r#"{{"protocol":"eutherus.boot","version":1,"event":"panic","reason":"kernel_panic"}}"#
    );
    halt()
}
