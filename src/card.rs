//! The report image Discord shows: big numbers, RAM and CPU graphs since the last report, and a row per
//! account. Drawn on the CPU (tiny-skia shapes, Geist text through ab_glyph), so it works in any login.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use tiny_skia::{Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, Rect, SpreadMode, Stroke, Transform};

use crate::report::{Account, Pc, Sample};

const W: f32 = 1600.0;
const PAD: f32 = 48.0;
const GAP: f32 = 24.0;
const ROW: f32 = 64.0;

const BG: u32 = 0x0A0A0A;
const PANEL: u32 = 0x151515;
const LINE: u32 = 0x262626;
const TEXT: u32 = 0xF2F2F2;
const DIM: u32 = 0xB4B4B4;
const FAINT: u32 = 0x8E8E8E;
const POTATO: u32 = 0xD9A35B;
const GOOD: u32 = 0x4ADE80;
const WARN: u32 = 0xF5B041;
const GREY: u32 = 0x5A5A5A;

fn color(rgb: u32, a: f32) -> Color {
    Color::from_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, (a * 255.0) as u8)
}

fn paint(rgb: u32, a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(rgb, a));
    p.anti_alias = true;
    p
}

struct Canvas {
    px: Pixmap,
    regular: FontRef<'static>,
    semi: FontRef<'static>,
}

impl Canvas {
    fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, fill: u32, stroke: Option<u32>) {
        let mut pb = PathBuilder::new();
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.quad_to(x, y + h, x, y + h - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.close();
        let path = pb.finish().unwrap();
        self.px.fill_path(&path, &paint(fill, 1.0), FillRule::Winding, Transform::identity(), None);
        if let Some(s) = stroke {
            self.px.stroke_path(&path, &paint(s, 1.0), &Stroke { width: 2.0, ..Stroke::default() }, Transform::identity(), None);
        }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: u32, a: f32) {
        if let Some(r) = Rect::from_xywh(x, y, w.max(0.5), h.max(0.5)) {
            self.px.fill_rect(r, &paint(rgb, a), Transform::identity(), None);
        }
    }

    fn width(&self, text: &str, size: f32, semi: bool) -> f32 {
        let font = if semi { &self.semi } else { &self.regular };
        let f = font.as_scaled(PxScale::from(size));
        let mut w = 0.0;
        let mut last = None;
        for c in text.chars() {
            let id = f.glyph_id(c);
            if let Some(l) = last {
                w += f.kern(l, id);
            }
            w += f.h_advance(id);
            last = Some(id);
        }
        w
    }

    /// Text with its top-left at (x, y); `align` 0 = left, 0.5 = centre, 1 = right of x.
    fn text(&mut self, text: &str, x: f32, y: f32, size: f32, semi: bool, rgb: u32, align: f32) {
        let x = x - self.width(text, size, semi) * align;
        let font = if semi { self.semi.clone() } else { self.regular.clone() };
        let f = font.as_scaled(PxScale::from(size));
        let (w, h) = (self.px.width() as i32, self.px.height() as i32);
        let (r, g, b) = ((rgb >> 16) as f32, ((rgb >> 8) & 0xFF) as f32, (rgb & 0xFF) as f32);
        let mut caret = x;
        let mut last = None;
        let pixels = self.px.pixels_mut();
        for c in text.chars() {
            let id = f.glyph_id(c);
            if let Some(l) = last {
                caret += f.kern(l, id);
            }
            let glyph = id.with_scale_and_position(size, point(caret, y + f.ascent()));
            caret += f.h_advance(id);
            last = Some(id);
            let Some(outline) = font.outline_glyph(glyph) else { continue };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let (px, py) = (bounds.min.x as i32 + gx as i32, bounds.min.y as i32 + gy as i32);
                if px < 0 || py < 0 || px >= w || py >= h || cov <= 0.0 {
                    return;
                }
                let a = cov.min(1.0);
                let i = (py * w + px) as usize;
                let d = pixels[i].demultiply();
                let mix = |s: f32, d: u8| (s * a + d as f32 * (1.0 - a)).round() as u8;
                pixels[i] = tiny_skia::ColorU8::from_rgba(mix(r, d.red()), mix(g, d.green()), mix(b, d.blue()), 255).premultiply();
            });
        }
    }

    /// Potato's pixel logo from assets/logo.txt, `cell` pixels per logo pixel.
    fn logo(&mut self, x: f32, y: f32, cell: f32) {
        let src = include_str!("../assets/logo.txt");
        let mut colors = std::collections::HashMap::new();
        let mut row = 0.0;
        for line in src.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            if let Some((k, v)) = line.split_once('=') {
                let hex = v.split_whitespace().next().unwrap_or("").trim_start_matches('#');
                if let Ok(c) = u32::from_str_radix(hex, 16) {
                    colors.insert(k.trim().chars().next().unwrap_or(' '), c);
                }
                continue;
            }
            for (col, ch) in line.chars().enumerate() {
                if let Some(&c) = colors.get(&ch) {
                    self.rect(x + col as f32 * cell, y + row * cell, cell, cell, c, 1.0);
                }
            }
            row += 1.0;
        }
    }
}

