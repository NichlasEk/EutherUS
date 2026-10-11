//! Explicit aspect fit shared by rendering and pointer mapping. Avoids backend
//! dependent placement when a tiling compositor changes the requested size.
use crate::paint::{H, W};
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}
impl Viewport {
    pub fn new(w: usize, h: usize) -> Self {
        let scale = (w as f64 / W as f64).min(h as f64 / H as f64);
        let width = ((W as f64 * scale) as usize).max(1).min(w.max(1));
        let height = ((H as f64 * scale) as usize).max(1).min(h.max(1));
        Self {
            x: w.saturating_sub(width) / 2,
            y: h.saturating_sub(height) / 2,
            width,
            height,
        }
    }
    pub fn pointer(self, x: f32, y: f32) -> Option<(i32, i32)> {
        let x = x - self.x as f32;
        let y = y - self.y as f32;
        if x < 0. || y < 0. || x >= self.width as f32 || y >= self.height as f32 {
            return None;
        }
        Some((
            (x * W as f32 / self.width as f32) as i32,
            (y * H as f32 / self.height as f32) as i32,
        ))
    }
}
fn blend(a: u32, b: u32, t: u32) -> u32 {
    let rb = (((a & 0xff00ff) * (256 - t) + (b & 0xff00ff) * t) >> 8) & 0xff00ff;
    let g = (((a & 0x00ff00) * (256 - t) + (b & 0x00ff00) * t) >> 8) & 0x00ff00;
    rb | g
}
pub struct Presentation {
    pub pixels: Vec<u32>,
    size: (usize, usize),
    xs: Vec<(usize, u32)>,
}
impl Presentation {
    pub fn new() -> Self {
        Self {
            pixels: Vec::new(),
            size: (0, 0),
            xs: Vec::new(),
        }
    }
    pub fn fit(&mut self, source: &[u32], w: usize, h: usize) -> Result<(), &'static str> {
        if w == 0 || h == 0 || w.saturating_mul(h) > 16_777_216 {
            return Err("Unsupported window dimensions");
        }
        let v = Viewport::new(w, h);
        if self.size != (w, h) {
            self.size = (w, h);
            self.pixels.resize(w * h, 0x070c13);
            self.pixels.fill(0x070c13);
            self.xs = (0..v.width)
                .map(|dx| {
                    let sx = dx * (W - 1) * 256 / v.width.saturating_sub(1).max(1);
                    (sx / 256, (sx % 256) as u32)
                })
                .collect();
        }
        for dy in 0..v.height {
            let sy = dy * (H - 1) * 256 / v.height.saturating_sub(1).max(1);
            let y = sy / 256;
            let t = (sy % 256) as u32;
            for (dx, &(x, tx)) in self.xs.iter().enumerate() {
                let a = blend(source[y * W + x], source[y * W + (x + 1).min(W - 1)], tx);
                let b = blend(
                    source[(y + 1).min(H - 1) * W + x],
                    source[(y + 1).min(H - 1) * W + (x + 1).min(W - 1)],
                    tx,
                );
                self.pixels[(v.y + dy) * w + v.x + dx] = blend(a, b, t);
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tall_and_wide_windows_map_visible_content_and_ignore_bars() {
        for (w, h) in [(830, 1002), (2000, 500), (1440, 900)] {
            let v = Viewport::new(w, h);
            let center = v
                .pointer((v.x + v.width / 2) as f32, (v.y + v.height / 2) as f32)
                .unwrap();
            assert!((center.0 - 720).abs() <= 2 && (center.1 - 450).abs() <= 2);
            assert_eq!(v.pointer(-1., 0.), None);
            if v.y > 0 {
                assert_eq!(v.pointer(20., 0.), None);
            }
            let mut p = Presentation::new();
            p.fit(&vec![0xffffff; W * H], w, h).unwrap();
            assert_eq!(
                p.pixels[(v.y + v.height / 2) * w + v.x + v.width / 2],
                0xffffff
            );
            if v.y > 0 {
                assert_eq!(p.pixels[0], 0x070c13);
            }
        }
    }
}
