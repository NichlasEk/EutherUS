//! Bounded, single-CPU cooperative experiment. IRQ handlers never schedule.
//! Each context is a suspended SysV64 call, not an interrupt register frame.
use crate::{Serial, interrupts};
use core::{
    arch::{asm, naked_asm},
    fmt::Write,
    ptr::{addr_of, addr_of_mut},
};

const STACK_BYTES: usize = 128 * 1024;
#[repr(C, align(4096))]
#[derive(Clone, Copy)]
struct Stack {
    low: [u8; 4096],
    bytes: [u8; STACK_BYTES],
    high: [u8; 4096],
}
static mut STACKS: [Stack; 2] = [Stack {
    low: [0; 4096],
    bytes: [0; STACK_BYTES],
    high: [0; 4096],
}; 2];
// Only the scheduler and currently executing task access these. No Rust borrow
// of shared context/state storage may live across switch(). No SMP/reentrancy.
static mut CONTEXTS: [u64; 3] = [0; 3];
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Running,
    Finished,
    #[cfg_attr(not(feature = "graphics"), allow(dead_code))]
    Sleeping(u64),
}
static mut STATES: [State; 2] = [State::Ready; 2];
static mut CURRENT: usize = 2;
static mut STARTED: bool = false;
#[derive(Clone, Copy)]
pub struct Task {
    pub name: &'static str,
    pub body: fn(),
}
static mut ENTRIES: [Option<Task>; 2] = [None; 2];
#[derive(Clone, Copy, Default)]
pub struct Activity {
    pub dispatches: u64,
    pub yields: u64,
    pub sleeps: u64,
    #[cfg_attr(not(feature = "graphics"), allow(dead_code))]
    pub work: u64,
}
static mut ACTIVITY: [Activity; 2] = [Activity {
    dispatches: 0,
    yields: 0,
    sleeps: 0,
    work: 0,
}; 2];

/// Read-only, by-value view; no mutable scheduler references escape.
#[cfg(feature = "graphics")]
#[derive(Clone, Copy)]
pub struct Snapshot {
    pub id: usize,
    pub name: &'static str,
    pub state: &'static str,
    pub wait: &'static str,
    pub deadline: Option<u64>,
    pub tick: u64,
    pub stack_base: u64,
    pub stack_end: u64,
    pub guard_low: u64,
    pub guard_high: u64,
    pub activity: Activity,
}
/// Called only from a kernel task on this single CPU. IRQs update ticks alone;
/// there is no yield inside the copy and no reference to scheduler storage escapes.
#[cfg(feature = "graphics")]
pub fn inspect(id: usize) -> Option<Snapshot> {
    if id >= 2 {
        return None;
    }
    unsafe {
        let task = ENTRIES[id]?;
        let tick = interrupts::ticks();
        let (state, wait, deadline) = match STATES[id] {
            State::Ready => ("ready", "dispatch", None),
            State::Running => ("running", "none", None),
            State::Sleeping(deadline) => ("sleeping", "timer", Some(deadline)),
            State::Finished => ("finished", "none", None),
        };
        let stack_base = base(id);
        Some(Snapshot {
            id,
            name: task.name,
            state,
            wait,
            deadline,
            tick,
            stack_base,
            stack_end: stack_base + STACK_BYTES as u64,
            guard_low: stack_base - 4096,
            guard_high: stack_base + STACK_BYTES as u64,
            activity: ACTIVITY[id],
        })
    }
}

/// Copy snapshots only: no borrow of shared scheduler data crosses a yield.
#[cfg(feature = "graphics")]
pub fn activity() -> [Activity; 2] {
    unsafe { ACTIVITY }
}
#[cfg(feature = "graphics")]
pub fn work_done() {
    unsafe {
        let id = CURRENT;
        assert!(id < 2 && STATES[id] == State::Running);
        ACTIVITY[id].work += 1;
    }
}
#[cfg(feature = "graphics")]
pub fn stack_pointer() -> u64 {
    let rsp: u64;
    unsafe {
        asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack));
    }
    rsp
}
/// Timed cooperative wait. IRQ only advances the clock; scheduler owns wakeup.
#[cfg(feature = "graphics")]
pub fn sleep(ticks: u32) {
    assert!(ticks > 0);
    unsafe {
        let id = CURRENT;
        assert!(id < 2 && STATES[id] == State::Running);
        STATES[id] = State::Sleeping(interrupts::ticks().wrapping_add(ticks as u64));
        ACTIVITY[id].yields += 1;
        ACTIVITY[id].sleeps += 1;
        transfer(id, 2);
        assert!(CURRENT == id && STATES[id] == State::Running);
    }
}

