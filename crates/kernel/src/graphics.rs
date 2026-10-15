//! Bootstrap framebuffer renderer. All firmware access is confined to prepare().
use crate::Serial;
use core::{fmt::Write, ptr::write_volatile};
use regis_scene::{Point, Projector, tint};
use uefi::{
    boot,
    proto::console::gop::{GraphicsOutput, PixelFormat},
};

use crate::Framebuffer;
pub fn prepare() -> Framebuffer {
    let handle = boot::get_handle_for_protocol::<GraphicsOutput>().expect("GOP handle");
    let mut gop = boot::open_protocol_exclusive::<GraphicsOutput>(handle).expect("GOP open");
    let mode = gop
        .modes()
        .find(|m| {
            m.info().resolution() == (1024, 768)
                && matches!(m.info().pixel_format(), PixelFormat::Rgb | PixelFormat::Bgr)
        })
        .expect("1024x768 RGB GOP mode");
    gop.set_mode(&mode).expect("GOP mode");
    let info = gop.current_mode_info();
    let (width, height) = info.resolution();
    let mut fb = gop.frame_buffer();
    let descriptor = Framebuffer {
        address: fb.as_mut_ptr() as u64,
        bytes: fb.size() as u64,
        back_buffer: 0,
        back_buffer_bytes: 0,
        width: width as u32,
        height: height as u32,
        stride: info.stride() as u32,
        format: info.pixel_format() as u32,
    };
    // Scoped GOP borrow closes here, before the final memory map/ExitBootServices.
    descriptor
}
const BG: u32 = 0x08141d;
const FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/font.bin"));
struct Canvas {
    pixels: &'static mut [u32],
    width: usize,
    height: usize,
}
impl Canvas {
    fn line(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, color: u32) {
        let dx = x1 as i32 - x0 as i32;
        let dy = y1 as i32 - y0 as i32;
        let steps = dx.abs().max(dy.abs()).max(1);
        for i in 0..=steps {
            let x = x0 as i32 + dx * i / steps;
            let y = y0 as i32 + dy * i / steps;
            if x >= 0 && y >= 0 {
                self.rect(x as usize, y as usize, 1, 1, color);
            }
        }
    }
    fn rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        for row in y..(y + height).min(self.height) {
            for col in x..(x + width).min(self.width) {
                self.pixels[row * self.width + col] = color;
            }
        }
    }
    fn text(&mut self, x: usize, y: usize, text: &str, color: u32) {
        for (i, c) in text.bytes().enumerate() {
            self.glyph(x + i * 8, y, c, color);
        }
    }
    fn glyph(&mut self, x: usize, y: usize, c: u8, color: u32) {
        if c >= 128 {
            return;
        }
        for dy in 0..14 {
            for dx in 0..8 {
                if x + dx >= self.width || y + dy >= self.height {
                    continue;
                }
                let alpha = FONT[c as usize * 112 + dy * 8 + dx] as u32;
                if alpha == 0 {
                    continue;
                }
                let at = (y + dy) * self.width + x + dx;
                let bg = self.pixels[at];
                let mut result = 0;
                for shift in [0, 8, 16] {
                    result |= ((((color >> shift) & 255) * alpha
                        + ((bg >> shift) & 255) * (255 - alpha))
                        / 255)
                        << shift;
                }
                self.pixels[at] = result;
            }
        }
    }
}
struct Label {
    bytes: [u8; 96],
    len: usize,
}
impl Write for Label {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len + text.len();
        if end > self.bytes.len() {
            return Err(core::fmt::Error);
        }
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
// Initialized once before tasks start; read as a copy by the renderer only.
static mut CONFIG: Option<(Framebuffer, usize)> = None;
pub fn run(fb: &Framebuffer, free_pages: usize) -> ! {
    crate::prompt::initialize(free_pages, fb.back_buffer_bytes);
    unsafe {
        CONFIG = Some((*fb, free_pages));
    }
    crate::tasks::run_with([
        crate::tasks::Task {
            name: "graphics",
            body: renderer,
        },
        crate::tasks::Task {
            name: "input",
            body: crate::input::run,
        },
    ]);
    panic!("graphics scheduler returned")
}
fn renderer() {
    let (fb, free_pages) = unsafe { CONFIG.expect("graphics config") };
    render(&fb, free_pages)
}
fn render(fb: &Framebuffer, free_pages: usize) -> ! {
    serial!(
        r#"{{"protocol":"eutherus.graphics","version":1,"event":"renderer_ready","task_id":0,"rsp":{}}}"#,
        crate::tasks::stack_pointer()
    );
    let mut label = Label {
        bytes: [0; 96],
        len: 0,
    };
    write!(
        label,
        "PAGE POOL FREE: {} KiB / FRAME ARENA: {} KiB",
        free_pages * 4,
        fb.back_buffer_bytes / 1024
    )
    .unwrap();
    let memory_label = core::str::from_utf8(&label.bytes[..label.len]).unwrap();
    assert!(fb.width == 1024 && fb.height == 768 && fb.stride >= fb.width);
    assert!(fb.format <= 1 && fb.address != 0 && fb.back_buffer != 0);
    assert!(fb.address % 4 == 0 && fb.back_buffer % 4096 == 0);
    assert!(fb.bytes >= fb.stride as u64 * fb.height as u64 * 4);
    assert!(fb.back_buffer_bytes >= fb.width as u64 * fb.height as u64 * 8);
    let mut canvas = Canvas {
        pixels: unsafe {
            core::slice::from_raw_parts_mut(
                fb.back_buffer as *mut u32,
                fb.width as usize * fb.height as usize,
            )
        },
        width: fb.width as usize,
        height: fb.height as usize,
    };
    let shadow = unsafe {
        core::slice::from_raw_parts_mut(
            (fb.back_buffer as *mut u32).add(canvas.pixels.len()),
            canvas.pixels.len(),
        )
    };
    shadow.fill(0xffff_ffff);
    let mut angle = -0.48;
    let mut frame = 0u64;
    let mut shown_revision = u64::MAX;
    let mut prompt_revision = u64::MAX;
    let mut counter_shown = (0, 0, "absent");
    assert!(
        crate::tasks::inspect(2).is_none(),
        "invalid task inspection"
    );
    loop {
        let paused = crate::input::paused();
        let toggles = crate::input::toggles();
        let activity = crate::tasks::activity();
        let view = crate::input::view();
        let inspected = crate::tasks::inspect(view.selected).expect("selected live task");
        let counter = crate::tasks::inspect(2);
        let counter_key = counter
            .map(|t| (crate::tasks::counter_run(), t.activity.work, t.state))
            .unwrap_or((0, 0, "absent"));
        let counter_changed = counter_key != counter_shown;
        let view_changed = view.revision != shown_revision;
        let console = crate::prompt::snapshot();
        let target = if view.memory {
            0.45
        } else if view.selected == 0 {
            0.0
        } else if view.selected == 1 {
            core::f32::consts::PI
        } else {
            core::f32::consts::FRAC_PI_2
        };
        let mut settled = false;
        if view.focused {
            // Shortest arc, bounded speed, ease-out, then exact rest (no endless drift).
            let mut delta = target - angle;
            while delta > core::f32::consts::PI {
                delta -= core::f32::consts::TAU;
            }
            while delta < -core::f32::consts::PI {
                delta += core::f32::consts::TAU;
            }
            if delta.abs() <= 0.003 {
                angle = target;
                settled = true;
            } else {
                angle += (delta * 0.18).clamp(-0.09, 0.09);
            }
        } else if frame > 0 && !paused {
            angle += 0.025;
            if angle >= core::f32::consts::TAU {
                angle -= core::f32::consts::TAU;
            }
        }
        canvas.pixels.fill(BG);
        canvas.text(48, 40, "S Y S T E M   R E G I S", 0xe9ca8c);
        canvas.text(48, 66, "EUTHERUS / NATIVE KERNEL / QEMU TCG", 0x77cfc6);
        canvas.text(
            48,
            94,
            "LIVE KERNEL TASKS / COOPERATIVE SCHEDULING",
            0x8b9ca8,
        );
        let projection = Projector::new(angle);
        let mut depths = [f32::NEG_INFINITY; 118 * 37];
        let mut glyphs = [b' '; 118 * 37];
        let mut colors = [0u32; 118 * 37];
        regis_scene::samples(|s| {
            let (x, y, z) = projection.project(s.p);
            if x < 54. || y < 188. {
                return;
            }
            let col = ((x - 54.) / 8.) as usize;
            let row = ((y - 188.) / 11.) as usize;
            if col >= 118 || row >= 37 {
                return;
            }
            let at = row * 118 + col;
            if z <= depths[at] {
                return;
            }
            depths[at] = z;
            let nx = s.nx * projection.cos + s.nz * projection.sin;
            let nz = -s.nx * projection.sin + s.nz * projection.cos;
            let light = (0.42 + nz * 0.38 - nx * 0.17).clamp(0.16, 1.);
            glyphs[at] = if s.ch == '.' {
                if light > 0.72 {
                    b':'
                } else if light > 0.47 {
                    b'.'
                } else {
                    b','
                }
            } else {
                s.ch as u8
            };
            colors[at] = tint(if s.band { 0xe9ca8c } else { 0xb5a386 }, light);
        });
        for i in 0..glyphs.len() {
            if glyphs[i] != b' ' {
                canvas.glyph(54 + i % 118 * 8, 188 + i / 118 * 11, glyphs[i], colors[i]);
            }
        }
        // Branches are measured scheduler activity, not the native simulated model.
        for (id, x, y, title, unit) in [
            (0, 64, 276, "GRAPHICS", "FRAMES"),
            (1, 704, 400, "INPUT", "POLLS"),
            (2, 64, 400, "COUNTER", "STEPS"),
        ] {
            if id == 2 && counter.is_none() {
                continue;
            }
            let item = activity[id];
            let mut text = Label {
                bytes: [0; 96],
                len: 0,
            };
            write!(
                text,
                "{} {}",
                if view.selected == id {
                    ">"
                } else {
                    [".", "o", "O", "o"][item.dispatches as usize % 4]
                },
                title
            )
            .unwrap();
            canvas.text(
                x,
                y,
                core::str::from_utf8(&text.bytes[..text.len]).unwrap(),
                if view.selected == id {
                    0xe9ca8c
                } else {
                    0x77cfc6
                },
            );
            for (row, key, value) in [
                (1, "RUNS", item.dispatches),
                (2, unit, item.work),
                (3, "YIELDS", item.yields),
            ] {
                text.len = 0;
                if id == 2 && row == 3 {
                    write!(
                        text,
                        "{} / RUN {}",
                        counter.unwrap().state,
                        crate::tasks::counter_run()
                    )
                    .unwrap();
                } else {
                    write!(text, "{} {:>10}", key, value).unwrap();
                }
                canvas.text(
                    x,
                    y + row * 20,
                    core::str::from_utf8(&text.bytes[..text.len]).unwrap(),
                    0xb5a386,
                );
            }
            canvas.text(x, y + 80, "STACK 128 KiB / GUARDED", 0x8b9ca8);
        }
        // Fixed, readable labels connected to points in the rotating geometry.
        for (id, px, y, z, label_x, label_y) in [
            (0, 0.0, 0.8, 0.9, 248, 283),
            (1, 0.0, -0.3, -0.9, 696, 407),
            (2, -0.9, -0.45, 0.0, 248, 407),
        ] {
            if id == 2 && counter.is_none() {
                continue;
            }
            let (x, y, _) = projection.project(Point { x: px, y, z });
            let front = -px * projection.sin + z * projection.cos >= 0.0;
            let color = if view.memory || (view.focused && view.selected == id) {
                0xe9ca8c
            } else if front {
                0x4f827f
            } else {
                0x243c48
            };
            canvas.line(label_x, label_y, x as usize, y as usize, color);
            canvas.rect(x as usize - 3, y as usize - 3, 7, 7, color);
            canvas.rect(x as usize - 1, y as usize - 1, 3, 3, BG);
        }
        if view.focused {
            canvas.text(
                48,
                122,
                if view.memory {
                    "MEMORY / SHARED POOL + TASK RESERVATIONS"
                } else if settled {
                    "BRANCH IN FOCUS / ESC OR tree TO RELEASE"
                } else {
                    "BRINGING BRANCH INTO VIEW"
                },
                0x77cfc6,
            );
        }
        if view.memory {
            memory_panel(&mut canvas, free_pages, fb.back_buffer_bytes);
        }
        if view.visible {
            inspector(&mut canvas, inspected);
        }
        canvas.text(48, 642, "KERNEL TASKS / IRQ CLOCK / SLEEP + HLT", 0xe9ca8c);
        canvas.text(
            48,
            666,
            "F1: CONSOLE   TAB / ARROWS: SELECT   F2: INSPECT   SPACE: ROTATE",
            0x77cfc6,
        );
        canvas.text(48, 704, memory_label, 0x8b9ca8);
        canvas.text(
            48,
            728,
            if view.focused {
                "READING FOCUS / CAMERA HELD"
            } else if paused {
                "ROTATION PAUSED"
            } else {
                "ROTATION RUNNING"
            },
            0xe9ca8c,
        );
        if console.active {
            draw_prompt(&mut canvas, &console, inspected.name);
        }
        for y in 0..canvas.height {
            for x in 0..canvas.width {
                let at = y * canvas.width + x;
                let rgb = canvas.pixels[at];
                if shadow[at] == rgb {
                    continue;
                }
                shadow[at] = rgb;
                let pixel = if fb.format == 1 {
                    rgb
                } else {
                    (rgb & 0xff00) | ((rgb & 255) << 16) | ((rgb >> 16) & 255)
                };
                unsafe {
                    write_volatile(
                        (fb.address as *mut u32).add(y * fb.stride as usize + x),
                        pixel,
                    );
                }
            }
        }
        if frame % 20 == 0 || view_changed {
            serial!(
                r#"{{"protocol":"eutherus.graphics","version":1,"event":"frame_presented","frame":{},"paused":{},"angle_milliradians":{},"timer_ticks":{},"halt_waits":{},"gfx_dispatches":{},"gfx_yields":{},"gfx_frames":{},"input_dispatches":{},"input_yields":{},"input_polls":{},"toggles":{},"inspector_visible":{},"selected_task":{},"selection_revision":{}}}"#,
                frame,
                paused,
                (angle * 1000.0) as i32,
                super::interrupts::ticks(),
                super::interrupts::waits(),
                activity[0].dispatches,
                activity[0].yields,
                activity[0].work,
                activity[1].dispatches,
                activity[1].yields,
                activity[1].work,
                toggles,
                view.visible,
                view.selected,
                view.revision
            );
        }
        if view.visible && (frame % 20 == 0 || view_changed || counter_changed) {
            // The same snapshot drives pixels and this post-presentation evidence.
            let i = inspected;
            let mut deadline = Label {
                bytes: [0; 96],
                len: 0,
            };
            if let Some(tick) = i.deadline {
                write!(deadline, "{}", tick).unwrap();
            } else {
                write!(deadline, "null").unwrap();
            }
            serial!(
                r#"{{"protocol":"eutherus.graphics","version":1,"event":"inspector_presented","frame":{},"revision":{},"id":{},"name":"{}","state":"{}","wait":"{}","deadline":{},"snapshot_ticks":{},"stack_base":{},"stack_end":{},"guard_low":{},"guard_high":{},"dispatches":{},"yields":{},"work":{}}}"#,
                frame,
                view.revision,
                i.id,
                i.name,
                i.state,
                i.wait,
                core::str::from_utf8(&deadline.bytes[..deadline.len]).unwrap(),
                i.tick,
                i.stack_base,
                i.stack_end,
                i.guard_low,
                i.guard_high,
                i.activity.dispatches,
                i.activity.yields,
                i.activity.work
            );
        }
        if console.revision != prompt_revision || (console.active && frame % 20 == 0) {
            // Hex keeps arbitrary command bytes out of JSON string syntax.
            let mut encoded = [0u8; 128];
            for (i, byte) in console.input[..console.len].iter().enumerate() {
                encoded[i * 2] = b"0123456789abcdef"[(byte >> 4) as usize];
                encoded[i * 2 + 1] = b"0123456789abcdef"[(byte & 15) as usize];
            }
            serial!(
                r#"{{"protocol":"eutherus.prompt","version":1,"event":"prompt_presented","frame":{},"revision":{},"active":{},"context":{},"completed":{},"input_len":{},"input_hex":"{}","overflow":{},"rows":{},"status":"{}"}}"#,
                frame,
                console.revision,
                console.active,
                view.selected,
                console.completed,
                console.len,
                core::str::from_utf8(&encoded[..console.len * 2]).unwrap(),
                console.overflow,
                console.count,
                console.status
            );
        }
        if view_changed || (view.focused && (!settled || frame % 5 == 0)) || frame % 20 == 0 {
            serial!(
                r#"{{"protocol":"eutherus.graphics","version":1,"event":"navigation_presented","frame":{},"revision":{},"selected":{},"focused":{},"mode":"{}","settled":{},"angle_milliradians":{},"target_milliradians":{},"inspector_visible":{},"free_pages":{},"arena_bytes":{}}}"#,
                frame,
                view.revision,
                view.selected,
                view.focused,
                if view.memory {
                    "memory"
                } else if view.focused {
                    "task"
                } else {
                    "overview"
                },
                settled,
                (angle * 1000.) as i32,
                (target * 1000.) as i32,
                view.visible,
                free_pages,
                fb.back_buffer_bytes
            );
        }
        if counter_changed || (counter.is_some() && frame % 20 == 0) {
            if let Some(t) = counter {
                serial!(
                    r#"{{"protocol":"eutherus.graphics","version":1,"event":"counter_presented","frame":{},"run":{},"state":"{}","steps":{},"dispatches":{},"sleeps":{},"selected":{},"gfx_frames":{},"input_polls":{}}}"#,
                    frame,
                    crate::tasks::counter_run(),
                    t.state,
                    t.activity.work,
                    t.activity.dispatches,
                    t.activity.sleeps,
                    view.selected,
                    activity[0].work,
                    activity[1].work
                );
            }
        }
        counter_shown = counter_key;
        prompt_revision = console.revision;
        shown_revision = view.revision;
        crate::tasks::work_done();
        frame += 1;
        crate::tasks::sleep(5);
    }
}

fn inspector(canvas: &mut Canvas, i: crate::tasks::Snapshot) {
    canvas.text(704, 136, "TASK INSPECTOR", 0xe9ca8c);
    let mut line = Label {
        bytes: [0; 96],
        len: 0,
    };
    for row in 0..12 {
        line.len = 0;
        match row {
            0 => write!(line, "{} / #{}", i.name, i.id),
            1 => write!(line, "STATE  {}", i.state),
            2 => match i.deadline {
                Some(tick) => write!(line, "WAIT   timer until {}", tick),
                None => write!(line, "WAIT   {}", i.wait),
            },
            3 => write!(line, "RUNS   {}", i.activity.dispatches),
            4 => write!(line, "YIELDS {}", i.activity.yields),
            5 => write!(
                line,
                "{}  {}",
                match i.id {
                    0 => "FRAMES",
                    1 => "POLLS ",
                    _ => "STEPS ",
                },
                i.activity.work
            ),
            6 => write!(line, "STACK  {} KiB", (i.stack_end - i.stack_base) / 1024),
            7 => write!(line, "START  0x{:016x}", i.stack_base),
            8 => write!(line, "END    0x{:016x}", i.stack_end),
            9 => write!(
                line,
                "GUARDS {} KiB each",
                (i.stack_base - i.guard_low) / 1024
            ),
            10 => write!(line, "SNAPSHOT TICK {}", i.tick),
            _ => write!(line, "END IS EXCLUSIVE"),
        }
        .unwrap();
        canvas.text(
            704,
            158 + row * 18,
            core::str::from_utf8(&line.bytes[..line.len]).unwrap(),
            if row < 3 { 0x77cfc6 } else { 0x8b9ca8 },
        );
    }
}

fn draw_prompt(canvas: &mut Canvas, console: &crate::prompt::Console, context: &str) {
    canvas.rect(32, 498, 960, 254, 0x0d1b25);
    canvas.rect(32, 498, 960, 1, 0x35565e);
    canvas.rect(32, 498, 3, 254, 0x77cfc6);
    canvas.text(56, 514, "REGIS  /  CONSOLE", 0xe9ca8c);
    canvas.text(720, 514, "F1 / ESC  RETURN TO TOWER", 0x7e929e);
    for (i, line) in console.rows[..console.count].iter().enumerate() {
        canvas.text(
            56,
            538 + i * 18,
            line.text(),
            if line.accent { 0x77cfc6 } else { 0xc8d4dc },
        );
    }
    canvas.rect(48, 690, 928, 1, 0x243c48);
    let mut prefix = Label {
        bytes: [0; 96],
        len: 0,
    };
    write!(prefix, "regis/{} > ", context).unwrap();
    canvas.text(
        56,
        704,
        core::str::from_utf8(&prefix.bytes[..prefix.len]).unwrap(),
        0x77cfc6,
    );
    let x = 56 + prefix.len * 8;
    canvas.text(x, 704, console.text(), 0xe8edf2);
    // Steady caret: text entry remains clear even while the tower is paused.
    canvas.rect(x + console.len * 8, 704, 2, 14, 0xe9ca8c);
    canvas.text(
        56,
        734,
        if console.overflow {
            "LINE FULL / 64 CHARACTERS / BACKSPACE TO EDIT"
        } else {
            "ENTER RUN   BACKSPACE EDIT   select / inspect / memory / tree   help"
        },
        if console.overflow { 0xe9a080 } else { 0x7e929e },
    );
}

fn memory_panel(canvas: &mut Canvas, free_pages: usize, arena_bytes: u64) {
    canvas.text(704, 136, "MEMORY / OWNERSHIP", 0xe9ca8c);
    for (row, key, value) in [
        (0, "POOL FREE", free_pages as u64 * 4),
        (1, "GFX ARENA", arena_bytes / 1024),
        (2, "GFX STACK", 128),
        (3, "INPUT STACK", 128),
        (4, "COUNTER SLOT", 128),
    ] {
        let mut text = Label {
            bytes: [0; 96],
            len: 0,
        };
        write!(text, "{} {} KiB", key, value).unwrap();
        canvas.text(
            704,
            164 + row * 24,
            core::str::from_utf8(&text.bytes[..text.len]).unwrap(),
            0x77cfc6,
        );
    }
    canvas.text(704, 274, "STACKS RESERVED IN IMAGE", 0x8b9ca8);
    canvas.text(704, 296, "ARENA OWNED BY GRAPHICS", 0x8b9ca8);
    canvas.text(704, 318, "POOL SHARED / BOOT SNAPSHOT", 0x8b9ca8);
    canvas.text(704, 340, "CAPACITY, NOT STACK USAGE", 0x8b9ca8);
}
