//! Temas: el que tenga kitty puesto ahora (lo cambian los looks de Hyprland),
//! wallust (el theme de kitty que genera), los colores de la terminal, unos
//! cuantos predefinidos y los que pongas en `~/.config/rola/themes/*.conf`
//! (mismo formato que un theme de kitty). Los que salen de un archivo se
//! recargan solos al cambiar.

use crate::config::{Config, Tint};
use ratatui::style::Color;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color, // Reset: el fondo de la terminal
    pub base: Color, // el fondo del tema aunque se use el de la terminal (texto sobre el color principal)
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
            bg: Color::Reset,
            base: Color::Black,
            primary: Color::Blue,
            accent: Color::Yellow,
            fg: Color::Reset,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            hl: Color::DarkGray,
        }
    }
}

type Rgb = (u8, u8, u8);

/// fondo, texto, principal, acento (avisos y errores)
const PRESETS: &[(&str, [u32; 4])] = &[
    ("catppuccin-mocha", [0x1e1e2e, 0xcdd6f4, 0xcba6f7, 0xf9e2af]),
    ("catppuccin-latte", [0xeff1f5, 0x4c4f69, 0x8839ef, 0xdf8e1d]),
    ("tokyo-night", [0x1a1b26, 0xc0caf5, 0x7aa2f7, 0xe0af68]),
    ("gruvbox", [0x282828, 0xebdbb2, 0x83a598, 0xfabd2f]),
    ("nord", [0x2e3440, 0xd8dee9, 0x88c0d0, 0xebcb8b]),
    ("dracula", [0x282a36, 0xf8f8f2, 0xbd93f9, 0xff79c6]),
    ("rose-pine", [0x191724, 0xe0def4, 0xebbcba, 0xf6c177]),
    ("everforest", [0x2d353b, 0xd3c6aa, 0xa7c080, 0xdbbc7f]),
    ("kanagawa", [0x1f1f28, 0xdcd7ba, 0x7e9cd8, 0xffa066]),
    ("youtube", [0x0f0f0f, 0xf1f1f1, 0xff0033, 0xffb800]),
];

fn custom_dir() -> PathBuf {
    crate::config::dir().join("themes")
}

/// Todos los nombres elegibles, en el orden en que se recorren.
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = ["kitty", "wallust", "terminal"].iter().map(|s| s.to_string()).collect();
    v.extend(PRESETS.iter().map(|p| p.0.to_string()));
    let mut custom: Vec<String> = std::fs::read_dir(custom_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".conf").map(String::from))
        .filter(|n| !v.contains(n))
        .collect();
    custom.sort();
    v.extend(custom);
    v
}

fn kitty_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("kitty")
}

/// El theme que incluye kitty.conf ahora mismo (el último `include` con
/// colores), o kitty.conf mismo si los colores van ahí.
fn kitty_theme() -> PathBuf {
    let dir = kitty_dir();
    let conf = dir.join("kitty.conf");
    let home = std::env::var("HOME").unwrap_or_default();
    std::fs::read_to_string(&conf)
        .ok()
        .and_then(|text| {
            text.lines()
                .filter_map(|l| l.trim().strip_prefix("include"))
                .map(|p| match p.trim().strip_prefix("~/") {
                    Some(rest) => PathBuf::from(&home).join(rest),
                    None => dir.join(p.trim()),
                })
                .rev()
                .find(|p| std::fs::read_to_string(p).is_ok_and(|t| t.lines().any(|l| l.starts_with("background"))))
        })
        .unwrap_or(conf)
}

/// El archivo del que sale el tema, si sale de uno (para recargarlo al cambiar).
pub fn source(c: &Config) -> Option<PathBuf> {
    let name = c.theme.as_str();
    match name {
        "kitty" => Some(kitty_theme()),
        "wallust" => Some(c.wallust.clone()),
        "terminal" => None,
        _ if PRESETS.iter().any(|p| p.0 == name) => None,
        _ => Some(custom_dir().join(format!("{name}.conf"))),
    }
}

pub fn mtime(file: Option<&Path>) -> Option<SystemTime> {
    std::fs::metadata(file?).and_then(|m| m.modified()).ok()
}

fn hex(s: &str) -> Option<Rgb> {
    let s = s.strip_prefix('#')?;
    let n = u32::from_str_radix(s, 16).ok()?;
    (s.len() == 6).then_some(split(n))
}

fn split(n: u32) -> Rgb {
    ((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Color {
    let m = |x: u8, y: u8| (x as f32 * t + y as f32 * (1.0 - t)).round() as u8;
    Color::Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn build(bg: Rgb, fg: Rgb, primary: Rgb, accent: Rgb, c: &Config) -> Theme {
    let rgb = |(r, g, b): Rgb| Color::Rgb(r, g, b);
    let transparent = c.transparent;
    let (fg, primary) = match c.tint {
        Tint::Color => (fg, primary),
        // como un monitor de fósforo: el texto también en el color principal
        Tint::Phosphor => (primary, primary),
        // papel y tinta: grises, el acento solo para avisos
        Tint::Ink => (fg, fg),
    };
    Theme {
        bg: if transparent { Color::Reset } else { rgb(bg) },
        base: rgb(bg),
        primary: rgb(primary),
        accent: rgb(accent),
        fg: rgb(fg),
        muted: mix(fg, bg, 0.55),
        border: mix(fg, bg, 0.22),
        hl: mix(primary, bg, 0.35),
    }
}

/// Theme de kitty: `background`, `foreground`, `color0`…`color15`; acepta
/// además `primary` y `accent` para elegirlos a mano.
fn from_file(file: &Path, cfg: &Config) -> Option<Theme> {
    let text = std::fs::read_to_string(file).ok()?;
    let c: HashMap<&str, Rgb> = text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?, hex(it.next()?)?))
        })
        .collect();
    let (&bg, &fg) = (c.get("background")?, c.get("foreground")?);
    let get = |ks: &[&str], d: Rgb| ks.iter().find_map(|k| c.get(k).copied()).unwrap_or(d);
    let primary = get(&["primary", "color12"], fg);
    Some(build(bg, fg, primary, get(&["accent", "color11"], primary), cfg))
}

pub fn load(c: &Config) -> Theme {
    if let Some(&(_, [bg, fg, p, a])) = PRESETS.iter().find(|p| p.0 == c.theme) {
        return build(split(bg), split(fg), split(p), split(a), c);
    }
    let t = match source(c) {
        Some(file) => from_file(&file, c),
        None => None,
    };
    t.unwrap_or_else(|| {
        let d = Theme::default();
        match c.tint {
            Tint::Color => d,
            Tint::Phosphor => Theme { fg: d.primary, ..d },
            Tint::Ink => Theme { primary: d.fg, ..d },
        }
    })
}
