//! Single-core Q35 bootstrap: owned stack, GDT/TSS/IDT and legacy PIT/PIC.
//! Firmware page tables remain retained. No allocation or scheduling occurs here.
use crate::{BootInfo, Serial, out};
use core::{
    arch::asm,
    fmt::Write,
    ptr::{addr_of, addr_of_mut},
    sync::atomic::{AtomicU64, Ordering},
};

pub const STACK_BYTES: u64 = 128 * 1024;
#[repr(C, align(4096))]
struct Stack {
    lower: [u8; 4096],
    usable: [u8; 128 * 1024],
    upper: [u8; 4096],
}
const EMPTY_STACK: Stack = Stack {
    lower: [0; 4096],
    usable: [0; 128 * 1024],
    upper: [0; 4096],
};
static mut STACK: Stack = EMPTY_STACK;
static mut EMERGENCY: Stack = EMPTY_STACK;
fn stack_base() -> u64 {
    unsafe { addr_of!(STACK.usable) as u64 }
}
fn emergency_base() -> u64 {
    unsafe { addr_of!(EMERGENCY.usable) as u64 }
}
#[cfg(feature = "paging")]
pub fn guards() -> [u64; 4] {
    [
        stack_base() - 4096,
        stack_base() + STACK_BYTES,
        emergency_base() - 4096,
        emergency_base() + STACK_BYTES,
    ]
}

