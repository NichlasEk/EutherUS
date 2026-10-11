use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;

pub const W: usize = 1440;
pub const H: usize = 900;
pub const INK: u32 = 0xe4e8e8;
pub const MUTED: u32 = 0x8c9ba9;
pub const GOLD: u32 = 0xddbd7d;
pub const EDGE: u32 = 0x263948;
pub const COLORS: [u32; 7] = [
    GOLD, 0x8aade2, 0x79cbbb, 0xddbd7d, 0x63e2bf, 0xad94ef, 0xea9f85,
];
pub fn color(id: u32) -> u32 {
    COLORS[id as usize % COLORS.len()]
}
fn mix(a: u32, b: u32, alpha: u32) -> u32 {
    let inv = 255 - alpha;
    let r = (((a >> 16) & 255) * inv + ((b >> 16) & 255) * alpha) / 255;
    let g = (((a >> 8) & 255) * inv + ((b >> 8) & 255) * alpha) / 255;
    let b = ((a & 255) * inv + (b & 255) * alpha) / 255;
    (r << 16) | (g << 8) | b
}
pub struct Painter {
    pub pixels: Vec<u32>,
    background: Vec<u32>,
    fonts: [Font; 3],
    glyphs: HashMap<(usize, u16, char), (Metrics, Vec<u8>)>,
}
impl Painter {
    pub fn new() -> Self {
        let fonts = [
            Font::from_bytes(
                include_bytes!("../assets/Sans.ttf") as &[u8],
                FontSettings::default(),
            )
            .unwrap(),
            Font::from_bytes(
                include_bytes!("../assets/Serif.ttf") as &[u8],
                FontSettings::default(),
            )
            .unwrap(),
            Font::from_bytes(
                include_bytes!("../assets/Mono.ttf") as &[u8],
                FontSettings::default(),
            )
            .unwrap(),
        ];
        let mut background = vec![0; W * H];
        for y in 0..H {
            for x in 0..W {
                let dx = (x as f32 - 500.) / 1300.;
                let dy = (y as f32 - 300.) / 900.;
                let light = (1. - (dx * dx + dy * dy)).max(0.);
                let r = 7 + (light * 4.) as u32;
                let g = 12 + (light * 9.) as u32;
                let b = 19 + (light * 14.) as u32;
                let grid = if x % 48 == 0 && y > 125 && y < 640 {
                    2
                } else {
                    0
                };
                background[y * W + x] = ((r + grid) << 16) | ((g + grid) << 8) | (b + grid);
            }
        }
        Self {
            pixels: background.clone(),
            background,
            fonts,
            glyphs: HashMap::new(),
        }
    }
    pub fn reset(&mut self) {
        self.pixels.copy_from_slice(&self.background);
    }
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        let x0 = x.clamp(0, W as i32) as usize;
        let x1 = (x + w).clamp(0, W as i32) as usize;
        let y0 = y.clamp(0, H as i32) as usize;
        let y1 = (y + h).clamp(0, H as i32) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for row in y0..y1 {
            self.pixels[row * W + x0..row * W + x1].fill(c);
        }
    }
    pub fn pixel(&mut self, x: i32, y: i32, c: u32, alpha: u32) {
        if x >= 0 && y >= 0 && x < W as i32 && y < H as i32 {
            let p = &mut self.pixels[y as usize * W + x as usize];
            *p = mix(*p, c, alpha.min(255));
        }
    }
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let steps = dx.abs().max(dy.abs()).max(1);
        for i in 0..=steps {
            self.pixel(x0 + dx * i / steps, y0 + dy * i / steps, c, 255);
        }
    }
    pub fn border(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        self.rect(x, y, w, 1, c);
        self.rect(x, y + h - 1, w, 1, c);
        self.rect(x, y, 1, h, c);
        self.rect(x + w - 1, y, 1, h, c);
    }
    pub fn glow(&mut self, x: i32, y: i32, radius: i32, c: u32, power: u32) {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let d2 = dx * dx + dy * dy;
                if d2 < radius * radius {
                    let value = 1. - d2 as f32 / (radius * radius) as f32;
                    self.pixel(x + dx, y + dy, c, (value * value * power as f32) as u32);
                }
            }
        }
    }
    pub fn text(
        &mut self,
        x: i32,
        baseline: i32,
        size: u16,
        font: usize,
        text: &str,
        c: u32,
    ) -> i32 {
        let mut cursor = x as f32;
        for ch in text.chars() {
            let key = (font, size, ch);
            let (metrics, bitmap) = self
                .glyphs
                .entry(key)
                .or_insert_with(|| self.fonts[font].rasterize(ch, size as f32));
            let x0 = cursor as i32 + metrics.xmin;
            let y0 = baseline - metrics.height as i32 - metrics.ymin;
            for y in 0..metrics.height {
                for x in 0..metrics.width {
                    let px = x0 + x as i32;
                    let py = y0 + y as i32;
                    let a = bitmap[y * metrics.width + x] as u32;
                    if a > 0 && px >= 0 && py >= 0 && px < W as i32 && py < H as i32 {
                        let target = &mut self.pixels[py as usize * W + px as usize];
                        *target = mix(*target, c, a);
                    }
                }
            }
            cursor += metrics.advance_width;
        }
        cursor as i32
    }
    pub fn save(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut encoder = png::Encoder::new(file, W as u32, H as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut data = Vec::with_capacity(W * H * 3);
        for p in &self.pixels {
            data.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
        }
        encoder.write_header()?.write_image_data(&data)?;
        Ok(())
    }
}
