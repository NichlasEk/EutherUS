//! Bounded read-only console. Only input mutates it, renderer copies it.
//! Single CPU, no yield while a reference to STATE is alive; no heap or shell.
use crate::{Serial, tasks};
use core::{
    fmt::{self, Write},
    ptr::addr_of_mut,
};
pub const CAPACITY: usize = 64;
pub const ROWS: usize = 8;
#[derive(Clone, Copy)]
pub struct Line {
    bytes: [u8; 112],
    len: usize,
    pub accent: bool,
}
impl Line {
    const EMPTY: Self = Self {
        bytes: [0; 112],
        len: 0,
        accent: false,
    };
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap()
    }
}
impl Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len + text.len();
        if end > self.bytes.len() {
            return Err(fmt::Error);
        }
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub struct Console {
    pub active: bool,
    pub revision: u64,
    pub completed: u64,
    pub input: [u8; CAPACITY],
    pub len: usize,
    pub overflow: bool,
    pub rows: [Line; ROWS],
    pub count: usize,
    pub status: &'static str,
    free_pages: usize,
    arena_bytes: u64,
    initialized: bool,
}
static mut STATE: Console = Console {
    active: false,
    revision: 0,
    completed: 0,
    input: [0; CAPACITY],
    len: 0,
    overflow: false,
    rows: [Line::EMPTY; ROWS],
    count: 0,
    status: "ready",
    free_pages: 0,
    arena_bytes: 0,
    initialized: false,
};
impl Console {
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.input[..self.len]).unwrap()
    }
    fn push(&mut self, accent: bool, args: fmt::Arguments<'_>) {
        let mut line = Line::EMPTY;
        line.accent = accent;
        line.write_fmt(args).expect("bounded console output");
        if self.count == ROWS {
            self.rows.copy_within(1..ROWS, 0);
            self.count -= 1;
        }
        self.rows[self.count] = line;
        self.count += 1;
    }
}
pub fn snapshot() -> Console {
    unsafe { STATE }
}
fn update(f: impl FnOnce(&mut Console)) {
    // This borrow is scoped to a non-yielding operation. IRQs never use STATE.
    unsafe {
        f(&mut *addr_of_mut!(STATE));
    }
}
pub fn initialize(free_pages: usize, arena_bytes: u64) {
    update(|s| {
        assert!(!s.initialized);
        s.initialized = true;
        s.free_pages = free_pages;
        s.arena_bytes = arena_bytes;
        s.push(true, format_args!("System Regis / console"));
        s.push(
            false,
            format_args!("Type help to explore kernel tasks and memory."),
        );
    });
}
pub fn toggle() {
    update(|s| {
        s.active = !s.active;
        if !s.active {
            crate::input::release_focus();
        }
        s.revision += 1;
    });
}
pub fn close() {
    update(|s| {
        if s.active {
            s.active = false;
            crate::input::release_focus();
            s.revision += 1;
        }
    });
}
pub fn insert(byte: u8) {
    assert!(byte.is_ascii_graphic() || byte == b' ');
    update(|s| {
        if s.len == CAPACITY {
            s.overflow = true;
        } else {
            s.input[s.len] = byte;
            s.len += 1;
        }
        s.revision += 1;
    });
}
pub fn backspace() {
    update(|s| {
        if s.len > 0 {
            s.len -= 1;
        }
        s.overflow = false;
        s.revision += 1;
    });
}
fn task_output(s: &mut Console, id: usize, detail: bool) {
    let t = tasks::inspect(id).expect("validated task");
    s.push(
        false,
        format_args!(
            "#{}  {:8}  {:8}  runs {} / yields {}",
            t.id, t.name, t.state, t.activity.dispatches, t.activity.yields
        ),
    );
    if detail {
        match t.deadline {
            Some(tick) => s.push(
                false,
                format_args!("wait: timer until {} / snapshot tick {}", tick, t.tick),
            ),
            None => s.push(
                false,
                format_args!("wait: {} / snapshot tick {}", t.wait, t.tick),
            ),
        }
        s.push(
            false,
            format_args!(
                "stack: 0x{:016x} .. 0x{:016x} (end exclusive)",
                t.stack_base, t.stack_end
            ),
        );
        s.push(
            false,
            format_args!(
                "reserved: {} KiB / guards: 4 KiB each",
                (t.stack_end - t.stack_base) / 1024
            ),
        );
        s.push(
            false,
            format_args!(
                "completed {}: {}",
                match id {
                    0 => "frames",
                    1 => "polls",
                    _ => "steps",
                },
                t.activity.work
            ),
        );
    }
    serial!(
        r#"{{"protocol":"eutherus.prompt","version":1,"event":"command_task","sequence":{},"id":{},"name":"{}","state":"{}","wait":"{}","snapshot_ticks":{},"stack_base":{},"stack_end":{},"dispatches":{},"yields":{},"work":{}}}"#,
        s.completed,
        t.id,
        t.name,
        t.state,
        t.wait,
        t.tick,
        t.stack_base,
        t.stack_end,
        t.activity.dispatches,
        t.activity.yields,
        t.activity.work
    );
}
pub fn submit(context: usize) {
    update(|s| {
        assert!(s.initialized && tasks::inspect(context).is_some());
        let bytes = s.input;
        let text = core::str::from_utf8(&bytes[..s.len]).unwrap();
        if text.trim().is_empty() && !s.overflow {
            s.len = 0;
            s.revision += 1;
            return;
        }
        s.completed += 1;
        let name = tasks::inspect(context).expect("context task").name;
        s.push(true, format_args!("regis/{} > {}", name, text));
        let mut words = text.split_ascii_whitespace();
        let command = words.next().unwrap_or("");
        let arg = words.next();
        let extra = words.next().is_some();
        let mut kind = "unknown";
        s.status = "ok";
        if s.overflow {
            s.status = "line_too_long";
            s.push(
                false,
                format_args!("Line too long. Limit: 64 characters; command was not run."),
            );
        } else if command.eq_ignore_ascii_case("help") && arg.is_none() {
            kind = "help";
            s.push(false, format_args!("help                  show commands"));
            s.push(
                false,
                format_args!("tasks                 list real kernel tasks"),
            );
            s.push(
                false,
                format_args!("memory                show reserved arena and free page pool"),
            );
            s.push(
                false,
                format_args!(
                    "inspect [graphics|input|counter]   read a task (default: selected branch)"
                ),
            );
            s.push(
                false,
                format_args!("select <graphics|input|counter>    bring a branch forward"),
            );
            s.push(
                false,
                format_args!("tree                  overview / release camera focus"),
            );
            s.push(
                false,
                format_args!("run counter           start the built-in demo task"),
            );
        } else if command.eq_ignore_ascii_case("run")
            && matches!(arg, Some(a) if a.eq_ignore_ascii_case("counter"))
            && !extra
        {
            kind = "run";
            match tasks::start_counter() {
                Ok(run) => {
                    crate::input::navigate(2, true, true, false);
                    s.push(
                        false,
                        format_args!("Counter run {} started / 10 steps / own guarded stack", run),
                    );
                }
                Err(()) => {
                    s.status = "task_busy";
                    s.push(
                        false,
                        format_args!("Counter is already running. Wait for completion."),
                    );
                }
            }
        } else if command.eq_ignore_ascii_case("tree") && arg.is_none() {
            kind = "tree";
            crate::input::navigate(context, false, false, false);
            s.push(
                false,
                format_args!("kernel / cooperative tasks / shared address space"),
            );
            task_output(s, 0, false);
            task_output(s, 1, false);
            if tasks::inspect(2).is_some() {
                task_output(s, 2, false);
            }
        } else if command.eq_ignore_ascii_case("select") && arg.is_some() && !extra {
            kind = "select";
            if let Some(id) = task_id(arg.unwrap()) {
                crate::input::navigate(id, false, true, false);
                s.push(
                    false,
                    format_args!(
                        "Selected {}. Type inspect for details.",
                        tasks::inspect(id).unwrap().name
                    ),
                );
            } else {
                s.status = "unknown_task";
                s.push(
                    false,
                    format_args!("Unknown task. Choose graphics, input, or an existing counter."),
                );
            }
        } else if command.eq_ignore_ascii_case("tasks") && arg.is_none() {
            kind = "tasks";
            task_output(s, 0, false);
            task_output(s, 1, false);
            if tasks::inspect(2).is_some() {
                task_output(s, 2, false);
            }
        } else if command.eq_ignore_ascii_case("memory") && arg.is_none() {
            kind = "memory";
            crate::input::navigate(context, false, true, true);
            s.push(
                false,
                format_args!("page pool free     {:>8} KiB", s.free_pages * 4),
            );
            s.push(
                false,
                format_args!("framebuffer arena  {:>8} KiB", s.arena_bytes / 1024),
            );
            s.push(
                false,
                format_args!(
                    "task stacks        {:>8} KiB (reserved in kernel image)",
                    tasks::reserved_stack_bytes() / 1024
                ),
            );
            serial!(
                r#"{{"protocol":"eutherus.prompt","version":1,"event":"command_memory","sequence":{},"free_pages":{},"arena_bytes":{},"task_stack_bytes":{}}}"#,
                s.completed,
                s.free_pages,
                s.arena_bytes,
                tasks::reserved_stack_bytes()
            );
        } else if command.eq_ignore_ascii_case("inspect") && !extra {
            kind = "inspect";
            let id = match arg {
                None => Some(context),
                Some(a) => task_id(a),
            };
            if let Some(id) = id {
                crate::input::navigate(id, true, true, false);
                task_output(s, id, true);
            } else {
                s.status = "unknown_task";
                s.push(
                    false,
                    format_args!("Unknown task. Choose graphics, input, or an existing counter."),
                );
            }
        } else {
            s.status = "invalid_command";
            s.push(
                false,
                format_args!("Unknown command or arguments. Type help."),
            );
        }
        serial!(
            r#"{{"protocol":"eutherus.prompt","version":1,"event":"command_result","sequence":{},"kind":"{}","status":"{}","context":{},"input_len":{}}}"#,
            s.completed,
            kind,
            s.status,
            context,
            s.len
        );
        s.len = 0;
        s.overflow = false;
        s.revision += 1;
    });
}

fn task_id(name: &str) -> Option<usize> {
    if name.eq_ignore_ascii_case("graphics") || name == "0" {
        Some(0)
    } else if name.eq_ignore_ascii_case("input") || name == "1" {
        Some(1)
    } else if (name.eq_ignore_ascii_case("counter") || name == "2") && tasks::inspect(2).is_some() {
        Some(2)
    } else {
        None
    }
}