fn gb(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if bytes >= 1000 << 20 { format!("{g:.1} GB") } else { format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64) }
}

fn uptime(minutes: u64) -> String {
    if minutes >= 60 { format!("{}h {}m", minutes / 60, minutes % 60) } else { format!("{minutes}m") }
}

/// A line graph in the box (x, y, w, h): values 0..=max over the samples' times, with a soft fill under it.
fn graph(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, history: &[Sample], max: f32, value: impl Fn(&Sample) -> f32, rgb: u32, fill: bool) {
    let (Some(first), Some(last)) = (history.first(), history.last()) else { return };
    let span = (last.at - first.at).max(1) as f32;
    let pts: Vec<Point> = history.iter()
        .map(|s| Point::from_xy(x + (s.at - first.at) as f32 / span * w, y + h - (value(s) / max).clamp(0.0, 1.0) * h))
        .collect();
    let mut line = PathBuilder::new();
    line.move_to(pts[0].x, pts[0].y);
    for p in &pts[1..] {
        line.line_to(p.x, p.y);
    }
    if fill {
        let mut area = line.clone();
        area.line_to(pts[pts.len() - 1].x, y + h);
        area.line_to(pts[0].x, y + h);
        area.close();
        let top = pts.iter().map(|p| p.y).fold(y + h, f32::min);   // the fade starts at the line's highest point
        if let (Some(path), Some(shader)) = (area.finish(), LinearGradient::new(
            Point::from_xy(0.0, top), Point::from_xy(0.0, y + h),
            vec![GradientStop::new(0.0, color(rgb, 0.45)), GradientStop::new(1.0, color(rgb, 0.0))],
            SpreadMode::Pad, Transform::identity())) {
            let p = Paint { shader, anti_alias: true, ..Paint::default() };
            c.px.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
        }
    }
    if let Some(path) = line.finish() {
        let stroke = Stroke { width: 4.0, line_join: tiny_skia::LineJoin::Round, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
        c.px.stroke_path(&path, &paint(rgb, 1.0), &stroke, Transform::identity(), None);
    }
}

/// The grid and labels around a graph box; `labels` are the y-axis texts from the bottom line up.
fn axes(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, labels: &[String], minutes: u64) {
    let n = labels.len() - 1;
    for (i, l) in labels.iter().enumerate() {
        let ly = y + h - h * i as f32 / n as f32;
        c.rect(x, ly - 1.0, w, 2.0, LINE, 1.0);
        c.text(l, x - 16.0, ly - 13.0, 22.0, false, FAINT, 1.0);
    }
    let (left, mid) = (format!("{minutes} Min Ago"), format!("{} Min Ago", minutes / 2));
    c.text(&left, x, y + h + 14.0, 22.0, false, FAINT, 0.0);
    c.text(&mid, x + w / 2.0, y + h + 14.0, 22.0, false, FAINT, 0.5);
    c.text("Now", x + w, y + h + 14.0, 22.0, false, FAINT, 1.0);
}

/// The report as a PNG.
pub fn render(pc: &Pc, accounts: &[Account], history: &[Sample]) -> Vec<u8> {
    let accounts = &accounts[..accounts.len().min(9)];
    let tiles_y = PAD + 110.0;
    let graphs_y = tiles_y + 150.0 + GAP;
    let graphs_h = 420.0;
    let list_y = graphs_y + graphs_h + GAP;
    let list_h = 92.0 + ROW * accounts.len() as f32;
    let h = list_y + list_h + PAD;
    let mut c = Canvas {
        px: Pixmap::new(W as u32, h as u32).unwrap(),
        regular: FontRef::try_from_slice(include_bytes!("../assets/fonts/Geist-Regular.ttf")).unwrap(),
        semi: FontRef::try_from_slice(include_bytes!("../assets/fonts/Geist-SemiBold.ttf")).unwrap(),
    };
    c.px.fill(color(BG, 1.0));

    let minutes = history.last().zip(history.first()).map_or(0, |(l, f)| (l.at - f.at) / 60);
    c.logo(PAD, PAD + 4.0, 4.0);
    c.text("Potato Report", PAD + 64.0 + 20.0, PAD, 48.0, true, TEXT, 0.0);
    c.text(&format!("Last {minutes} Min"), W - PAD, PAD + 4.0, 26.0, true, DIM, 1.0);
    c.text(&format!("Potato {}", pc.version), W - PAD, PAD + 40.0, 22.0, false, FAINT, 1.0);

    // big numbers
    let running: usize = accounts.iter().map(|a| a.clients.len()).sum();
    let roblox: u64 = accounts.iter().flat_map(|a| &a.clients).map(|c| c.ram).sum();
    let tw = (W - 2.0 * PAD - 3.0 * GAP) / 4.0;
    let tiles = [
        ("Roblox Open", format!("{running}"), TEXT),
        ("Roblox RAM", gb(roblox), TEXT),
        ("PC RAM", format!("{:.1} / {}", pc.used as f64 / (1u64 << 30) as f64, gb(pc.total)), TEXT),
        ("Freed By Potato", gb(pc.freed), POTATO),
    ];
    for (i, (label, value, ink)) in tiles.iter().enumerate() {
        let x = PAD + i as f32 * (tw + GAP);
        c.round_rect(x, tiles_y, tw, 150.0, 20.0, PANEL, Some(LINE));
        c.text(label, x + 28.0, tiles_y + 26.0, 24.0, false, FAINT, 0.0);
        let size = if value.len() > 9 { 44.0 } else { 56.0 };
        c.text(value, x + 28.0, tiles_y + 64.0, size, true, *ink, 0.0);
        if i == 2 && pc.total > 0 {
            let bw = tw - 56.0;
            c.round_rect(x + 28.0, tiles_y + 128.0, bw, 8.0, 4.0, LINE, None);
            c.round_rect(x + 28.0, tiles_y + 128.0, bw * (pc.used as f32 / pc.total as f32).min(1.0), 8.0, 4.0, TEXT, None);
        }
    }

    // RAM and CPU over time
    let ram_w = (W - 2.0 * PAD - GAP) * 0.62;
    let cpu_w = W - 2.0 * PAD - GAP - ram_w;
    for (x, w, title) in [(PAD, ram_w, "RAM"), (PAD + ram_w + GAP, cpu_w, "Roblox CPU")] {
        c.round_rect(x, graphs_y, w, graphs_h, 20.0, PANEL, Some(LINE));
        c.text(title, x + 28.0, graphs_y + 24.0, 28.0, true, TEXT, 0.0);
    }
    let (gx, gy, gw, gh) = (PAD + 120.0, graphs_y + 100.0, ram_w - 160.0, graphs_h - 170.0);
    let total_gb = pc.total as f32 / (1u64 << 30) as f32;
    let labels: Vec<String> = (0..=4).map(|i| format!("{:.0} GB", total_gb * i as f32 / 4.0)).collect();
    axes(&mut c, gx, gy, gw, gh, &labels, minutes);
    let legend_x = PAD + ram_w - 28.0;
    c.text("Roblox", legend_x, graphs_y + 30.0, 22.0, false, DIM, 1.0);
    let rw = c.width("Roblox", 22.0, false);
    c.round_rect(legend_x - rw - 26.0, graphs_y + 37.0, 14.0, 14.0, 7.0, POTATO, None);
    let pw = c.width("Whole PC", 22.0, false);
    c.text("Whole PC", legend_x - rw - 48.0, graphs_y + 30.0, 22.0, false, DIM, 1.0);
    c.round_rect(legend_x - rw - 48.0 - pw - 26.0, graphs_y + 37.0, 14.0, 14.0, 7.0, TEXT, None);
    let (cx, cw) = (PAD + ram_w + GAP + 100.0, cpu_w - 140.0);
    axes(&mut c, cx, gy, cw, gh, &["0%".into(), "50%".into(), "100%".into()], minutes);
    if history.len() >= 2 {
        let full = (pc.total as f32).max(1.0);
        graph(&mut c, gx, gy, gw, gh, history, full, |s| s.pc_used as f32, TEXT, false);
        graph(&mut c, gx, gy, gw, gh, history, full, |s| s.roblox as f32, POTATO, true);
        graph(&mut c, cx, gy, cw, gh, history, 100.0, |s| s.cpu, POTATO, true);
    } else {
        c.text("The graphs fill in after a few minutes", gx + gw / 2.0, gy + gh / 2.0 - 14.0, 24.0, false, FAINT, 0.5);
    }

    // one row per account
    c.round_rect(PAD, list_y, W - 2.0 * PAD, list_h, 20.0, PANEL, Some(LINE));
    let cols = [PAD + 28.0, PAD + 380.0, PAD + 640.0, PAD + 1010.0, PAD + 1160.0, PAD + 1300.0, W - PAD - 28.0];
    let heads = ["Account", "Status", "RAM", "CPU", "Up For", "Freezes", "Restarts"];
    for (i, head) in heads.iter().enumerate() {
        c.text(head, cols[i], list_y + 28.0, 22.0, false, FAINT, if i == 6 { 1.0 } else { 0.0 });
    }
    let most = accounts.iter().map(|a| a.clients.iter().map(|c| c.ram).sum::<u64>()).max().unwrap_or(1).max(1);
    for (n, a) in accounts.iter().enumerate() {
        let y = list_y + 76.0 + n as f32 * ROW;
        c.rect(PAD + 28.0, y - 8.0, W - 2.0 * PAD - 56.0, 2.0, LINE, 1.0);
        let (status, dot) = if !a.on { ("Potato Off", GREY) } else if a.frozen_now > 0 { ("Not Responding", WARN) }
            else if a.clients.is_empty() { ("Closed", GREY) } else { ("Running", GOOD) };
        let ram: u64 = a.clients.iter().map(|c| c.ram).sum();
        let cpu: f32 = a.clients.iter().map(|c| c.cpu).sum();
        let up = a.clients.iter().map(|c| c.up_minutes).max().unwrap_or(0);
        let open = !a.clients.is_empty();
        c.text(&a.name, cols[0], y + 10.0, 30.0, true, TEXT, 0.0);
        c.round_rect(cols[1], y + 22.0, 14.0, 14.0, 7.0, dot, None);
        c.text(status, cols[1] + 26.0, y + 12.0, 26.0, false, DIM, 0.0);
        if open {
            let bw = 200.0;
            c.round_rect(cols[2], y + 24.0, bw, 10.0, 5.0, LINE, None);
            c.round_rect(cols[2], y + 24.0, (bw * ram as f32 / most as f32).max(10.0), 10.0, 5.0, POTATO, None);
            c.text(&gb(ram), cols[2] + bw + 20.0, y + 12.0, 26.0, false, TEXT, 0.0);
        }
        let dash = |s: String| if open { s } else { "-".into() };
        c.text(&dash(format!("{cpu:.0}%")), cols[3], y + 12.0, 26.0, false, TEXT, 0.0);
        c.text(&dash(uptime(up)), cols[4], y + 12.0, 26.0, false, TEXT, 0.0);
        c.text(&a.freezes.to_string(), cols[5], y + 12.0, 26.0, false, if a.freezes > 0 { WARN } else { TEXT }, 0.0);
        c.text(&a.restarts.to_string(), cols[6], y + 12.0, 26.0, false, TEXT, 1.0);
    }

    c.px.encode_png().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Client;

    fn sample_data() -> (Pc, Vec<Account>, Vec<Sample>) {
        let pc = Pc { used: 17 << 30, total: 32 << 30, freed: 3_650 << 20, version: "0.1.0" };
        let acc = |name: &str, n: usize, ram: u64, freezes: u32, frozen: u32| Account {
            name: name.into(), on: true,
            clients: (0..n).map(|_| Client { ram: ram << 20, cpu: 6.5, up_minutes: 197 }).collect(),
            freezes, frozen_now: frozen, restarts: freezes, screenshots: vec![],
        };
        let accounts = vec![acc("Jay", 0, 0, 0, 0), acc("Roblox", 1, 1430, 0, 0), acc("Main", 1, 1020, 1, 0), acc("Alt1", 1, 797, 0, 1), acc("Alt2", 1, 713, 0, 0)];
        let history = (0..=120u64).map(|i| {
            let t = i as f32;
            let roblox = 3.9 - 0.8 * (t / 60.0).fract() - if i > 90 { 2.4 } else { 0.0 } + 0.15 * (t / 4.0).sin();
            Sample { at: i * 30, pc_used: ((roblox + 13.2) * 1073741824.0) as u64, roblox: (roblox * 1073741824.0) as u64, cpu: 22.0 + 8.0 * (t / 6.0).sin() }
        }).collect();
        (pc, accounts, history)
    }

    #[test]
    fn renders_a_png() {
        let (pc, accounts, history) = sample_data();
        let png = render(&pc, &accounts, &history);
        assert_eq!(&png[1..4], b"PNG");
        assert!(render(&pc, &accounts, &[]).len() > 1000);   // no history yet still draws
    }

    /// cargo test --lib card -- --ignored: writes target/report-card.png to look at.
    #[test]
    #[ignore]
    fn write_preview() {
        let (pc, accounts, history) = sample_data();
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/target/report-card.png"), render(&pc, &accounts, &history)).unwrap();
    }
}
