//! Ajustes en `~/.config/ytm/config.json`. Se editan desde la app (`o`), pero
//! el archivo se puede tocar a mano; lo que falte o no se entienda queda por
//! defecto.

use crate::i18n::Lang;
use ratatui::widgets::BorderType;
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("ytm")
}

fn file() -> PathBuf {
    dir().join("config.json")
}

/// Un ajuste que se elige de una lista fija de valores.
pub trait Choice: Sized + Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    fn key(self) -> &'static str;
    fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|x| x.key() == s)
    }
    fn step(self, d: isize) -> Self {
        let n = Self::ALL.len() as isize;
        let i = Self::ALL.iter().position(|&x| x == self).unwrap_or(0) as isize;
        Self::ALL[(i + d).rem_euclid(n) as usize]
    }
}

macro_rules! choice {
    ($t:ident { $($v:ident => $k:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, PartialEq)]
        pub enum $t { $($v),+ }
        impl Choice for $t {
            const ALL: &'static [Self] = &[$($t::$v),+];
            fn key(self) -> &'static str {
                match self { $($t::$v => $k),+ }
            }
        }
    };
}

choice!(Borders { Auto => "auto", Rounded => "rounded", Plain => "plain", Double => "double", Thick => "thick", None => "none" });
choice!(Bar { Line => "line", Knob => "knob", Blocks => "blocks", Dots => "dots", Ascii => "ascii" });
choice!(Quality { Best => "best", Medium => "medium", Low => "low" });

impl Borders {
    /// `rounded`: con Auto, si Hyprland redondea las ventanas.
    pub fn border_type(self, rounded: bool) -> Option<BorderType> {
        Some(match self {
            Borders::Auto if rounded => BorderType::Rounded,
            Borders::Auto => BorderType::Plain,
            Borders::Rounded => BorderType::Rounded,
            Borders::Plain => BorderType::Plain,
            Borders::Double => BorderType::Double,
            Borders::Thick => BorderType::Thick,
            Borders::None => return None,
        })
    }
}

impl Bar {
    /// hecho, cabeza, lo que falta
    pub fn glyphs(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Bar::Line => ("━", "━", "─"),
            Bar::Knob => ("━", "●", "─"),
            Bar::Blocks => ("█", "█", "░"),
            Bar::Dots => ("•", "•", "·"),
            Bar::Ascii => ("=", ">", "-"),
        }
    }
}

impl Quality {
    pub fn ytdl_format(self) -> &'static str {
        match self {
            Quality::Best => "bestaudio/best",
            Quality::Medium => "bestaudio[abr<=128]/bestaudio/best",
            Quality::Low => "worstaudio/worst",
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub lang: Lang,
    pub theme: String,
    pub transparent: bool,
    pub borders: Borders,
    pub bar: Bar,
    pub icons: bool,
    pub side_width: u16,
    pub lyrics_width: u16, // % del panel principal
    pub footer: bool,
    pub quality: Quality,
    pub vol_step: i64,
    pub seek: i64,
    pub notify_secs: u64,
    pub restore: bool,
    pub wallust: PathBuf,
}

pub const SIDE_WIDTHS: &[u16] = &[20, 24, 28, 32, 36, 40, 48];
pub const LYRICS_WIDTHS: &[u16] = &[30, 35, 40, 45, 50, 60];
pub const VOL_STEPS: &[i64] = &[1, 2, 5, 10];
pub const SEEKS: &[i64] = &[5, 10, 15, 30, 60];
pub const NOTIFY_SECS: &[u64] = &[2, 4, 6, 10];

/// El siguiente (o anterior) de `opts` a partir de `cur`, sin dar la vuelta.
pub fn step_in<T: Copy + PartialOrd>(opts: &[T], cur: T, d: isize) -> T {
    let i = opts.iter().position(|&x| x >= cur).unwrap_or(opts.len() - 1) as isize;
    opts[(i + d).clamp(0, opts.len() as isize - 1) as usize]
}

impl Default for Config {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        Config {
            lang: Lang::detect(),
            theme: "kitty".into(),
            transparent: true,
            borders: Borders::Auto,
            bar: Bar::Line,
            icons: true,
            side_width: 28,
            lyrics_width: 40,
            footer: true,
            quality: Quality::Best,
            vol_step: 5,
            seek: 5,
            notify_secs: 4,
            restore: true,
            wallust: PathBuf::from(home).join(".config/kitty/kitty-themes/01-Wallust.conf"),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        let d = Config::default();
        let Some(v) = std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
            return d;
        };
        fn pick<T: Choice>(v: &Value, d: T) -> T {
            v.as_str().and_then(T::parse).unwrap_or(d)
        }
        let num = |k: &str, d: u64, lo: u64, hi: u64| v[k].as_u64().map(|n| n.clamp(lo, hi)).unwrap_or(d);
        Config {
            lang: v["lang"].as_str().and_then(Lang::parse).unwrap_or(d.lang),
            theme: v["theme"].as_str().map(String::from).unwrap_or(d.theme),
            transparent: v["transparent"].as_bool().unwrap_or(d.transparent),
            borders: pick(&v["borders"], d.borders),
            bar: pick(&v["bar"], d.bar),
            icons: v["icons"].as_bool().unwrap_or(d.icons),
            side_width: num("side_width", d.side_width as u64, 16, 80) as u16,
            lyrics_width: num("lyrics_width", d.lyrics_width as u64, 20, 80) as u16,
            footer: v["footer"].as_bool().unwrap_or(d.footer),
            quality: pick(&v["quality"], d.quality),
            vol_step: num("vol_step", d.vol_step as u64, 1, 25) as i64,
            seek: num("seek", d.seek as u64, 1, 300) as i64,
            notify_secs: num("notify_secs", d.notify_secs, 1, 60),
            restore: v["restore"].as_bool().unwrap_or(d.restore),
            wallust: v["wallust"].as_str().map(PathBuf::from).unwrap_or(d.wallust),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let v = json!({
            "lang": self.lang.key(),
            "theme": self.theme,
            "transparent": self.transparent,
            "borders": self.borders.key(),
            "bar": self.bar.key(),
            "icons": self.icons,
            "side_width": self.side_width,
            "lyrics_width": self.lyrics_width,
            "footer": self.footer,
            "quality": self.quality.key(),
            "vol_step": self.vol_step,
            "seek": self.seek,
            "notify_secs": self.notify_secs,
            "restore": self.restore,
            "wallust": self.wallust,
        });
        let f = file();
        std::fs::create_dir_all(dir())
            .and_then(|_| std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"))
            .map_err(|e| format!("{}: {e}", f.display()))
    }
}
