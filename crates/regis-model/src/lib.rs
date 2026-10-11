//! Deterministic simulated telemetry, not an OS process manager.
//! No I/O, clock, host process access, or execution authority.
#![no_std]
extern crate alloc;
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

pub const CAPACITY_MIB: u32 = 256;
pub const MAX_NODES: usize = 24;
#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub memory: u32,
    pub paused: bool,
    pub protected: bool,
    pub activity: u32,
    pub history: [u32; 48],
}
#[derive(Clone, Debug)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub selected: u32,
    pub inspector: bool,
    pub tick: u64,
    pub events: Vec<String>,
    next_id: u32,
}
impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}
impl Model {
    pub fn new() -> Self {
        let mut model = Self {
            nodes: Vec::new(),
            selected: 4,
            inspector: true,
            tick: 0,
            next_id: 7,
            events: vec!["System Regis vaknar · simulerat system".into()],
        };
        for (id, parent, name, memory, protected) in [
            (0, None, "System Regis", 0, true),
            (1, Some(0), "Kärna", 24, true),
            (2, Some(0), "Tjänster", 12, true),
            (3, Some(0), "Verkstad", 4, true),
            (4, Some(3), "Harmonics", 24, false),
            (5, Some(3), "Prisma", 16, false),
            (6, Some(3), "Arkiv", 8, false),
        ] {
            model.nodes.push(Node {
                id,
                parent,
                name: name.into(),
                memory,
                paused: false,
                protected,
                activity: 0,
                history: [0; 48],
            });
        }
        model
    }
    pub fn used(&self) -> u32 {
        self.nodes.iter().map(|n| n.memory).sum()
    }
    pub fn selected_node(&self) -> &Node {
        self.nodes
            .iter()
            .find(|n| n.id == self.selected)
            .expect("selected node exists")
    }
    pub fn select(&mut self, id: u32) -> bool {
        if self.nodes.iter().any(|n| n.id == id) {
            self.selected = id;
            true
        } else {
            false
        }
    }
    pub fn order(&self) -> Vec<(u32, usize)> {
        fn walk(m: &Model, id: u32, depth: usize, output: &mut Vec<(u32, usize)>) {
            output.push((id, depth));
            for n in m.nodes.iter().filter(|n| n.parent == Some(id)) {
                walk(m, n.id, depth + 1, output);
            }
        }
        let mut order = Vec::new();
        walk(self, 0, 0, &mut order);
        order
    }
    pub fn advance(&mut self) {
        self.tick += 1;
        for n in &mut self.nodes {
            // Integer triangular waves: deterministic, bounded and cheap.
            let phase = ((self.tick * (1 + n.id as u64 % 3) + n.id as u64 * 13) % 80) as u32;
            let wave = if phase < 40 { phase } else { 80 - phase };
            n.activity = if n.paused || n.id == 0 {
                0
            } else {
                2 + wave / (2 + n.id % 3)
            };
            n.history.rotate_left(1);
            n.history[47] = n.activity;
        }
    }
    pub fn execute(&mut self, command: &str) -> Result<String, String> {
        let parts: Vec<_> = command.split_whitespace().collect();
        let result = self.apply(&parts);
        let message = match &result {
            Ok(s) => s.clone(),
            Err(s) => format!("Avvisat: {s}"),
        };
        self.events.push(message);
        if self.events.len() > 5 {
            self.events.remove(0);
        }
        result
    }
    fn amount(text: Option<&&str>) -> Result<u32, String> {
        let value = text
            .ok_or("Ange minne i MiB")?
            .parse::<u32>()
            .map_err(|_| "Ange ett positivt heltal")?;
        if value == 0 || value > CAPACITY_MIB {
            return Err("Minne måste vara 1–256 MiB".into());
        }
        Ok(value)
    }
    fn apply(&mut self, p: &[&str]) -> Result<String, String> {
        let usage = "start namn MiB · stop · pause · resume · alloc MiB · free MiB · open · close";
        match p.first().copied().unwrap_or("") {
            "help" if p.len() == 1 => Ok(usage.into()),
            "open" | "öppna" if p.len() == 1 => {
                self.inspector = true;
                Ok(format!("Öppnade {}", self.selected_node().name))
            }
            "close" | "stäng" if p.len() == 1 => {
                self.inspector = false;
                Ok("Detaljvyn stängd".into())
            }
            "start" | "starta" if p.len() == 3 => {
                let memory = Self::amount(p.get(2))?;
                if self.nodes.len() >= MAX_NODES {
                    return Err("Trädet har nått prototypens 24 noder".into());
                }
                if p[1].chars().count() > 18
                    || !p[1]
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    return Err("Namn: 1–18 bokstäver, siffror, - eller _".into());
                }
                if self.used() + memory > CAPACITY_MIB {
                    return Err("Otillräckligt simulerat minne".into());
                }
                if self.selected_node().paused {
                    return Err("Återuppta föräldern först".into());
                }
                let id = self.next_id;
                self.next_id += 1;
                self.nodes.push(Node {
                    id,
                    parent: Some(self.selected),
                    name: p[1].into(),
                    memory,
                    protected: false,
                    paused: false,
                    activity: 0,
                    history: [0; 48],
                });
                self.selected = id;
                Ok(format!("{} startad · +{} MiB", p[1], memory))
            }
            "stop" | "stoppa" if p.len() == 1 => {
                let node = self.selected_node();
                if node.protected {
                    return Err("Systemnoden är skyddad i prototypen".into());
                }
                let parent = node.parent.unwrap_or(0);
                let name = node.name.clone();
                let mut ids = vec![node.id];
                loop {
                    let before = ids.len();
                    for n in &self.nodes {
                        if n.parent.is_some_and(|id| ids.contains(&id)) && !ids.contains(&n.id) {
                            ids.push(n.id);
                        }
                    }
                    if ids.len() == before {
                        break;
                    }
                }
                let released: u32 = self
                    .nodes
                    .iter()
                    .filter(|n| ids.contains(&n.id))
                    .map(|n| n.memory)
                    .sum();
                self.nodes.retain(|n| !ids.contains(&n.id));
                self.selected = parent;
                Ok(format!("{} stoppad · {} MiB återlämnat", name, released))
            }
            "alloc" if p.len() == 2 => {
                let amount = Self::amount(p.get(1))?;
                if self.used() + amount > CAPACITY_MIB {
                    return Err("Otillräckligt simulerat minne".into());
                }
                self.nodes
                    .iter_mut()
                    .find(|n| n.id == self.selected)
                    .unwrap()
                    .memory += amount;
                Ok(format!("+{amount} MiB tilldelat"))
            }
            "free" if p.len() == 2 => {
                let amount = Self::amount(p.get(1))?;
                let node = self
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == self.selected)
                    .unwrap();
                if node.protected {
                    return Err("Systemnodens minne är skyddat".into());
                }
                if amount > node.memory {
                    return Err("Noden äger inte så mycket minne".into());
                }
                node.memory -= amount;
                Ok(format!("{amount} MiB återlämnat"))
            }
            "pause" | "resume" if p.len() == 1 => {
                let node = self
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == self.selected)
                    .unwrap();
                if node.protected {
                    return Err("Systemnoden är skyddad i prototypen".into());
                }
                node.paused = p[0] == "pause";
                if node.paused {
                    node.activity = 0;
                }
                Ok(if node.paused {
                    "Uppgiften vilar"
                } else {
                    "Uppgiften återupptagen"
                }
                .to_string())
            }
            _ => Err(format!("Okänt kommando. {usage}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subtree_stop_returns_exact_memory_and_focus() {
        let mut m = Model::new();
        let before = m.used();
        let parent = m.selected;
        m.execute("start test 16").unwrap();
        let child = m.selected;
        m.execute("start nested 8").unwrap();
        m.execute("alloc 4").unwrap();
        assert_eq!(m.used(), before + 28);
        m.select(child);
        m.execute("stop").unwrap();
        assert_eq!(m.used(), before);
        assert_eq!(m.selected, parent);
    }
    #[test]
    fn rejected_commands_do_not_mutate_resources() {
        let mut m = Model::new();
        let before = m.used();
        let len = m.nodes.len();
        for cmd in [
            "start huge 256",
            "alloc 4294967295",
            "free 25",
            "start a 0",
            "rm -rf /",
            "stop extra",
        ] {
            assert!(m.execute(cmd).is_err(), "{cmd}");
            assert_eq!(m.used(), before);
            assert_eq!(m.nodes.len(), len);
        }
        m.select(0);
        assert!(m.execute("stop").is_err());
        assert_eq!(m.nodes.len(), len);
    }
    #[test]
    fn pause_resumes_without_losing_memory() {
        let mut m = Model::new();
        let before = m.used();
        m.execute("pause").unwrap();
        for _ in 0..100 {
            m.advance();
            assert_eq!(m.selected_node().activity, 0);
        }
        m.execute("resume").unwrap();
        m.advance();
        assert!(m.selected_node().activity > 0);
        assert_eq!(m.used(), before);
    }
    #[test]
    fn samples_replay_deterministically_and_model_is_bounded() {
        let mut a = Model::new();
        let mut b = a.clone();
        for _ in 0..1000 {
            a.advance();
            b.advance();
        }
        assert_eq!(a.selected_node().history, b.selected_node().history);
        for _ in 0..100 {
            let _ = a.execute("start a 1");
        }
        assert_eq!(a.nodes.len(), MAX_NODES);
        assert!(a.used() <= CAPACITY_MIB);
        assert!(a.events.len() <= 5);
    }
}