/// Win64 entry: RCX carries BootInfo, 32-byte shadow space and dummy return slot.
/// BootInfo remains on the retained firmware stack; this function never returns.
pub unsafe fn enter(info: &BootInfo) -> ! {
    let top = stack_base() + STACK_BYTES;
    unsafe {
        asm!("mov rsp, {top}", "sub rsp, 40", "xor rbp, rbp", "jmp {entry}",
        top=in(reg) top, entry=sym crate::kernel_main, in("rcx") info, options(noreturn));
    }
}
#[repr(C, packed)]
struct TablePointer {
    limit: u16,
    base: u64,
}
#[repr(C, packed)]
struct Tss {
    reserved: u32,
    rsp: [u64; 3],
    reserved2: u64,
    ist: [u64; 7],
    reserved3: u64,
    reserved4: u16,
    iomap: u16,
}
static mut TSS: Tss = Tss {
    reserved: 0,
    rsp: [0; 3],
    reserved2: 0,
    ist: [0; 7],
    reserved3: 0,
    reserved4: 0,
    iomap: 104,
};
static mut GDT: [u64; 5] = [0, 0x00af_9a00_0000_ffff, 0x00cf_9200_0000_ffff, 0, 0];
#[repr(C)]
#[derive(Clone, Copy)]
struct Gate {
    low: u16,
    selector: u16,
    ist: u8,
    flags: u8,
    mid: u16,
    high: u32,
    reserved: u32,
}
impl Gate {
    const EMPTY: Self = Self {
        low: 0,
        selector: 0,
        ist: 0,
        flags: 0,
        mid: 0,
        high: 0,
        reserved: 0,
    };
    fn new(handler: u64, ist: u8) -> Self {
        Self {
            low: handler as u16,
            selector: 8,
            ist,
            flags: 0x8e,
            mid: (handler >> 16) as u16,
            high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}
static mut IDT: [Gate; 256] = [Gate::EMPTY; 256];
const _: () = assert!(core::mem::size_of::<Tss>() == 104 && core::mem::size_of::<Gate>() == 16);
#[repr(C)]
#[derive(Clone, Copy)]
struct Frame {
    rip: u64,
    cs: u64,
    flags: u64,
    rsp: u64,
    ss: u64,
}
static TICKS: AtomicU64 = AtomicU64::new(0);
static BREAKPOINTS: AtomicU64 = AtomicU64::new(0);
static WAITS: AtomicU64 = AtomicU64::new(0);
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}
pub fn waits() -> u64 {
    WAITS.load(Ordering::Relaxed)
}
fn exception(vector: u8, frame: Frame, error: u64) -> ! {
    let cr2: u64;
    let rsp: u64;
    unsafe {
        asm!("mov {}, cr2",out(reg) cr2,options(nomem,nostack));
        asm!("mov {}, rsp",out(reg) rsp,options(nomem,nostack));
    }
    let base = emergency_base();
    let emergency = (base..base + STACK_BYTES).contains(&rsp);
    serial!(
        r#"{{"protocol":"eutherus.cpu","version":1,"event":"exception","vector":{},"error_code":{},"rip":{},"cr2":{},"handler_rsp":{},"emergency_stack":{}}}"#,
        vector,
        error,
        frame.rip,
        cr2,
        rsp,
        emergency
    );
    crate::halt()
}
macro_rules! no_error {
    ($name:ident,$vector:expr) => {
        extern "x86-interrupt" fn $name(frame: Frame) {
            exception($vector, frame, 0)
        }
    };
}
macro_rules! error {
    ($name:ident,$vector:expr) => {
        extern "x86-interrupt" fn $name(frame: Frame, code: u64) {
            exception($vector, frame, code)
        }
    };
}
no_error!(divide, 0);
no_error!(debug, 1);
no_error!(nmi, 2);
no_error!(overflow, 4);
no_error!(bound, 5);
no_error!(invalid_opcode, 6);
no_error!(device_unavailable, 7);
error!(double_fault, 8);
no_error!(coprocessor, 9);
error!(invalid_tss, 10);
error!(segment_absent, 11);
error!(stack_fault, 12);
error!(protection, 13);
error!(page_fault, 14);
no_error!(reserved, 15);
// 255 means an unexpected/unclassified vector, not a claimed exception number.
no_error!(unexpected, 255);
no_error!(x87, 16);
error!(alignment, 17);
no_error!(machine_check, 18);
no_error!(simd, 19);
no_error!(virtualization, 20);
error!(control_protection, 21);
error!(vmm_communication, 29);
error!(security, 30);
extern "x86-interrupt" fn breakpoint(_frame: Frame) {
    BREAKPOINTS.fetch_add(1, Ordering::Relaxed);
}
extern "x86-interrupt" fn timer(_frame: Frame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    unsafe {
        out(0x20, 0x20);
    }
}
// Masked 8259 IRQs can still generate spurious IRQ7/15. Only acknowledge a real ISR bit.
extern "x86-interrupt" fn spurious_master(_frame: Frame) {
    unsafe {
        out(0x20, 0x0b);
        if input(0x20) & 0x80 != 0 {
            out(0x20, 0x20);
        }
    }
}
extern "x86-interrupt" fn spurious_slave(_frame: Frame) {
    unsafe {
        out(0xa0, 0x0b);
        if input(0xa0) & 0x80 != 0 {
            out(0xa0, 0x20);
        }
        out(0x20, 0x20);
    }
}
unsafe fn input(port: u16) -> u8 {
    let v: u8;
    unsafe {
        asm!("in al, dx",in("dx") port,out("al") v,options(nomem,nostack,preserves_flags));
    }
    v
}
unsafe fn io(port: u16, value: u8) {
    unsafe {
        out(port, value);
        out(0x80, 0);
    }
}

pub fn init() {
    unsafe {
        asm!("cli", options(nostack));
        // No references to mutable descriptor statics escape initialization.
        addr_of_mut!(TSS.ist)
            .cast::<u64>()
            .write_unaligned(emergency_base() + STACK_BYTES);
        addr_of_mut!(TSS.rsp)
            .cast::<u64>()
            .write_unaligned(stack_base() + STACK_BYTES);
        let base = addr_of!(TSS) as u64;
        let descriptor =
            103u64 | ((base & 0xff_ffff) << 16) | (0x89u64 << 40) | (((base >> 24) & 255) << 56);
        addr_of_mut!(GDT).cast::<u64>().add(3).write(descriptor);
        addr_of_mut!(GDT).cast::<u64>().add(4).write(base >> 32);
        let gdtr = TablePointer {
            limit: 39,
            base: addr_of!(GDT) as u64,
        };
        asm!("lgdt [{}]",in(reg) &gdtr,options(readonly,nostack));
        asm!("push 8", "lea rax, [rip + 2f]", "push rax", "retfq", "2:",
            "mov ax, 16", "mov ds, ax", "mov es, ax", "mov ss, ax", "xor eax, eax", "mov fs, ax", "mov gs, ax",out("rax") _);
        asm!("ltr ax",in("ax") 24u16,options(nostack));
        let handlers = [
            divide as *const () as u64,
            debug as *const () as u64,
            nmi as *const () as u64,
            breakpoint as *const () as u64,
            overflow as *const () as u64,
            bound as *const () as u64,
            invalid_opcode as *const () as u64,
            device_unavailable as *const () as u64,
            double_fault as *const () as u64,
            coprocessor as *const () as u64,
            invalid_tss as *const () as u64,
            segment_absent as *const () as u64,
            stack_fault as *const () as u64,
            protection as *const () as u64,
            page_fault as *const () as u64,
            reserved as *const () as u64,
            x87 as *const () as u64,
            alignment as *const () as u64,
            machine_check as *const () as u64,
            simd as *const () as u64,
            virtualization as *const () as u64,
            control_protection as *const () as u64,
        ];
        for vector in 0..256 {
            addr_of_mut!(IDT)
                .cast::<Gate>()
                .add(vector)
                .write(Gate::new(unexpected as *const () as u64, 0));
        }
        for (vector, handler) in handlers.iter().enumerate() {
            addr_of_mut!(IDT)
                .cast::<Gate>()
                .add(vector)
                .write(Gate::new(*handler, if vector == 8 { 1 } else { 0 }));
        }
        for (vector, handler) in [
            (29, vmm_communication as *const () as u64),
            (30, security as *const () as u64),
            (32, timer as *const () as u64),
            (39, spurious_master as *const () as u64),
            (47, spurious_slave as *const () as u64),
        ] {
            addr_of_mut!(IDT)
                .cast::<Gate>()
                .add(vector)
                .write(Gate::new(handler, 0));
        }
        let idtr = TablePointer {
            limit: 4095,
            base: addr_of!(IDT) as u64,
        };
        asm!("lidt [{}]",in(reg) &idtr,options(readonly,nostack));
        asm!("int3");
        assert_eq!(BREAKPOINTS.load(Ordering::Relaxed), 1);
        let rsp: u64;
        asm!("mov {}, rsp",out(reg) rsp,options(nomem,nostack));
        let stack_base = stack_base();
        assert!((stack_base..stack_base + STACK_BYTES).contains(&rsp));
        serial!(
            r#"{{"protocol":"eutherus.cpu","version":1,"event":"tables_ready","owned_stack":true,"breakpoint_returned":true,"idt":{},"gdt":{},"stack_base":{},"stack_bytes":{},"emergency_base":{}}}"#,
            addr_of!(IDT) as u64,
            addr_of!(GDT) as u64,
            stack_base,
            STACK_BYTES,
            emergency_base()
        );
        // Remap the legacy PIC to 32..47 and mask everything except PIT IRQ0.
        io(0x21, 0xff);
        io(0xa1, 0xff);
        io(0x20, 0x11);
        io(0xa0, 0x11);
        io(0x21, 32);
        io(0xa1, 40);
        io(0x21, 4);
        io(0xa1, 2);
        io(0x21, 1);
        io(0xa1, 1);
        io(
            0x21,
            if cfg!(feature = "fault-timer-masked") {
                0xff
            } else {
                0xfe
            },
        );
        io(0xa1, 0xff);
        // PIT channel 0, mode 2, nominal 100 Hz. Timer handler only counts + EOI.
        io(0x43, 0x34);
        io(0x40, 11932u16 as u8);
        io(0x40, (11932u16 >> 8) as u8);
        asm!("sti", options(nostack));
    }
    wait_ticks(3);
    serial!(
        r#"{{"protocol":"eutherus.cpu","version":1,"event":"timer_ready","hz_nominal":100,"ticks":{},"halt_waits":{}}}"#,
        ticks(),
        waits()
    );
}
/// Atomic check-and-sleep: IRQ cannot arrive between the condition and HLT.
/// STI's interrupt shadow covers HLT. Always returns with interrupts enabled.
pub fn wait_ticks(count: u64) {
    let start = ticks();
    unsafe {
        asm!("cli", options(nostack));
        while ticks().wrapping_sub(start) < count {
            WAITS.fetch_add(1, Ordering::Relaxed);
            asm!("sti", "hlt", "cli", options(nostack));
        }
        asm!("sti", options(nostack));
    }
}
pub fn inject_faults() {
    unsafe {
        if cfg!(feature = "fault-invalid-opcode") {
            asm!("ud2", options(noreturn));
        }
        if cfg!(feature = "fault-page-fault") {
            core::ptr::read_volatile(0x4000_0000_0000 as *const u64);
        }
        if cfg!(feature = "fault-double-fault") {
            // A #GP whose gate is absent escalates via #NP to #DF on IST1.
            asm!("cli", options(nostack));
            addr_of_mut!(IDT).cast::<Gate>().add(13).write(Gate::EMPTY);
        }
        if cfg!(feature = "fault-general-protection") || cfg!(feature = "fault-double-fault") {
            asm!("mov ds, ax",in("ax") 0xfff8u16,options(nostack));
        }
    }
}
pub fn idle() -> ! {
    loop {
        wait_ticks(100);
        serial!(
            r#"{{"protocol":"eutherus.cpu","version":1,"event":"idle","ticks":{},"halt_waits":{}}}"#,
            ticks(),
            waits()
        );
    }
}
