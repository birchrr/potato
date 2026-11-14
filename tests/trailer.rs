//! Films the real window (fake accounts from src/system_demo.rs) being used, for the trailer: every frame at
//! 60 fps to target/trailer/NNNNN.png with a big cursor drawn in, each click on a beat of a 117.5 BPM song, and
//! target/trailer/beats.json saying when each step happened. Needs a GPU or lavapipe:
//! cargo test --test trailer -- --ignored
#![cfg(not(windows))]

use eframe::egui::{self, Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, Role, TouchPhase, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

const FPS: f32 = 60.0;
const BEAT: f32 = 60.0 / 117.5;
const PPP: f32 = 1.5;   // 1280 x 720 points -> 1920 x 1080 pixels

struct Film {
    h: Harness<'static, potato::app::App>,
    frame: u32,
    cursor: Pos2,
    press: u32,
    dir: String,
    marks: Vec<(String, f32)>,
}

impl Film {
    fn shot(&mut self) {
        self.h.step();
        let mut img = self.h.render().expect("render");
        draw_cursor(&mut img, self.cursor, if self.press > 0 { 0.84 } else { 1.0 });
        self.press = self.press.saturating_sub(1);
        img.save(format!("{}/{:05}.png", self.dir, self.frame)).expect("save");
        self.frame += 1;
    }

    fn until(&mut self, beat: f32) {
        while (self.frame as f32) < beat * BEAT * FPS {
            self.shot();
        }
    }

    /// Glide to a point, eased in and out, arriving on the given beat.
    fn glide(&mut self, to: Pos2, arrive: f32) {
        let (from, start) = (self.cursor, self.frame as f32);
        let end = arrive * BEAT * FPS;
        while (self.frame as f32) < end {
            let t = ((self.frame as f32 - start + 1.0) / (end - start)).clamp(0.0, 1.0);
            let e = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
            // a slight arc: the path bows up a little on the way
            let mut p = from + (to - from) * e;
            p.y -= (std::f32::consts::PI * e).sin() * (to - from).length() * 0.08;
            self.cursor = p;
            self.h.event(Event::PointerMoved(p));
            self.shot();
        }
        self.cursor = to;
    }

    fn click(&mut self, name: &str) {
        let p = self.cursor;
        self.marks.push((name.to_string(), self.frame as f32 / FPS));
        self.press = (0.1 * FPS) as u32;   // the cursor squashes for a tenth of a second
        self.h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        self.shot();
        self.h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        self.shot();
    }

    fn at(&self, role: Role, label: &str) -> Pos2 {
        self.h.get_by_role_and_label(role, label).rect().center()
    }

    /// The switch at the right end of a setting's row.
    fn switch(&self, label: &str) -> Pos2 {
        let r = self.h.get_by_role_and_label(Role::Switch, label).rect();
        pos2(r.right() - 40.0, r.center().y)
    }

    fn label(&self, label: &str) -> Pos2 {
        self.h.query_all_by_label(label).last().expect(label).rect().center()
    }

    /// Scroll the page under the cursor down by `points`, eased, arriving on the given beat.
    fn scroll(&mut self, points: f32, arrive: f32) {
        let (start, end) = (self.frame as f32, arrive * BEAT * FPS);
        let mut done = 0.0;
        while (self.frame as f32) < end {
            let t = ((self.frame as f32 - start + 1.0) / (end - start)).clamp(0.0, 1.0);
            let e = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
            let step = points * e - done;
            done += step;
            self.h.event(Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, -step), phase: TouchPhase::Move,
                                            source: MouseWheelSource::Unknown, modifiers: Modifiers::NONE });
            self.shot();
        }
    }

    /// Select what's in the box, then type one character per beat from `first`, like a person would.
    fn type_text(&mut self, text: &str, first: f32) {
        self.h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        self.shot();
        for (i, c) in text.chars().enumerate() {
            self.until(first + i as f32);
            self.h.event(Event::Text(c.to_string()));
            self.shot();
        }
    }
}

/// A normal-size arrow cursor, white with a black edge, tip at `p` (in points).
fn draw_cursor(img: &mut image::RgbaImage, p: Pos2, scale: f32) {
    let shape = [(0.0, 0.0), (0.0, 34.0), (8.5, 26.0), (14.5, 39.0), (20.5, 36.5), (14.5, 23.5), (26.0, 23.5)];
    let s = 0.8 * scale * PPP;
    let (ox, oy) = (p.x * PPP, p.y * PPP);
    let pts: Vec<(f32, f32)> = shape.iter().map(|(x, y)| (ox + x * s, oy + y * s)).collect();
    let inside = |x: f32, y: f32, poly: &[(f32, f32)]| {
        let mut c = false;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                c = !c;
            }
        }
        c
    };
    let (w, h) = img.dimensions();
    for y in (oy - 8.0).max(0.0) as u32..((oy + 45.0 * s).min(h as f32 - 1.0)) as u32 {
        for x in (ox - 8.0).max(0.0) as u32..((ox + 32.0 * s).min(w as f32 - 1.0)) as u32 {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if inside(fx, fy, &pts) {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            } else if [(-1.6, 0.0), (1.6, 0.0), (0.0, -1.6), (0.0, 1.6), (-1.1, -1.1), (1.1, -1.1), (-1.1, 1.1), (1.1, 1.1)]
                .iter().any(|(dx, dy)| inside(fx + dx, fy + dy, &pts)) {
                img.put_pixel(x, y, image::Rgba([10, 10, 10, 255]));
            }
        }
    }
}

