//! Q35 translated PS/2 bootstrap, owned exclusively by task 1.
use crate::{Serial, tasks};
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
static PAUSED: AtomicBool = AtomicBool::new(false);
static TOGGLES: AtomicU64 = AtomicU64::new(0);
pub fn paused() -> bool {
    PAUSED.load(Ordering::Relaxed)
}
pub fn toggles() -> u64 {
    TOGGLES.load(Ordering::Relaxed)
}
// One packed value makes selection/visibility/revision a coherent UI snapshot.
static VIEW: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Copy)]
pub struct View {
    pub selected: usize,
    pub visible: bool,
    pub revision: u64,
    pub focused: bool,
    pub memory: bool,
}
pub fn view() -> View {
    let value = VIEW.load(Ordering::Relaxed);
    View {
        selected: (value & 3) as usize,
        visible: value & 4 != 0,
        revision: value >> 5,
        focused: value & 8 != 0,
        memory: value & 16 != 0,
    }
}
#[derive(Clone, Copy)]
enum Control {
    Space,
    Next,
    Inspect,
    Close,
    Left,
    Right,
    Prompt,
    Enter,
    Backspace,
    Text(u8),
}
struct Decoder {
    extended: bool,
    pause_skip: u8,
    down: [bool; 256],
    shift: [bool; 2],
    ctrl: [bool; 2],
    alt: [bool; 2],
}
impl Decoder {
    fn new() -> Self {
        Self {
            extended: false,
            pause_skip: 0,
            down: [false; 256],
            shift: [false; 2],
            ctrl: [false; 2],
            alt: [false; 2],
        }
    }
    fn feed(&mut self, byte: u8) -> Option<Control> {
        if self.pause_skip > 0 {
            self.pause_skip -= 1;
            return None;
        }
        if byte == 0xe1 {
            self.extended = false;
            self.pause_skip = 5;
            return None;
        }
        if byte == 0xe0 {
            self.extended = true;
            return None;
        }
        let extended = self.extended;
        self.extended = false;
        let code = byte & 0x7f;
        let pressed = byte & 0x80 == 0;
        match (extended, code) {
            (false, 0x2a) => {
                self.shift[0] = pressed;
                return None;
            }
            (false, 0x36) => {
                self.shift[1] = pressed;
                return None;
            }
            (_, 0x1d) => {
                self.ctrl[usize::from(extended)] = pressed;
                return None;
            }
            (_, 0x38) => {
                self.alt[usize::from(extended)] = pressed;
                return None;
            }
            _ => {}
        }
        let down = &mut self.down[code as usize + usize::from(extended) * 128];
        if !pressed {
            *down = false;
            return None;
        }
        if *down {
            return None;
        }
        *down = true;
        if self.ctrl.iter().any(|x| *x) || self.alt.iter().any(|x| *x) {
            return None;
        }
        let control = match (extended, code) {
            (false, 0x39) => Control::Space,
            (false, 0x0f) => Control::Next,
            (false, 0x3c) => Control::Inspect,
            (false, 0x3b) => Control::Prompt,
            (false, 0x01) => Control::Close,
            (false, 0x0e) => Control::Backspace,
            (_, 0x1c) => Control::Enter,
            (true, 0x4b) => Control::Left,
            (true, 0x4d) => Control::Right,
            (false, _) => {
                let shift = self.shift.iter().any(|x| *x);
                let mut letter = None;
                for (start, row) in [
                    (0x10, &b"qwertyuiop"[..]),
                    (0x1e, &b"asdfghjkl"[..]),
                    (0x2c, &b"zxcvbnm"[..]),
                ] {
                    if code >= start && (code - start) < row.len() as u8 {
                        letter = Some(row[(code - start) as usize]);
                    }
                }
                let ascii = if let Some(c) = letter {
                    if shift { c.to_ascii_uppercase() } else { c }
                } else {
                    match code {
                        0x02..=0x0b if !shift => b"1234567890"[(code - 2) as usize],
                        0x0c => {
                            if shift {
                                b'_'
                            } else {
                                b'-'
                            }
                        }
                        0x34 if !shift => b'.',
                        0x35 => {
                            if shift {
                                b'?'
                            } else {
                                b'/'
                            }
                        }
                        _ => return None,
                    }
                };
                Control::Text(ascii)
            }
            _ => return None,
        };
        Some(control)
    }
}
fn handle(control: Control, poll: u64) {
    if matches!(control, Control::Prompt) {
        crate::prompt::toggle();
        return;
    }
    if crate::prompt::snapshot().active {
        match control {
            Control::Text(byte) => crate::prompt::insert(byte),
            Control::Space => crate::prompt::insert(b' '),
            Control::Enter => crate::prompt::submit(view().selected),
            Control::Backspace => crate::prompt::backspace(),
            Control::Close => crate::prompt::close(),
            _ => {}
        }
        return;
    }
    if matches!(
        control,
        Control::Text(_) | Control::Enter | Control::Backspace
    ) {
        return;
    }
    if matches!(control, Control::Space) {
        let next = !paused();
        PAUSED.store(next, Ordering::Relaxed);
        let toggles = TOGGLES.fetch_add(1, Ordering::Relaxed) + 1;
        serial!(
            r#"{{"protocol":"eutherus.graphics","version":1,"event":"rotation_toggled","task_id":1,"paused":{},"toggles":{},"poll":{},"ticks":{}}}"#,
            next,
            toggles,
            poll,
            crate::interrupts::ticks()
        );
        return;
    }
    let previous = view();
    let mut next = previous;
    match control {
        Control::Next => {
            next.selected = (next.selected + 1) % if tasks::inspect(2).is_some() { 3 } else { 2 };
            next.visible = true;
        }
        Control::Left => {
            next.selected = 0;
            next.visible = true;
        }
        Control::Right => {
            next.selected = 1;
            next.visible = true;
        }
        Control::Inspect => next.visible = !next.visible,
        Control::Close => next.visible = false,
        Control::Space
        | Control::Prompt
        | Control::Text(_)
        | Control::Enter
        | Control::Backspace => unreachable!(),
    }
    publish(previous, next);
}
fn publish(previous: View, mut next: View) {
    if next.selected != previous.selected
        || next.visible != previous.visible
        || next.focused != previous.focused
        || next.memory != previous.memory
    {
        next.revision = previous.revision + 1;
        VIEW.store(
            (next.revision << 5)
                | (u64::from(next.memory) << 4)
                | (u64::from(next.focused) << 3)
                | (u64::from(next.visible) << 2)
                | next.selected as u64,
            Ordering::Relaxed,
        );
        serial!(
            r#"{{"protocol":"eutherus.graphics","version":1,"event":"inspector_changed","task_id":1,"selected":{},"visible":{},"revision":{},"ticks":{}}}"#,
            next.selected,
            next.visible,
            next.revision,
            crate::interrupts::ticks()
        );
    }
}
// UI changes only, called from input without yielding. Scheduler state is untouched.
pub fn navigate(selected: usize, visible: bool, focused: bool, memory: bool) {
    assert!(tasks::inspect(selected).is_some());
    let previous = view();
    publish(
        previous,
        View {
            selected,
            visible,
            focused,
            memory,
            ..previous
        },
    );
}
pub fn release_focus() {
    let v = view();
    if v.focused {
        navigate(v.selected, false, false, false);
    }
}

