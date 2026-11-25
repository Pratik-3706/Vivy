//! Pixel Matsumoto: the cube helper, drawn in the terminal with half-block
//! characters (two pixels per character cell).
//!
//! He is drawn procedurally rather than from fixed sprites, so his size,
//! colours, the way he turns, blinks and floats all come from a few numbers.
//! Everything about his look can be changed in the CLI config
//! (`vivy matsumoto --help`).

use crate::term::{Color, ColorMode, Rgb};

/// A named colour scheme. `body`, `eye` and `lens` can each be overridden.
pub struct Theme {
    pub name: &'static str,
    pub blurb: &'static str,
    pub body: Rgb,
    pub eye: Rgb,
    pub lens: Rgb,
}

pub const THEMES: &[Theme] = &[
    Theme { name: "classic", blurb: "white shell, blue eye", body: Rgb(236, 234, 228), eye: Rgb(61, 139, 255), lens: Rgb(38, 38, 43) },
    Theme { name: "night", blurb: "graphite shell, cyan eye", body: Rgb(70, 76, 90), eye: Rgb(124, 240, 255), lens: Rgb(17, 19, 23) },
    Theme { name: "sakura", blurb: "blossom pink, rose eye", body: Rgb(255, 222, 233), eye: Rgb(255, 79, 139), lens: Rgb(58, 38, 48) },
    Theme { name: "mint", blurb: "mint shell, green eye", body: Rgb(214, 245, 230), eye: Rgb(25, 179, 122), lens: Rgb(30, 43, 38) },
    Theme { name: "sunset", blurb: "peach shell, amber eye", body: Rgb(255, 216, 170), eye: Rgb(255, 120, 30), lens: Rgb(56, 34, 24) },
    Theme { name: "retro", blurb: "four greens, like an old handheld", body: Rgb(155, 188, 15), eye: Rgb(15, 56, 15), lens: Rgb(48, 98, 48) },
];

pub fn theme(name: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.name.eq_ignore_ascii_case(name.trim()))
}

/// Cube edge length in pixels for each size.
pub fn edge_for(size: &str) -> Option<u32> {
    match size.trim().to_ascii_lowercase().as_str() {
        "tiny" => Some(8),
        "small" => Some(12),
        "medium" => Some(16),
        "large" => Some(22),
        _ => None,
    }
}

pub const SIZES: &[&str] = &["tiny", "small", "medium", "large"];

/// The colours of every part, derived from the three theme colours.
#[derive(Clone, Copy)]
pub struct Palette {
    pub body: Rgb,
    pub top: Rgb,
    pub side: Rgb,
    pub edge: Rgb,
    pub seam: Rgb,
    pub lens: Rgb,
    pub eye: Rgb,
    pub glint: Rgb,
    pub shadow: Rgb,
}

impl Palette {
    pub fn new(body: Rgb, eye: Rgb, lens: Rgb) -> Self {
        let dark_body = body.luma() < 110.0;
        Self {
            body,
            top: if dark_body { body.mix(Rgb(255, 255, 255), 0.18) } else { body.mix(Rgb(255, 255, 255), 0.55) },
            side: body.mix(Rgb(0, 0, 0), 0.22),
            edge: body.mix(Rgb(0, 0, 0), 0.38),
            seam: body.mix(Rgb(0, 0, 0), 0.12),
            lens,
            eye,
            glint: eye.mix(Rgb(255, 255, 255), 0.85),
            shadow: Rgb(58, 58, 64),
        }
    }
}

/// How he's posed in one frame.
#[derive(Clone, Copy, Default)]
pub struct Pose {
    /// -1.0 (turned to his left) .. 1.0 (turned right); 0 faces you.
    pub turn: f32,
    /// Where the eye looks: -1 left, 0 ahead, 1 right.
    pub look: i32,
    pub blink: bool,
    /// Pixels lifted off the ground (0 or 1 looks best).
    pub lift: u32,
}

/// A small RGB image; `None` pixels are see-through.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    px: Vec<Option<Rgb>>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, px: vec![None; w * h] }
    }
    fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = Some(c);
        }
    }
    fn clear(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = None;
        }
    }
    pub fn get(&self, x: usize, y: usize) -> Option<Rgb> {
        if x < self.w && y < self.h { self.px[y * self.w + x] } else { None }
    }
    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.set(x, y, c);
            }
        }
    }
}

/// Canvas size for a cube of edge `s`, the same for every pose so frames
/// line up when animated.
pub fn canvas_size(s: u32) -> (usize, usize) {
    let s = s as usize;
    let top = top_height(s as u32) as usize;
    let w = s + (s * 2) / 3 + 2;
    // cube + top face + room to float + shadow; even, for half-blocks
    let h = s + top + 3;
    (w, h + h % 2)
}

fn top_height(s: u32) -> u32 {
    ((s as f32 * 0.22).round() as u32).max(2)
}

