mod paint;
mod present;
mod tower;
mod ui;
use minifb::{InputCallback, Key, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};
use paint::{H, Painter, W};
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};
use ui::Ui;

enum Input {
    Character(char),
    Key(Key),
}
struct TextInput(Rc<RefCell<Vec<Input>>>);
impl InputCallback for TextInput {
    fn add_char(&mut self, unicode: u32) {
        if let Some(ch) = char::from_u32(unicode).filter(|c| !c.is_control()) {
            self.0.borrow_mut().push(Input::Character(ch));
        }
    }
    fn set_key_state(&mut self, key: Key, down: bool) {
        if down {
            self.0.borrow_mut().push(Input::Key(key));
        }
    }
}
fn state(ui: &Ui) -> serde_json::Value {
    serde_json::json!({"simulated":true,"selected":ui.model.selected,"memory_mib":ui.model.used(),
        "tower_angle":ui.tower.angle,"tower_locked":ui.tower.locked,"motion":ui.motion,
        "labels":tower::labels(&ui.model,ui.scroll).iter().map(|l|serde_json::json!({"id":l.id,"x":l.x,"y":l.y})).collect::<Vec<_>>(),
        "inspector":ui.model.inspector,"status":ui.status,"input":ui.input,
        "nodes":ui.model.nodes.iter().map(|n| serde_json::json!({"id":n.id,"name":n.name,"parent":n.parent,"memory_mib":n.memory,"paused":n.paused})).collect::<Vec<_>>()})
}
fn save_json(path: &Path, value: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}
fn verify(ui: &mut Ui) -> Result<(), Box<dyn std::error::Error>> {
    let initial = ui.model.used();
    ui.click(170, 424); // Harmonics: stable branch label hit-testing.
    assert_eq!(ui.model.selected, 4);
    ui.click(100, 748); // Suggested command fills the same prompt as typing.
    assert_eq!(ui.input, "start Aurora 16");
    ui.submit();
    assert_eq!(ui.model.used(), initial + 16);
    let child = ui.model.selected;
    ui.input = "alloc 8".into();
    ui.submit();
    assert_eq!(ui.model.used(), initial + 24);
    ui.click(1100, 550);
    assert!(ui.model.selected_node().paused);
    ui.model.advance();
    assert_eq!(ui.model.selected_node().activity, 0);
    ui.click(1100, 550);
    assert!(!ui.model.selected_node().paused);
    ui.click(1370, 185);
    assert!(!ui.model.inspector);
    ui.click(1100, 550);
    assert!(ui.model.inspector);
    ui.click(1280, 550);
    assert!(!ui.model.nodes.iter().any(|n| n.id == child));
    assert_eq!(ui.model.used(), initial);
    assert_eq!(ui.model.selected, 4);
    ui.input = "start too-large 256".into();
    ui.submit();
    assert!(ui.error);
    assert_eq!(ui.model.used(), initial);
    // Node-bound and scrolling remain usable with the fullest supported tree.
    for _ in 0..30 {
        ui.command("start leaf 1");
    }
    assert_eq!(ui.model.nodes.len(), regis_model::MAX_NODES);
    assert!(ui.scroll > 0);
    ui.navigate(-1);
    ui.command("open");
    *ui = Ui::new();
    for _ in 0..48 {
        ui.model.advance();
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let value = |flag: &str| -> Option<&str> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    };
    if args.iter().any(|a| a == "--help") {
        println!(
            "System Regis native Rust form study (SIMULATED DATA)\nregis-preview [--headless FILE.png] [--verify] [--benchmark] [--frames N] [--state-file FILE.json]\nF2 inspector · F3 reduced motion · F4 orbit · F10 screenshot · Ctrl+Q quit"
        );
        return Ok(());
    }
    let mut ui = Ui::new();
    let mut painter = Painter::new();
    if args.iter().any(|a| a == "--verify") {
        verify(&mut ui)?;
    } else {
        for _ in 0..48 {
            ui.model.advance();
        }
    }
    if args.iter().any(|a| a == "--benchmark") {
        let mut times = Vec::new();
        for i in 0..630 {
            if i % 3 == 0 {
                ui.model.advance();
            }
            ui.animate(1. / 30., false);
            let t = Instant::now();
            ui.draw(&mut painter);
            if i >= 30 {
                times.push(t.elapsed().as_secs_f64() * 1000.);
            }
        }
        times.sort_by(f64::total_cmp);
        let rss = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("VmRSS:"))
                    .map(str::to_owned)
            });
        let report = serde_json::json!({"simulation":true,"resolution":[W,H],"renderer":"CPU RGB, cached glyphs and background","samples":times.len(),
            "render_ms_median":times[times.len()/2],"render_ms_p95":times[times.len()*95/100],"render_ms_max":times[times.len()-1],
            "render_budget_ms":8.0,"within_budget":times[times.len()*95/100]<8.0,"rss":rss,
            "excludes":"window presentation and compositor; host measurements, not guest performance"});
        save_json(Path::new("artifacts/regis/benchmark.json"), &report)?;
        println!("{report}");
    }
    if let Some(path) = value("--headless") {
        ui.draw(&mut painter);
        painter.save(Path::new(path))?;
        save_json(
            Path::new("artifacts/regis/headless-state.json"),
            &state(&ui),
        )?;
        println!(
            "{{\"headless\":true,\"verified\":{},\"screenshot\":{}}}",
            args.iter().any(|a| a == "--verify"),
            serde_json::to_string(path)?
        );
        return Ok(());
    }
    let frame_limit = value("--frames").map(str::parse::<usize>).transpose()?;
    let state_file = value("--state-file").map(Path::new);
    let mut window = Window::new(
        "System Regis — EutherUS / SIMULERAD PROTOTYP",
        W,
        H,
        WindowOptions {
            resize: true,
            scale_mode: ScaleMode::Stretch,
            ..WindowOptions::default()
        },
    )?;
    window.set_target_fps(30);
    let chars = Rc::new(RefCell::new(Vec::new()));
    window.set_input_callback(Box::new(TextInput(chars.clone())));
    let mut presentation = present::Presentation::new();
    let mut previous_mouse = false;
    let mut frame = 0;
    let mut last_tick = Instant::now();
    let mut last_frame = Instant::now();
    while window.is_open() && frame_limit.is_none_or(|limit| frame < limit) {
        let began = Instant::now();
        if window.is_key_down(Key::LeftCtrl) && window.is_key_down(Key::Q) {
            break;
        }
        for event in chars.borrow_mut().drain(..) {
            let key = match event {
                Input::Character(ch) => {
                    ui.tower.hold();
                    if ui.input.chars().count() < 90 {
                        ui.input.push(ch);
                    }
                    continue;
                }
                Input::Key(key) => key,
            };
            match key {
                Key::Enter => ui.submit(),
                Key::Backspace => {
                    ui.input.pop();
                }
                Key::Escape => ui.input.clear(),
                Key::Up => ui.navigate(-1),
                Key::Down => ui.navigate(1),
                Key::F2 => ui.command(if ui.model.inspector { "close" } else { "open" }),
                Key::F3 => ui.motion = !ui.motion,
                Key::F4 => ui.tower.release(),
                Key::F10 => {
                    painter.save(Path::new("artifacts/regis/live.png"))?;
                }
                _ => {}
            }
        }
        let (ww, hh) = window.get_size();
        let pointer = window
            .get_unscaled_mouse_pos(MouseMode::Discard)
            .and_then(|(x, y)| present::Viewport::new(ww, hh).pointer(x, y));
        let hover =
            pointer.is_some_and(|(x, y)| (54..1005).contains(&x) && (188..622).contains(&y));
        ui.animate(last_frame.elapsed().as_secs_f32(), hover);
        last_frame = Instant::now();
        let mouse = window.get_mouse_down(MouseButton::Left);
        if mouse && !previous_mouse {
            if let Some((x, y)) = window.get_unscaled_mouse_pos(MouseMode::Discard) {
                let (ww, hh) = window.get_size();
                if let Some((px, py)) = present::Viewport::new(ww, hh).pointer(x, y) {
                    ui.click(px, py);
                }
            }
        }
        previous_mouse = mouse;
        if let Some((_, dy)) = window.get_scroll_wheel() {
            if dy > 0. {
                ui.scroll = ui.scroll.saturating_sub(1);
            } else if dy < 0. {
                ui.scroll = (ui.scroll + 1).min(ui.model.nodes.len().saturating_sub(8));
            }
        }
        if last_tick.elapsed() >= Duration::from_millis(100) {
            ui.model.advance();
            last_tick = Instant::now();
            if let Some(path) = state_file {
                save_json(path, &state(&ui))?;
            }
        }
        ui.draw(&mut painter);
        let (ww, hh) = window.get_size();
        if (ww, hh) == (W, H) {
            ui.render_ms = began.elapsed().as_secs_f64() * 1000.;
            window.update_with_buffer(&painter.pixels, W, H)?;
        } else if ww > 0 && hh > 0 {
            presentation.fit(&painter.pixels, ww, hh)?;
            ui.render_ms = began.elapsed().as_secs_f64() * 1000.;
            window.update_with_buffer(&presentation.pixels, ww, hh)?;
        } else {
            window.update();
        }
        frame += 1;
    }
    if let Some(path) = state_file {
        save_json(path, &state(&ui))?;
    }
    Ok(())
}
