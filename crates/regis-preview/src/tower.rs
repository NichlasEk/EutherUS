//! CPU-projected ASCII architecture. Depth testing happens at character-cell
//! resolution; labels have stable screen-space hitboxes independent of orbit.
use crate::paint::{GOLD, INK, MUTED, Painter, color};
use regis_model::Model;
use std::f32::consts::TAU;
const COLS: usize = 118;
const ROWS: usize = 37;
const X: i32 = 54;
const Y: i32 = 188;
const CW: i32 = 8;
const CH: i32 = 11;

use regis_scene::{Point, Projector, Sample, tint};
#[derive(Clone, Copy)]
struct Cell {
    depth: f32,
    ch: char,
    color: u32,
}
impl Cell {
    fn put(&mut self, depth: f32, ch: char, color: u32) {
        if depth > self.depth {
            *self = Self { depth, ch, color };
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Label {
    pub id: u32,
    pub x: i32,
    pub y: i32,
}
impl Label {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        (self.x..self.x + 225).contains(&x) && (self.y..self.y + 62).contains(&y)
    }
}
pub fn labels(model: &Model, scroll: usize) -> Vec<Label> {
    model
        .order()
        .iter()
        .skip(scroll)
        .take(8)
        .enumerate()
        .map(|(i, (id, _))| Label {
            id: *id,
            x: if i % 2 == 0 { 70 } else { 771 },
            y: 214 + (i / 2) as i32 * 92,
        })
        .collect()
}
fn theta(id: u32) -> f32 {
    id as f32 * 2.3999631
}
fn anchor(model: &Model, id: u32) -> Point {
    let depth = model
        .order()
        .into_iter()
        .find(|(i, _)| *i == id)
        .map_or(0, |(_, d)| d);
    let radius = if id == 0 {
        0.
    } else {
        1.18 + (depth.min(4) as f32) * 0.26
    };
    let a = theta(id);
    Point {
        x: radius * a.sin(),
        y: 1.08 - (id % 7) as f32 * 0.34,
        z: radius * a.cos(),
    }
}
fn write(cells: &mut [Cell], x: f32, y: f32, z: f32, ch: char, color: u32) {
    let col = ((x - X as f32) / CW as f32).floor() as i32;
    let row = ((y - Y as f32) / CH as f32).floor() as i32;
    if col >= 0 && row >= 0 && col < COLS as i32 && row < ROWS as i32 {
        cells[row as usize * COLS + col as usize].put(z, ch, color);
    }
}
pub struct Tower {
    pub angle: f32,
    pub locked: bool,
    pub hovered: bool,
    target: Option<f32>,
    samples: Vec<Sample>,
}
impl Tower {
    pub fn new() -> Self {
        let mut samples = Vec::new();
        regis_scene::samples(|sample| samples.push(sample));
        Self {
            angle: -0.48,
            locked: false,
            hovered: false,
            target: None,
            samples,
        }
    }
    pub fn hold(&mut self) {
        self.locked = true;
        self.target = None;
    }
    pub fn release(&mut self) {
        self.locked = false;
        self.target = None;
    }
    pub fn focus(&mut self, id: u32, motion: bool) {
        self.locked = true;
        if motion {
            self.target = Some(-theta(id));
        } else {
            self.target = None;
        }
    }
    pub fn advance(&mut self, dt: f32, motion: bool, hover: bool) {
        self.hovered = hover;
        if !motion {
            return;
        }
        let dt = dt.clamp(0., 0.05);
        if let Some(target) = self.target {
            let d = (target - self.angle)
                .sin()
                .atan2((target - self.angle).cos());
            if d.abs() < 0.002 {
                self.angle = target.rem_euclid(TAU);
                self.target = None;
            } else {
                self.angle = (self.angle + d * (dt * 7.).min(1.)).rem_euclid(TAU);
            }
        } else if !self.locked && !hover {
            self.angle = (self.angle + dt * 0.19).rem_euclid(TAU);
        }
    }
    pub fn pick(&self, model: &Model, scroll: usize, x: i32, y: i32) -> Option<u32> {
        let visible = labels(model, scroll);
        if let Some(label) = visible.iter().find(|l| l.contains(x, y)) {
            return Some(label.id);
        }
        let p = Projector::new(self.angle);
        visible
            .iter()
            .filter_map(|l| {
                let (px, py, z) = p.project(anchor(model, l.id));
                if z > 0. && (px - x as f32).hypot(py - y as f32) < 16. {
                    Some((l.id, z))
                } else {
                    None
                }
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|a| a.0)
    }
    pub fn draw(&self, p: &mut Painter, model: &Model, scroll: usize, motion: bool) {
        let projection = Projector::new(self.angle);
        let mut cells = vec![
            Cell {
                depth: f32::NEG_INFINITY,
                ch: ' ',
                color: 0
            };
            COLS * ROWS
        ];
        for s in &self.samples {
            let nx = s.nx * projection.cos + s.nz * projection.sin;
            let nz = -s.nx * projection.sin + s.nz * projection.cos;
            let light = (0.42 + nz * 0.38 - nx * 0.17).clamp(0.16, 1.);
            let (x, y, z) = projection.project(s.p);
            let glyph = if s.ch == '.' {
                if light > 0.72 {
                    ':'
                } else if light > 0.47 {
                    '.'
                } else {
                    ','
                }
            } else {
                s.ch
            };
            let base = if s.band { 0xe9ca8c } else { 0xb5a386 };
            write(&mut cells, x, y, z, glyph, tint(base, light));
        }
        let visible = labels(model, scroll);
        // Real parent-child links from the simulation, projected through the same depth buffer.
        for label in &visible {
            let node = model.nodes.iter().find(|n| n.id == label.id).unwrap();
            let end = anchor(model, node.id);
            let start = if let Some(parent) = node.parent {
                anchor(model, parent)
            } else {
                Point {
                    x: 0.,
                    y: 1.14,
                    z: 0.,
                }
            };
            for step in 0..100 {
                let t = step as f32 / 99.;
                let point = Point {
                    x: start.x + (end.x - start.x) * t,
                    y: start.y + (end.y - start.y) * t + (std::f32::consts::PI * t).sin() * 0.16,
                    z: start.z + (end.z - start.z) * t,
                };
                let (x, y, z) = projection.project(point);
                let pulse =
                    motion && node.activity > 0 && ((step as u64 + model.tick * 3) % 32 < 4);
                write(
                    &mut cells,
                    x,
                    y,
                    z + 0.025,
                    if pulse { '*' } else { '+' },
                    tint(
                        color(node.id),
                        if pulse {
                            1.
                        } else if z > 0. {
                            0.7
                        } else {
                            0.25
                        },
                    ),
                );
            }
            let (x, y, z) = projection.project(end);
            write(&mut cells, x, y, z + 0.1, '@', color(node.id));
            // Label leaders stay outside the tower silhouette, including back-facing nodes.
            let edge = if label.x < 300 { 298 } else { 767 };
            let elbow = if label.x < 300 { 327 } else { 730 };
            let ly = label.y + 24;
            for (ax, ay, bx, by) in [
                (x as i32, y as i32, elbow, y as i32),
                (elbow, y as i32, elbow, ly),
                (elbow, ly, edge, ly),
            ] {
                let steps = (bx - ax).abs().max((by - ay).abs()).max(1) / 4 + 1;
                for i in 0..=steps {
                    let px = ax + (bx - ax) * i / steps;
                    let py = ay + (by - ay) * i / steps;
                    write(
                        &mut cells,
                        px as f32,
                        py as f32,
                        -3.,
                        if ay == by { '-' } else { ':' },
                        tint(
                            color(node.id),
                            if node.id == model.selected {
                                0.65
                            } else {
                                0.23
                            },
                        ),
                    );
                }
            }
        }
        // Ground plane: a restrained ASCII ellipse, not an animated backdrop.
        for i in 0..150 {
            let a = i as f32 / 150. * TAU;
            write(
                &mut cells,
                523. + a.cos() * 178.,
                563. + a.sin() * 22.,
                -5.,
                '.',
                0x344446,
            );
        }
        for (i, cell) in cells.iter().enumerate() {
            if cell.ch != ' ' {
                p.text(
                    X + (i % COLS) as i32 * CW,
                    Y + (i / COLS) as i32 * CH + 10,
                    12,
                    2,
                    &cell.ch.to_string(),
                    cell.color,
                );
            }
        }
        for label in visible {
            let node = model.nodes.iter().find(|n| n.id == label.id).unwrap();
            let c = color(node.id);
            let selected = node.id == model.selected;
            p.rect(
                label.x,
                label.y,
                225,
                62,
                if selected { 0x162e32 } else { 0x0b1822 },
            );
            if selected {
                p.border(label.x, label.y, 225, 62, 0x365459);
                p.rect(label.x, label.y, 2, 62, c);
            }
            p.text(
                label.x + 10,
                label.y + 17,
                10,
                2,
                &format!(
                    "{:02} / {}",
                    node.id,
                    if node.paused {
                        "VILA"
                    } else if node.protected {
                        "SYSTEM"
                    } else {
                        "UPPGIFT"
                    }
                ),
                if selected { c } else { MUTED },
            );
            p.text(
                label.x + 10,
                label.y + 37,
                17,
                0,
                &node.name,
                if selected { INK } else { 0xb5c5ce },
            );
            p.text(
                label.x + 10,
                label.y + 53,
                10,
                2,
                &format!(
                    "{} MiB · förälder {}",
                    node.memory,
                    node.parent.map_or("–".into(), |id| format!("{id:02}"))
                ),
                MUTED,
            );
            for (i, v) in node.history.iter().rev().take(20).rev().enumerate() {
                let height = (*v as i32 / 2).min(11);
                p.rect(
                    label.x + 150 + i as i32 * 3,
                    label.y + 53 - height,
                    2,
                    height.max(1),
                    tint(c, 0.65),
                );
            }
        }
        p.text(
            390,
            612,
            11,
            2,
            if !motion {
                "STILLBILD / F3"
            } else if self.locked {
                "FOKUS LÅST / F4 ROTERA"
            } else if self.hovered {
                "PEKAREN HÅLLER TORNET"
            } else {
                "ROTERANDE / 3D ASCII"
            },
            GOLD,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orbit_moves_but_hover_focus_and_reduced_motion_hold() {
        let mut t = Tower::new();
        let start = t.angle;
        t.advance(0.03, true, false);
        assert_ne!(t.angle, start);
        let stop = t.angle;
        t.advance(0.03, true, true);
        assert_eq!(t.angle, stop);
        t.hold();
        t.advance(0.03, true, false);
        assert_eq!(t.angle, stop);
        t.release();
        t.advance(0.03, false, false);
        assert_eq!(t.angle, stop);
        t.focus(4, true);
        for _ in 0..120 {
            t.advance(0.03, true, false);
        }
        assert!((t.angle + theta(4)).sin().abs() < 0.003);
    }
    #[test]
    fn labels_are_stable_and_all_nodes_remain_reachable() {
        let mut m = Model::new();
        let mut t = Tower::new();
        for _ in 0..30 {
            let _ = m.execute("start test 1");
        }
        for scroll in 0..=m.nodes.len() - 8 {
            for l in labels(&m, scroll) {
                for angle in [0., 1., 3., 5.] {
                    t.angle = angle;
                    assert_eq!(t.pick(&m, scroll, l.x + 50, l.y + 30), Some(l.id));
                }
            }
        }
    }
    #[test]
    fn nearest_character_wins_depth_test() {
        let mut c = Cell {
            depth: -10.,
            ch: ' ',
            color: 0,
        };
        c.put(1., 'A', 1);
        c.put(-1., 'B', 2);
        assert_eq!(c.ch, 'A');
        c.put(2., 'C', 3);
        assert_eq!(c.ch, 'C');
    }
}