/// Draw Matsumoto in `pose`.
pub fn draw(p: &Palette, s: u32, pose: Pose) -> Canvas {
    let (w, h) = canvas_size(s);
    let mut c = Canvas::new(w, h);
    let sf = s as f32;
    let angle = pose.turn.clamp(-1.0, 1.0) * 40f32.to_radians();
    let fw = ((sf * angle.cos()).round() as i32).max(2);
    let sw = (sf * angle.sin().abs() * 0.8).round() as i32;
    let total = fw + sw;
    let th = top_height(s) as i32;
    let x0 = (w as i32 - total) / 2;
    let (front_x, side_x) = if pose.turn >= 0.0 { (x0, x0 + fw) } else { (x0 + sw, x0) };

    let ground = h as i32 - 2; // shadow row
    let bottom = ground - pose.lift as i32; // first row below the cube
    let face_top = bottom - s as i32;
    let top_y = face_top - th;

    // Shadow: shrinks as he lifts off.
    let shadow_w = (total as f32 * if pose.lift > 0 { 0.55 } else { 0.72 }).round() as i32;
    let sx = (w as i32 - shadow_w) / 2;
    c.fill(sx, ground, sx + shadow_w, ground + 1, p.shadow);
    c.clear(sx, ground);
    c.clear(sx + shadow_w - 1, ground);

    // Top face: lighter, with a hint of the side's shade above the side face.
    c.fill(x0, top_y, x0 + total, face_top, p.top);
    if sw > 0 {
        c.fill(side_x, top_y, side_x + sw, face_top, p.top.mix(p.side, 0.35));
    }
    // Front and side faces.
    c.fill(front_x, face_top, front_x + fw, bottom, p.body);
    if sw > 0 {
        c.fill(side_x, face_top, side_x + sw, bottom, p.side);
    }
    // Edges: where the top meets the faces, and where the faces meet.
    c.fill(x0, face_top, x0 + total, face_top + 1, p.edge.mix(p.top, 0.45));
    if sw > 0 {
        let seam_x = if pose.turn >= 0.0 { front_x + fw } else { front_x - 1 };
        c.fill(seam_x, face_top, seam_x + 1, bottom, p.edge);
    }
    // Panel seam across the lower part of the faces.
    let seam_y = face_top + (s as f32 * 0.8).round() as i32;
    c.fill(front_x + 1, seam_y, front_x + fw - 1, seam_y + 1, p.seam);
    if sw > 1 {
        c.fill(side_x, seam_y, side_x + sw, seam_y + 1, p.side.mix(p.edge, 0.5));
    }
    // Rounded (chamfered) outer corners.
    for (x, y) in [(x0, top_y), (x0 + total - 1, top_y), (x0, bottom - 1), (x0 + total - 1, bottom - 1)] {
        c.clear(x, y);
    }
    if s >= 16 {
        for (x, y) in [(x0 + 1, top_y), (x0 + total - 2, top_y), (x0, top_y + 1), (x0 + total - 1, top_y + 1)] {
            c.clear(x, y);
        }
    }

    // The lens and eye, squashed sideways as the front face turns away. Whole-
    // pixel centres and radii keep the circles symmetric at every size.
    let k = fw as f32 / sf;
    if k > 0.3 {
        let r = (sf * 0.28).round().max(2.0);
        let ri = (r * 0.58).max(1.2);
        let cx = front_x + fw / 2;
        let cy = face_top + (sf * 0.45).round() as i32;
        let shift = if r >= 3.0 { pose.look.clamp(-1, 1) } else { 0 };
        let inside = |dx: i32, dy: i32, rad: f32| {
            let (fx, fy) = (dx as f32 + 0.5, dy as f32 + 0.5);
            (fx / k).powi(2) + fy.powi(2) <= rad * rad
        };
        let span = r.ceil() as i32 + 1;
        for dy in -span..span {
            for dx in -span..span {
                if !inside(dx, dy, r) {
                    continue;
                }
                c.set(cx + dx, cy + dy, p.lens);
                let iris = if pose.blink {
                    dy == 0 && inside(dx - shift, 0, ri)
                } else {
                    inside(dx - shift, dy, ri)
                };
                if iris {
                    c.set(cx + dx, cy + dy, p.eye);
                }
            }
        }
        if !pose.blink && k > 0.6 {
            // A glint at the top left of the iris.
            let top = -(ri.ceil() as i32);
            c.set(cx + shift - 1, cy + top, p.glint);
        }
    }
    c
}

// ---- Turning it into text ---------------------------------------------------