unsafe fn read(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }
    value
}
pub fn run() {
    serial!(
        r#"{{"protocol":"eutherus.graphics","version":1,"event":"input_ready","task_id":1,"rsp":{}}}"#,
        tasks::stack_pointer()
    );
    let mut decoder = Decoder::new();
    let mut polls = 0u64;
    loop {
        // Bound work per dispatch even if the device never drains.
        for _ in 0..32 {
            let status = unsafe { read(0x64) };
            if status & 1 == 0 {
                break;
            }
            let key = unsafe { read(0x60) };
            // AUX bytes do not participate in keyboard prefix/make-break state.
            if status & 0x20 != 0 {
                continue;
            }
            if status & 0xc0 != 0 {
                decoder = Decoder::new();
                continue;
            }
            if let Some(control) = decoder.feed(key) {
                handle(control, polls + 1);
            }
        }

        polls += 1;
        tasks::work_done();
        if polls == 20 {
            let activity = tasks::activity();
            serial!(
                r#"{{"protocol":"eutherus.graphics","version":1,"event":"input_live","polls":{},"input_dispatches":{},"graphics_frames":{},"rsp":{}}}"#,
                polls,
                activity[1].dispatches,
                activity[0].work,
                tasks::stack_pointer()
            );
            if cfg!(feature = "fault-graphics-input-exit") {
                return;
            }
            if cfg!(feature = "fault-graphics-input-guard") {
                unsafe {
                    (tasks::guards()[2] as *mut u64).write_volatile(0xfeed);
                }
                panic!("input guard did not fault");
            }
        }
        tasks::sleep(1);
    }
}
