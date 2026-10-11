#![no_std]
//! Shared allocation-free tower geometry and perspective for host and kernel.
use core::f32::consts::TAU;
#[derive(Clone, Copy)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
#[derive(Clone, Copy)]
pub struct Sample {
    pub p: Point,
    pub nx: f32,
    pub nz: f32,
    pub ch: char,
    pub band: bool,
}
pub fn tint(c: u32, amount: f32) -> u32 {
    let amount = amount.clamp(0., 1.);
    (((((c >> 16) & 255) as f32 * amount) as u32) << 16)
        | (((((c >> 8) & 255) as f32 * amount) as u32) << 8)
        | (((c & 255) as f32 * amount) as u32)
}
pub struct Projector {
    pub sin: f32,
    pub cos: f32,
}
impl Projector {
    pub fn new(angle: f32) -> Self {
        let (sin, cos) = (libm::sinf(angle), libm::cosf(angle));
        Self { sin, cos }
    }
    pub fn project(&self, p: Point) -> (f32, f32, f32) {
        let x = p.x * self.cos + p.z * self.sin;
        let z = -p.x * self.sin + p.z * self.cos;
        let y = p.y * 0.978 - z * 0.208;
        let depth = p.y * 0.208 + z * 0.978;
        let scale = 650. / (6. - depth);
        (523. + x * scale, 389. - y * scale, depth)
    }
}
pub fn samples(mut emit: impl FnMut(Sample)) {
    let mut directions = [(0.0, 0.0); 144];
    for (i, direction) in directions.iter_mut().enumerate() {
        let a = i as f32 / 144. * TAU;
        *direction = (libm::sinf(a), libm::cosf(a));
    }
    for row in 0..83 {
        let y = -1.5 + row as f32 * 0.036;
        let radius = if y < -1.27 {
            1.12
        } else if y > 1.12 {
            1.03
        } else if (row % 17) < 2 {
            0.95
        } else {
            0.86
        };
        for column in 0..144 {
            // Eight crenellations, silhouetted at the crown of the tower.
            if y > 1.28 && (column / 9) % 2 == 1 {
                continue;
            }
            let (sin, cos) = directions[column];
            let band = y < -1.27 || y > 1.1 || row % 17 < 2;
            let ch = if band {
                '='
            } else if row % 6 == 0 {
                '-'
            } else if (column + (row / 6 % 2) * 6) % 12 == 0 {
                '|'
            } else {
                '.'
            };
            emit(Sample {
                p: Point {
                    x: radius * sin,
                    y,
                    z: radius * cos,
                },
                nx: sin,
                nz: cos,
                ch,
                band,
            });
        }
    }
    // Top and base ledges, sampled as rings so the perspective reads as a cylinder.
    for (y, outer) in [(1.14, 1.03), (-1.28, 1.12)] {
        for r in 0..18 {
            let radius = r as f32 / 17. * outer;
            for &(sin, cos) in &directions {
                emit(Sample {
                    p: Point {
                        x: radius * sin,
                        y,
                        z: radius * cos,
                    },
                    nx: 0.,
                    nz: 0.8,
                    ch: '.',
                    band: true,
                });
            }
        }
    }
}
