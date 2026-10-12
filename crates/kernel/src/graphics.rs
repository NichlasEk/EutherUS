//! Bootstrap framebuffer renderer. All firmware access is confined to prepare().
use crate::Serial;
use core::{fmt::Write, ptr::write_volatile};
use regis_scene::{Projector, tint};
use uefi::{
    boot,
    proto::console::gop::{GraphicsOutput, PixelFormat},
};

/// Internal handoff extension v2; all addresses refer to retained mappings.
#[derive(Clone, Copy)]
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
    let bytes = width.checked_mul(height).unwrap().checked_mul(4).unwrap();
    let pages = (bytes * 2).div_ceil(4096);
    let back = boot::allocate_pages(
        boot::AllocateType::AnyPages,
        boot::MemoryType::LOADER_DATA,
        pages,
    )
    .expect("back buffer");
    let descriptor = Framebuffer {
        address: fb.as_mut_ptr() as u64,
        bytes: fb.size() as u64,
        back_buffer: back.as_ptr() as u64,
        back_buffer_bytes: (pages * 4096) as u64,
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
unsafe fn input(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!("in al, dx",in("dx") port,out("al") value,options(nomem,nostack,preserves_flags));
    }
    value
}
fn wait_frame() {
    super::interrupts::wait_ticks(5);
}
pub fn run(fb: &Framebuffer) -> ! {
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
    let mut paused = false;
    let mut space_down = false;
    loop {
        canvas.pixels.fill(BG);
        canvas.text(48, 40, "S Y S T E M   R E G I S", 0xe9ca8c);
        canvas.text(48, 66, "EUTHERUS / NATIVE KERNEL / QEMU TCG", 0x77cfc6);
        canvas.text(
            48,
            94,
            "UEFI BOOT SERVICES EXITED - CPU FRAMEBUFFER RENDERING",
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
        canvas.text(48, 642, "SHARED ASCII GEOMETRY / IRQ TIMER + HLT", 0xe9ca8c);
        canvas.text(48, 666, "SPACE: PAUSE / RESUME ROTATION", 0x77cfc6);
        canvas.text(
            48,
            704,
            "BOOTSTRAP GRAPHICS - NO PROCESS SCHEDULER YET",
            0x8b9ca8,
        );
        canvas.text(
            48,
            728,
            if paused {
                "ROTATION PAUSED"
            } else {
                "ROTATION RUNNING"
            },
            0xe9ca8c,
        );
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
        if frame % 20 == 0 {
            serial!(
                r#"{{"protocol":"eutherus.graphics","version":1,"event":"frame_presented","frame":{},"paused":{},"angle_milliradians":{},"timer_ticks":{},"halt_waits":{}}}"#,
                frame,
                paused,
                (angle * 1000.0) as i32,
                super::interrupts::ticks(),
                super::interrupts::waits()
            );
        }
        frame += 1;
        wait_frame();
        // Q35 legacy keyboard bootstrap. Consume make/break bytes; ignore mouse bytes.
        unsafe {
            for _ in 0..32 {
                let status = input(0x64);
                if status & 1 == 0 {
                    break;
                }
                let key = input(0x60);
                if status & 0x20 == 0 {
                    if key == 0x39 && !space_down {
                        paused = !paused;
                        space_down = true;
                    }
                    if key == 0xb9 {
                        space_down = false;
                    }
                }
            }
        }
        if !paused {
            angle += 0.025;
            if angle >= core::f32::consts::TAU {
                angle -= core::f32::consts::TAU;
            }
        }
    }
}