fn base(id: usize) -> u64 {
    assert!(id < 2);
    unsafe { addr_of!(STACKS).cast::<Stack>().add(id) as u64 + 4096 }
}
pub fn guards() -> [u64; 4] {
    [
        base(0) - 4096,
        base(0) + STACK_BYTES as u64,
        base(1) - 4096,
        base(1) + STACK_BYTES as u64,
    ]
}

/// Explicit SysV ABI even on the UEFI/Win64 target. Rust callers handle the ABI
/// boundary (including Win64's nonvolatile XMM registers). Save SysV nonvolatile
/// GPRs, FP control state, RFLAGS and RSP; caller-saved values belong to callers.
/// Layout at saved RSP: MXCSR:u32, FCW:u16, pad:u16, r15,r14,r13,r12,rbx,rbp,
/// rflags, return RIP. Disable IRQs only while changing the active stack.
#[unsafe(naked)]
unsafe extern "sysv64" fn switch(_save: *mut u64, _next: *const u64) {
    naked_asm!(
        "pushfq",
        "cli",
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "sub rsp, 8",
        "stmxcsr [rsp]",
        "fnstcw [rsp + 4]",
        "mov [rdi], rsp",
        "mov rsp, [rsi]",
        "ldmxcsr [rsp]",
        "fldcw [rsp + 4]",
        "add rsp, 8",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "popfq",
        "ret"
    );
}
unsafe fn transfer(from: usize, to: usize) {
    unsafe {
        let contexts = addr_of_mut!(CONTEXTS).cast::<u64>();
        switch(contexts.add(from), contexts.add(to));
    }
}
#[cfg(not(feature = "graphics"))]
extern "sysv64" fn yield_now() {
    unsafe {
        let id = CURRENT;
        assert!(
            id < 2 && STATES[id] == State::Running,
            "yield outside running task"
        );
        STATES[id] = State::Ready;
        ACTIVITY[id].yields += 1;
        transfer(id, 2);
        assert!(CURRENT == id && STATES[id] == State::Running);
    }
}

