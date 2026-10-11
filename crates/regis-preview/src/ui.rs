use crate::paint::*;
use regis_model::{CAPACITY_MIB, Model};

pub struct Ui {
    pub model: Model,
    pub input: String,
    pub scroll: usize,
    pub motion: bool,
    pub render_ms: f64,
    pub status: String,
    pub error: bool,
}
impl Ui {
    pub fn new() -> Self {
        Self {
            model: Model::new(),
            input: String::new(),
            scroll: 0,
            motion: true,
            render_ms: 0.,
            status: "Välj en gren. Skriv ett kommando. Se systemet förändras.".into(),
            error: false,
        }
    }
    pub fn command(&mut self, command: &str) {
        let result = self.model.execute(command);
        self.error = result.is_err();
        self.status = result.unwrap_or_else(|e| e);
        self.keep_visible();
    }
    pub fn submit(&mut self) {
        let command = std::mem::take(&mut self.input);
        if !command.trim().is_empty() {
            self.command(&command);
        }
    }
    pub fn keep_visible(&mut self) {
        let order = self.model.order();
        let i = order
            .iter()
            .position(|(id, _)| *id == self.model.selected)
            .unwrap_or(0);
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + 8 {
            self.scroll = i - 7;
        }
        self.scroll = self.scroll.min(order.len().saturating_sub(8));
    }
    pub fn navigate(&mut self, delta: i32) {
        let order = self.model.order();
        let i = order
            .iter()
            .position(|(id, _)| *id == self.model.selected)
            .unwrap_or(0);
        let next = (i as i32 + delta).clamp(0, order.len() as i32 - 1) as usize;
        self.model.select(order[next].0);
        self.keep_visible();
    }
    pub fn click(&mut self, x: i32, y: i32) {
        if (1260..1390).contains(&x) && (127..156).contains(&y) {
            self.motion = !self.motion;
            return;
        }
        if (54..1005).contains(&x) && (194..594).contains(&y) {
            let row = ((y - 194) / 50) as usize + self.scroll;
            if let Some((id, _)) = self.model.order().get(row) {
                self.model.select(*id);
            }
        } else if (1350..1400).contains(&x) && (169..209).contains(&y) {
            self.command("close");
        } else if (1068..1215).contains(&x) && (531..568).contains(&y) {
            self.command(if self.model.inspector {
                if self.model.selected_node().paused {
                    "resume"
                } else {
                    "pause"
                }
            } else {
                "open"
            });
        } else if (1230..1375).contains(&x) && (531..568).contains(&y) {
            self.command("stop");
        } else if (54..1386).contains(&x) && (666..704).contains(&y) {
            let slot = ((x - 54) as u32 * CAPACITY_MIB / 1332).min(CAPACITY_MIB - 1);
            let mut pos = 0;
            for node in &self.model.nodes {
                pos += node.memory;
                if slot < pos {
                    self.model.selected = node.id;
                    break;
                }
            }
            self.keep_visible();
        } else if (55..225).contains(&x) && (735..762).contains(&y) {
            self.input = "start Aurora 16".into();
        } else if (245..420).contains(&x) && (735..762).contains(&y) {
            self.input = "alloc 8".into();
        } else if (440..620).contains(&x) && (735..762).contains(&y) {
            self.command("open");
        }
    }
    pub fn draw(&self, p: &mut Painter) {
        p.reset();
        // Three crowns: procedural System Regis mark, no bitmap animation.
        for i in 0..3 {
            let x = 58 + i * 15;
            let y = if i == 1 { 38 } else { 44 };
            p.line(x, y, x + 2, y + 9, GOLD);
            p.line(x + 2, y + 9, x + 10, y + 9, GOLD);
            p.line(x + 10, y + 9, x + 12, y, GOLD);
            p.line(x, y, x + 4, y + 4, GOLD);
            p.line(x + 4, y + 4, x + 6, y - 2, GOLD);
            p.line(x + 6, y - 2, x + 8, y + 4, GOLD);
            p.line(x + 8, y + 4, x + 12, y, GOLD);
        }
        p.text(124, 60, 34, 1, "SYSTEM REGIS", INK);
        p.text(126, 85, 14, 0, "Maskinen tillhör dig.", GOLD);
        p.text(55, 116, 11, 2, "E U T H E R U S   /   FORMSTUDIE 01", MUTED);
        let total: u32 = self.model.nodes.iter().map(|n| n.activity).sum();
        p.text(800, 43, 11, 2, "AKTIVITET", MUTED);
        p.text(800, 76, 27, 0, &format!("{}", total), INK);
        p.text(849, 75, 12, 0, "enheter", MUTED);
        p.text(978, 43, 11, 2, "TILLDELAT", MUTED);
        p.text(978, 76, 27, 0, &format!("{}", self.model.used()), INK);
        p.text(1026, 75, 12, 0, "/ 256 MiB", MUTED);
        p.rect(1151, 36, 234, 44, 0x212922);
        p.border(1151, 36, 234, 44, 0x59604b);
        p.text(1166, 55, 12, 2, "SIMULERAT SYSTEM", GOLD);
        p.text(1166, 71, 11, 0, "Ingen styrning av värddatorn", MUTED);
        p.line(54, 125, 1386, 125, EDGE);
        p.text(55, 153, 12, 2, "01 / LEVANDE SYSTEM", GOLD);
        p.text(
            756,
            153,
            11,
            2,
            &format!("{} NODER · 10 Hz DATA", self.model.nodes.len()),
            MUTED,
        );
        p.text(
            1270,
            148,
            12,
            0,
            if self.motion {
                "Rörelse: på"
            } else {
                "Rörelse: av"
            },
            MUTED,
        );
        p.text(111, 181, 10, 2, "TILLHÖRIGHET", MUTED);
        p.text(610, 181, 10, 2, "AKTIVITET", MUTED);
        p.text(898, 181, 10, 2, "MINNE", MUTED);
        let order = self.model.order();
        for (row, (id, depth)) in order.iter().skip(self.scroll).take(8).enumerate() {
            let node = self.model.nodes.iter().find(|n| n.id == *id).unwrap();
            let y = 218 + row as i32 * 50;
            let x = 88 + (*depth).min(5) as i32 * 43;
            let c = color(*id);
            let selected = *id == self.model.selected;
            if selected {
                p.rect(54, y - 24, 950, 47, 0x142c32);
                p.rect(54, y - 24, 3, 47, c);
                p.border(54, y - 24, 950, 47, 0x284c51);
            }
            if let Some(parent) = node.parent {
                if let Some(parent_row) = order.iter().position(|(id, _)| *id == parent) {
                    let parent_y =
                        (218 + (parent_row as i32 - self.scroll as i32) * 50 + 14).max(194);
                    p.line(x - 43, parent_y, x - 43, y, EDGE);
                    p.line(x - 43, y, x - 18, y, EDGE);
                }
            }
            if self.motion && node.activity > 0 {
                p.glow(x, y, 24, c, 15 + node.activity * 2);
            }
            p.ring(x, y, 12, EDGE, 1.);
            p.ring(
                x,
                y,
                12,
                c,
                if *id == 0 {
                    1.
                } else {
                    node.memory as f32 / 48.
                },
            );
            p.rect(x - 2, y - 2, 5, 5, c);
            p.text(
                x + 25,
                y + 1,
                17,
                0,
                &node.name,
                if selected { INK } else { 0xbccbd3 },
            );
            p.text(
                x + 25,
                y + 16,
                10,
                2,
                if node.paused {
                    "VILANDE"
                } else if node.protected {
                    "SYSTEM"
                } else {
                    "UPPGIFT"
                },
                MUTED,
            );
            for (i, value) in node.history.iter().enumerate() {
                let h = (*value as i32).min(25);
                let bx = 611 + i as i32 * 4;
                p.rect(bx, y + 12 - h, 2, h.max(1), if h > 0 { c } else { EDGE });
            }
            p.text(826, y + 5, 12, 2, &format!("{:02}", node.activity), c);
            p.text(
                898,
                y + 5,
                13,
                2,
                &format!("{:>3} MiB", node.memory),
                if selected { INK } else { MUTED },
            );
        }
        if order.len() > 8 {
            p.text(
                60,
                614,
                11,
                0,
                &format!(
                    "{}–{} / {} · rulla eller använd piltangenter",
                    self.scroll + 1,
                    (self.scroll + 8).min(order.len()),
                    order.len()
                ),
                MUTED,
            );
        } else {
            p.text(
                62,
                615,
                12,
                0,
                "Varje gren har ett ursprung. Varje tilldelning har en ägare.",
                MUTED,
            );
        }
        self.inspector(p);
        p.line(54, 638, 1386, 638, EDGE);
        p.text(55, 658, 11, 2, "02 / MINNETS LANDSKAP", GOLD);
        p.text(
            1105,
            658,
            12,
            0,
            &format!(
                "{} MiB ledigt · välj ett fält",
                CAPACITY_MIB - self.model.used()
            ),
            MUTED,
        );
        let mut x = 54;
        let mut allocated = 0;
        for node in &self.model.nodes {
            allocated += node.memory;
            let end = 54 + (allocated * 1332 / CAPACITY_MIB) as i32;
            let width = end - x;
            if width == 0 {
                continue;
            }
            p.rect(x, 675, width - 2, 22, color(node.id));
            if node.id == self.model.selected {
                p.border(x - 1, 669, width, 34, INK);
            }
            x += width;
        }
        for px in (x..1386).step_by(8) {
            p.line(px, 676, (px + 18).min(1386), 695, EDGE);
        }
        p.text(
            55,
            724,
            12,
            0,
            &clip(&self.status, 148),
            if self.error { 0xf1a78b } else { MUTED },
        );
        p.text(55, 752, 12, 2, "+ start Aurora 16", color(4));
        p.text(245, 752, 12, 2, "+ alloc 8", color(5));
        p.text(440, 752, 12, 2, "open", GOLD);
        p.text(1113, 751, 11, 2, "help · visa kommandon", MUTED);
        let c = color(self.model.selected);
        p.glow(100, 804, 60, c, 12);
        p.rect(54, 777, 1332, 65, 0x101e29);
        p.border(54, 777, 1332, 65, 0x36515d);
        p.rect(54, 777, 3, 65, c);
        p.text(77, 802, 10, 2, "KONTEXT", MUTED);
        p.text(77, 825, 15, 0, &self.model.selected_node().name, c);
        p.text(280, 817, 23, 2, "›", c);
        let input = if self.input.is_empty() {
            "Skriv här · start namn MiB · open · stop"
        } else {
            &self.input
        };
        let end = p.text(
            315,
            816,
            17,
            2,
            &clip(input, 90),
            if self.input.is_empty() { MUTED } else { INK },
        );
        if !self.input.is_empty() {
            p.rect(end + 3, 800, 2, 20, c);
        }
        p.text(1269, 816, 11, 2, "ENTER", MUTED);
        p.text(55,874,11,0,"↑ ↓ välj gren    ·    Enter kör    ·    Esc töm    ·    F2 öppna    ·    F3 rörelse    ·    F10 bild",MUTED);
        p.text(
            1070,
            874,
            11,
            2,
            &format!("RGB / CPU   {:.2} ms   30 fps max", self.render_ms),
            MUTED,
        );
    }
    fn inspector(&self, p: &mut Painter) {
        let node = self.model.selected_node();
        let c = color(node.id);
        p.rect(1040, 169, 346, 436, 0x101d29);
        p.border(1040, 169, 346, 436, EDGE);
        p.rect(1040, 169, 346, 2, c);
        p.text(1068, 200, 11, 2, &format!("FOKUS / {:03}", node.id), c);
        p.text(1360, 197, 17, 0, "×", MUTED);
        p.text(1068, 240, 30, 1, &node.name, INK);
        if !self.model.inspector {
            p.text(1068, 286, 16, 0, "En gren. Ett eget rum.", INK);
            p.text(1068, 315, 13, 0, "Öppna noden för att se dess", MUTED);
            p.text(1068, 337, 13, 0, "aktivitet och tilldelningar.", MUTED);
            p.border(1068, 531, 147, 37, c);
            p.text(1090, 555, 13, 0, "Öppna", c);
            return;
        }
        p.text(
            1068,
            264,
            12,
            0,
            if node.paused {
                "Vilar · minnet behålls"
            } else {
                "Aktiv · simulerad uppgift"
            },
            MUTED,
        );
        p.text(1068, 303, 10, 2, "AKTIVITET / SENASTE 4,8 SEKUNDER", MUTED);
        for (i, v) in node.history.iter().enumerate() {
            let h = *v as i32 * 2;
            let x = 1069 + i as i32 * 6;
            p.rect(x, 372 - h, 4, h.max(1), c);
        }
        p.line(1068, 373, 1356, 373, EDGE);
        p.text(1068, 407, 11, 2, "ÄGT MINNE", MUTED);
        p.text(1068, 439, 28, 0, &format!("{} MiB", node.memory), INK);
        p.text(1240, 407, 11, 2, "FÖRÄLDER", MUTED);
        let parent = node
            .parent
            .and_then(|id| self.model.nodes.iter().find(|n| n.id == id));
        p.text(
            1240,
            433,
            14,
            0,
            &clip(parent.map(|n| n.name.as_str()).unwrap_or("Rot"), 13),
            c,
        );
        p.text(1068, 474, 12, 0, "Tilldelning är simulerat ägande.", MUTED);
        p.text(
            1068,
            494,
            12,
            0,
            "Stopp återlämnar nodens hela gren.",
            MUTED,
        );
        p.border(1068, 531, 147, 37, if node.protected { EDGE } else { c });
        p.text(
            1090,
            555,
            13,
            0,
            if node.paused { "Återuppta" } else { "Pausa" },
            if node.protected { MUTED } else { c },
        );
        p.border(1230, 531, 126, 37, EDGE);
        p.text(
            1250,
            555,
            13,
            0,
            "Stoppa",
            if node.protected { MUTED } else { 0xe9a68c },
        );
        p.text(1068, 590, 10, 2, "SAMMA NOD · EN ANNAN VY", MUTED);
    }
}
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        s.chars()
            .take(max - 1)
            .chain(core::iter::once('…'))
            .collect()
    } else {
        s.into()
    }
}