#[test]
#[ignore]
fn film_the_trailer() {
    // the demo's settings start fresh, with the trim timer off: filming takes minutes of real time, and a timed
    // trim firing mid-shoot would leave nothing for the Trim All click to free
    let settings = std::env::temp_dir().join("Potato");
    let _ = std::fs::remove_dir_all(&settings);
    std::fs::create_dir_all(&settings).unwrap();
    std::fs::write(settings.join("settings.json"), r#"{"trimMinutes": 0}"#).unwrap();
    let dir = format!("{}/target/trailer", env!("CARGO_MANIFEST_DIR"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::builder().with_size([1280.0, 720.0]).with_pixels_per_point(PPP).with_step_dt(1.0 / FPS)
        .with_accessibility_check(false).with_render_cursor(false).build_eframe(|cc| potato::app::App::new(cc));
    h.ctx.set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
    h.run_steps(8);
    let mut f = Film { h, frame: 0, cursor: pos2(1320.0, 780.0), press: 0, dir: dir.clone(), marks: Vec::new() };

    // one click per beat inside a group of settings, two beats to move between groups
    // 1. switch to the Roblox account
    let p = f.label("Roblox"); f.glide(p, 2.0); f.click("account");
    // 2. type a new FPS cap, a digit per beat
    let p = f.at(Role::TextInput, "FPS Cap"); f.glide(p, 4.0); f.click("fps");
    f.type_text("25", 5.0);
    // 3. flip Ultra Low on, and Open Maximized off and back on
    let p = f.switch("Ultra Low"); f.glide(p, 8.0); f.click("ultra");
    let p = f.switch("Open Maximized"); f.glide(p, 9.0); f.click("maximized-off");
    f.until(10.0); f.click("maximized-on");
    // 4. the Main account: lower its priority
    let p = f.label("Main"); f.glide(p, 12.0); f.click("main");
    let combo = f.at(Role::ComboBox, "Priority"); f.glide(combo, 13.0); f.click("priority"); f.until(13.25);
    let p = f.label("Low"); f.glide(p, 14.0); f.click("low");
    // 5. Whole PC: trim every 10 minutes
    let p = f.label("Whole PC"); f.glide(p, 16.0); f.click("wholepc");
    let combo = f.at(Role::ComboBox, "Trim Roblox RAM"); f.glide(combo, 17.0); f.click("trim-dropdown"); f.until(17.25);
    let p = f.label("Every 10 Min"); f.glide(p, 18.0); f.click("trim-10min");
    // 6. Trim All Roblox, then hold on the result
    let p = f.at(Role::Button, "Trim All Roblox"); f.glide(p, 20.0); f.click("trim-all");
    // 7. Alt1: cap its CPU, then further down restart it past 2500 MB, rejoin after crashes and mute it
    let p = f.label("Alt1"); f.glide(p, 22.0); f.click("alt1");
    let p = f.at(Role::TextInput, "CPU Limit"); f.glide(p, 23.0); f.click("cpu-limit");
    f.type_text("25", 24.0);
    let p = pos2(760.0, 420.0); f.glide(p, 26.0); f.scroll(330.0, 27.0);
    let p = f.at(Role::TextInput, "Restart Memory Hogs"); f.glide(p, 28.0); f.click("ram-limit");
    f.type_text("2500", 29.0);
    let p = f.switch("Rejoin After A Crash"); f.glide(p, 33.0); f.click("rejoin");
    let p = f.switch("Mute"); f.glide(p, 34.0); f.click("mute");
    // 8. Whole PC: cap uploads, turn the PC off at 6am, paste a webhook and send a report
    let p = f.label("Whole PC"); f.glide(p, 36.0); f.click("wholepc-2");
    let p = pos2(760.0, 420.0); f.glide(p, 37.0); f.scroll(420.0, 38.0);
    let p = f.at(Role::TextInput, "Upload Cap"); f.glide(p, 39.0); f.click("upload");
    f.type_text("512", 40.0);
    let p = f.at(Role::TextInput, "Turn PC Off At"); f.glide(p, 43.0); f.click("shutdown");
    f.type_text("6am", 44.0);
    let p = f.at(Role::TextInput, "Webhook"); f.glide(p, 47.0); f.click("webhook");
    f.until(48.0);
    f.h.event(Event::Paste("https://discord.com/api/webhooks/1438203347712/potato-trailer".into()));
    f.shot();
    let p = f.at(Role::Button, "Test It"); f.glide(p, 50.0); f.click("send-report");
    f.glide(pos2(1320.0, 820.0), 51.5);
    f.until(52.5);

    let json = serde_json::to_string_pretty(&f.marks.iter().map(|(n, t)| serde_json::json!({"step": n, "at": t})).collect::<Vec<_>>()).unwrap();
    std::fs::write(format!("{dir}/beats.json"), json).unwrap();
}