/// Independent assembly probe keeps distinct sentinels in ALL six nonvolatile
/// GPRs over a real yield. The compiler cannot optimize away these checks.
#[cfg(not(feature = "graphics"))]
#[unsafe(naked)]
unsafe extern "sysv64" fn register_probe(_id: u64) -> u64 {
    naked_asm!(
        "push rbp", "push rbx", "push r12", "push r13", "push r14", "push r15",
        "sub rsp, 8", "mov [rsp], rdi",
        "lea rbp, [rdi + 0x101]", "lea rbx, [rdi + 0x202]",
        "lea r12, [rdi + 0x303]", "lea r13, [rdi + 0x404]",
        "lea r14, [rdi + 0x505]", "lea r15, [rdi + 0x606]",
        "call {yield_now}",
        "mov rdx, [rsp]", "lea rax, [rdx + 0x101]", "cmp rbp, rax", "jne 2f",
        "lea rax, [rdx + 0x202]", "cmp rbx, rax", "jne 2f",
        "lea rax, [rdx + 0x303]", "cmp r12, rax", "jne 2f",
        "lea rax, [rdx + 0x404]", "cmp r13, rax", "jne 2f",
        "lea rax, [rdx + 0x505]", "cmp r14, rax", "jne 2f",
        "lea rax, [rdx + 0x606]", "cmp r15, rax", "jne 2f",
        "mov eax, 1", "jmp 3f", "2:", "xor eax, eax", "3:",
        "add rsp, 8", "pop r15", "pop r14", "pop r13", "pop r12", "pop rbx", "pop rbp", "ret",
        yield_now = sym yield_now
    );
}
fn controls() -> (u32, u16) {
    let mut mxcsr = 0u32;
    let mut fcw = 0u16;
    unsafe {
        asm!("stmxcsr [{}]", "fnstcw [{}]", in(reg) &mut mxcsr, in(reg) &mut fcw, options(nostack));
    }
    (mxcsr, fcw)
}
fn flags() -> u64 {
    let value: u64;
    unsafe {
        asm!("pushfq", "pop {}", out(reg) value);
    }
    value
}
extern "sysv64" fn entry() -> ! {
    let id = unsafe { CURRENT };
    let body = unsafe { ENTRIES[id].expect("task entry").body };
    body();
    unsafe {
        assert!(STATES[id] == State::Running);
        STATES[id] = State::Finished;
        serial!(
            r#"{{"protocol":"eutherus.tasks","version":1,"event":"task_finished","id":{}}}"#,
            id
        );
        transfer(id, 2);
    }
    panic!("finished task resumed")
}
#[cfg(not(feature = "graphics"))]
fn task_body(id: usize) {
    let mut local = [0u64; 32];
    let ptr = local.as_mut_ptr();
    let stack = base(id);
    assert!((stack..stack + STACK_BYTES as u64).contains(&(ptr as u64)));
    assert!(ptr as u64 + core::mem::size_of_val(&local) as u64 <= stack + STACK_BYTES as u64);
    let (initial_mxcsr, initial_fcw) = controls();
    let mxcsr = (initial_mxcsr & !(3 << 13)) | ((id as u32 + 1) << 13);
    let fcw = (initial_fcw & !(3 << 10)) | ((id as u16 + 1) << 10);
    unsafe {
        asm!("ldmxcsr [{}]", "fldcw [{}]", in(reg) &mxcsr, in(reg) &fcw, options(nostack));
    }
    // Unequal lifetimes prove a finished task is skipped while its peer advances.
    let rounds = if id == 0 { 8 } else { 5 };
    for round in 0..rounds {
        for i in 0..32 {
            unsafe {
                ptr.add(i)
                    .write_volatile(((id as u64 + 1) << 48) | (round << 16) | i as u64);
            }
        }
        // Exercise per-context IF restoration as well as enabled timer delivery.
        if id == 1 {
            unsafe {
                asm!("cli", options(nostack));
            }
        }
        let expected_if = flags() & 0x200;
        assert_eq!(
            unsafe { register_probe(id as u64) },
            1,
            "callee-saved GPR corruption"
        );
        assert_eq!(flags() & 0x200, expected_if, "interrupt flag corruption");
        assert_eq!(controls(), (mxcsr, fcw), "FP control corruption");
        for i in 0..32 {
            assert_eq!(
                unsafe { ptr.add(i).read_volatile() },
                ((id as u64 + 1) << 48) | (round << 16) | i as u64
            );
        }
        interrupts::wait_ticks(1);
        serial!(
            r#"{{"protocol":"eutherus.tasks","version":1,"event":"task_progress","id":{},"round":{},"stack_address":{},"registers_preserved":true,"stack_preserved":true,"controls_preserved":true,"if_preserved":true,"ticks":{}}}"#,
            id,
            round + 1,
            ptr as u64,
            interrupts::ticks()
        );
        if round == 1 {
            let low = (id == 0 && cfg!(feature = "fault-task0-low"))
                || (id == 1 && cfg!(feature = "fault-task1-low"));
            let high = (id == 0 && cfg!(feature = "fault-task0-high"))
                || (id == 1 && cfg!(feature = "fault-task1-high"));
            if low || high {
                let address = if low {
                    stack - 4096
                } else {
                    stack + STACK_BYTES as u64
                };
                unsafe {
                    (address as *mut u64).write_volatile(0xfeed);
                }
                panic!("task guard did not fault");
            }
            if id == 1 && cfg!(feature = "fault-task-overflow") {
                unsafe {
                    asm!("cli", "mov rsp, {edge}", "push rax", "ud2", edge=in(reg) stack, options(noreturn));
                }
            }
        }
    }
}

