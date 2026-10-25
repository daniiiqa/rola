//! Colores desde wallust (el theme de kitty que genera), recargados en vivo.

use ratatui::style::Color;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Copy)]
pub struct Theme {
    pub primary: Color,
    pub accent: Color,
    pub fg: Color,
    pub muted: Color,
    pub border: Color,
    pub hl: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            primary: Color::Blue,
            accent: Color::Yellow,
            fg: Color::Reset,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            hl: Color::DarkGray,
        }
    }
}

fn path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/kitty/kitty-themes/01-Wallust.conf")
}

pub fn mtime() -> Option<SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

fn hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#')?;
    let n = u32::from_str_radix(s, 16).ok()?;
    (s.len() == 6).then_some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color {
    let m = |x: u8, y: u8| (x as f32 * t + y as f32 * (1.0 - t)).round() as u8;
    Color::Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

pub fn load() -> Theme {
    let Ok(text) = std::fs::read_to_string(path()) else { return Theme::default() };
    let c: HashMap<&str, (u8, u8, u8)> = text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?, hex(it.next()?)?))
        })
        .collect();
    let (Some(&bg), Some(&fg)) = (c.get("background"), c.get("foreground")) else {
        return Theme::default();
    };
    let get = |k: &str, d: (u8, u8, u8)| *c.get(k).unwrap_or(&d);
    let primary = get("color12", fg);
    let rgb = |(r, g, b): (u8, u8, u8)| Color::Rgb(r, g, b);
    Theme {
        primary: rgb(primary),
        accent: rgb(get("color11", primary)),
        fg: rgb(fg),
        muted: mix(fg, bg, 0.55),
        border: mix(fg, bg, 0.22),
        hl: mix(primary, bg, 0.35),
    }
}