/// Lines of text for `canvas`. `pixel` uses half blocks and colour; otherwise
/// plain ASCII shading for terminals without Unicode or colour.
pub fn to_lines(canvas: &Canvas, mode: ColorMode, pixel: bool) -> Vec<String> {
    let mut lines = Vec::with_capacity(canvas.h / 2);
    for row in (0..canvas.h).step_by(2) {
        let mut line = String::new();
        for x in 0..canvas.w {
            let (up, down) = (canvas.get(x, row), canvas.get(x, row + 1));
            if !pixel || mode == ColorMode::None {
                line.push(ascii_cell(up, down));
                continue;
            }
            match (up, down) {
                (None, None) => {
                    line.push_str(Color::RESET);
                    line.push(' ');
                }
                (Some(u), None) => {
                    line.push_str(Color::RESET);
                    line.push_str(&Color::fg(u, mode));
                    line.push('▀');
                }
                (None, Some(d)) => {
                    line.push_str(Color::RESET);
                    line.push_str(&Color::fg(d, mode));
                    line.push('▄');
                }
                (Some(u), Some(d)) => {
                    line.push_str(&Color::fg(u, mode));
                    line.push_str(&Color::bg(d, mode));
                    line.push('▀');
                }
            }
        }
        if pixel && mode != ColorMode::None {
            line.push_str(Color::RESET);
        }
        lines.push(line.trim_end_matches(' ').to_string());
    }
    lines
}

fn ascii_cell(up: Option<Rgb>, down: Option<Rgb>) -> char {
    let l = match (up, down) {
        (None, None) => return ' ',
        (Some(a), None) => a.luma() * 0.5,
        (None, Some(b)) => b.luma() * 0.5,
        (Some(a), Some(b)) => (a.luma() + b.luma()) / 2.0,
    };
    const RAMP: &[u8] = b".:-=+*%#@";
    RAMP[((l / 256.0) * RAMP.len() as f32) as usize % RAMP.len()] as char
}

// ---- Animation ----------------------------------------------------------------

/// Idle: he floats, looks around and blinks now and then. `t` counts frames
/// at roughly 8 per second.
pub fn idle_pose(t: u64) -> Pose {
    let cycle = t % 64;
    Pose {
        turn: 0.0,
        look: match cycle {
            16..=23 => -1,
            40..=47 => 1,
            _ => 0,
        },
        blink: matches!(cycle, 30 | 31 | 58),
        lift: if (t / 6) % 2 == 0 { 0 } else { 1 },
    }
}

/// Busy: turns back and forth as he works, like the desktop cube.
pub fn busy_pose(t: u64) -> Pose {
    let phase = (t as f32 * 0.21).sin();
    Pose { turn: phase * 0.9, look: 0, blink: false, lift: if (t / 3) % 2 == 0 { 1 } else { 0 } }
}

/// A one-line, two-pixel-tall Matsumoto for status lines: a lit top edge
/// over a face whose eye scans while he works.
pub fn mini(p: &Palette, t: u64, mode: ColorMode, pixel: bool) -> String {
    if !pixel || mode == ColorMode::None {
        const FRAMES: [&str; 4] = ["[o  ]", "[ o ]", "[  o]", "[ o ]"];
        return FRAMES[(t / 2) as usize % 4].to_string();
    }
    let eye_at = [1usize, 2, 3, 2][(t / 2) as usize % 4];
    let mut c = Canvas::new(5, 2);
    for x in 0..5 {
        c.set(x, 0, if x == 4 { p.top.mix(p.side, 0.35) } else { p.top });
        c.set(x, 1, if x == 4 { p.side } else { p.body });
    }
    c.clear(0, 0);
    c.set(eye_at as i32, 1, p.eye);
    to_lines(&c, mode, true).remove(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let t = theme("classic").unwrap();
        Palette::new(t.body, t.eye, t.lens)
    }

    #[test]
    fn every_pose_fits_the_same_canvas() {
        for size in SIZES {
            let s = edge_for(size).unwrap();
            let (w, h) = canvas_size(s);
            assert_eq!(h % 2, 0, "half-blocks need an even height");
            for t in 0..80 {
                for pose in [idle_pose(t), busy_pose(t)] {
                    let c = draw(&palette(), s, pose);
                    assert_eq!((c.w, c.h), (w, h));
                    // Nothing is cut off at the canvas edges.
                    for y in 0..h {
                        assert!(c.get(0, y).is_none() && c.get(w - 1, y).is_none(), "{size} t={t}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_eye_is_visible_facing_forward() {
        let p = palette();
        let c = draw(&p, 16, Pose::default());
        let eye_pixels = (0..c.h).flat_map(|y| (0..c.w).map(move |x| (x, y))).filter(|&(x, y)| c.get(x, y) == Some(p.eye)).count();
        assert!(eye_pixels >= 6, "only {eye_pixels} eye pixels");
    }

    #[test]
    fn ascii_mode_has_no_escape_codes() {
        let lines = to_lines(&draw(&palette(), 12, Pose::default()), ColorMode::None, false);
        assert!(lines.iter().all(|l| l.is_ascii() && !l.contains('\x1b')));
        assert!(mini(&palette(), 0, ColorMode::None, false).is_ascii());
    }

    #[test]
    fn themes_and_sizes_resolve() {
        assert!(theme("Sakura").is_some());
        assert!(theme("nope").is_none());
        assert_eq!(edge_for("LARGE"), Some(22));
    }
}