#[cfg(not(feature = "graphics"))]
pub fn run() {
    let before = interrupts::ticks();
    let dispatches = run_with([
        Task {
            name: "probe0",
            body: || task_body(0),
        },
        Task {
            name: "probe1",
            body: || task_body(1),
        },
    ]);
    assert_eq!(dispatches, 15);
    assert!(interrupts::ticks().wrapping_sub(before) >= 13);
    serial!(
        r#"{{"protocol":"eutherus.tasks","version":1,"event":"tasks_complete","finished":2,"dispatches":{},"ticks":{}}}"#,
        dispatches,
        interrupts::ticks()
    );
}

pub fn run_with(entries: [Task; 2]) -> u64 {
    unsafe {
        assert!(!STARTED, "scheduler initialized once");
        STARTED = true;
        ENTRIES = entries.map(Some);
    }
    let initial_controls = controls();
    let initial_if = flags() & 0x200;
    for id in 0..2 {
        let stack = base(id);
        let top = stack + STACK_BYTES as u64;
        // RET leaves RSP=top-8, satisfying SysV entry alignment. Entry never returns.
        let saved = top - 80;
        unsafe {
            core::ptr::write_bytes(saved as *mut u8, 0, 80);
            (saved as *mut u32).write(initial_controls.0);
            ((saved + 4) as *mut u16).write(initial_controls.1);
            ((saved + 56) as *mut u64).write(0x202);
            ((saved + 64) as *mut u64).write(entry as *const () as u64);
            CONTEXTS[id] = saved;
        }
        serial!(
            r#"{{"protocol":"eutherus.tasks","version":1,"event":"task_ready","id":{},"name":"{}","stack_base":{},"stack_bytes":{},"guard_low":{},"guard_high":{}}}"#,
            id,
            entries[id].name,
            stack,
            STACK_BYTES,
            stack - 4096,
            top
        );
    }
    serial!(
        r#"{{"protocol":"eutherus.tasks","version":1,"event":"scheduler_ready","tasks":2,"policy":"cooperative_round_robin"}}"#
    );
    let mut dispatches = 0;
    let mut idle_reported = false;
    loop {
        let mut runnable = false;
        for id in 0..2 {
            unsafe {
                if STATES[id] == State::Finished {
                    continue;
                }
                if let State::Sleeping(deadline) = STATES[id] {
                    let now = interrupts::ticks();
                    if (now.wrapping_sub(deadline) as i64) < 0 {
                        continue;
                    }
                    STATES[id] = State::Ready;
                    if ACTIVITY[id].sleeps == 1 {
                        serial!(
                            r#"{{"protocol":"eutherus.tasks","version":1,"event":"task_woke","id":{},"deadline":{},"ticks":{}}}"#,
                            id,
                            deadline,
                            now
                        );
                    }
                }
                assert!(STATES[id] == State::Ready);
                runnable = true;
                STATES[id] = State::Running;
                CURRENT = id;
                ACTIVITY[id].dispatches += 1;
                transfer(2, id);
                CURRENT = 2;
                assert!(STATES[id] != State::Running);
                #[cfg(feature = "graphics")]
                assert!(
                    STATES[id] != State::Finished,
                    "essential graphics task exited"
                );
            }
            dispatches += 1;
            assert_eq!(controls(), initial_controls, "scheduler FP state corrupted");
            assert_eq!(flags() & 0x200, initial_if, "scheduler IF corrupted");
        }
        if !runnable {
            if unsafe { STATES[0] == State::Finished && STATES[1] == State::Finished } {
                break;
            }
            if !idle_reported {
                serial!(
                    r#"{{"protocol":"eutherus.tasks","version":1,"event":"scheduler_idle","ticks":{}}}"#,
                    interrupts::ticks()
                );
                idle_reported = true;
            }
            // All remaining tasks are sleeping. HLT on the scheduler's own stack.
            interrupts::wait_ticks(1);
        }
    }
    dispatches
}
