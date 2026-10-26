//! Ajustes en `~/.config/rola/config.json`. Se editan desde la app (`o`), pero
//! el archivo se puede tocar a mano; lo que falte o no se entienda queda por
//! defecto.
//!
//! `~/.config/rola/look.json`, si existe, pisa las claves que traiga: es lo que
//! cambia cada look de Hyprland (LookSwitch.sh lo copia como al resto de
//! archivos del look). Lo que se cambie desde la app en una de esas claves se
//! guarda ahí, así que vuelve con el look.

use crate::i18n::Lang;
use ratatui::symbols::border;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::SystemTime;

pub fn dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("rola")
}

fn file() -> PathBuf {
    dir().join("config.json")
}

fn look_file() -> PathBuf {
    dir().join("look.json")
}

/// Para notar cuando alguien (un look, a mano) cambia los archivos.
pub fn mtimes() -> [Option<SystemTime>; 2] {
    [file(), look_file()].map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
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

choice!(Borders {
    Auto => "auto",
    Rounded => "rounded",
    Plain => "plain",
    Double => "double",
    Thick => "thick",
    Dashed => "dashed",
    None => "none",
});
choice!(Bar { Line => "line", Knob => "knob", Blocks => "blocks", Dots => "dots", Ascii => "ascii" });
choice!(Quality { Best => "best", Medium => "medium", Low => "low" });
// cómo se rotulan los botones: iconos de Nerd Font, palabras cortas o enteras
choice!(Labels { Icons => "icons", Short => "short", Words => "words" });
// aviso del escritorio con cada canción: nunca, si no estás mirando rola, siempre
choice!(Desktop { Off => "off", Away => "away", Always => "always" });
// cómo suena: filtros de audio de mpv (ffmpeg)
choice!(Sound { Normal => "normal", Bass => "bass", Vocals => "vocals", Night => "night", Even => "even" });

impl Sound {
    /// El filtro (lavfi) de cada uno; None, tal cual.
    pub fn filter(self) -> Option<&'static str> {
        match self {
            Sound::Normal => None,
            Sound::Bass => Some("bass=g=6:f=110"),
            // la voz (2–4 kHz) un poco arriba, los graves un poco abajo
            Sound::Vocals => Some("equalizer=f=3000:t=q:w=1.2:g=4,bass=g=-3"),
            // comprime: lo fuerte baja, lo bajo sube (para la noche, con cascos)
            Sound::Night => Some("acompressor=threshold=0.1:ratio=4:attack=20:release=250:makeup=2"),
            // iguala el volumen entre canciones (y dentro, poco a poco)
            Sound::Even => Some("dynaudnorm=f=200:g=11:p=0.9"),
        }
    }
}
// mayúsculas/minúsculas en los rótulos de la interfaz (no en títulos de canciones)
choice!(Case { Normal => "normal", Upper => "upper", Lower => "lower" });
// cómo se marca la fila elegida
choice!(Select { Fill => "fill", Bar => "bar", Inverse => "inverse" });
// color: normal · phosphor: todo en el color principal · ink: grises, sin acento
choice!(Tint { Color => "color", Phosphor => "phosphor", Ink => "ink" });

const DASHED: border::Set = border::Set {
    horizontal_top: "┄",
    horizontal_bottom: "┄",
    vertical_left: "┆",
    vertical_right: "┆",
    ..border::PLAIN
};

impl Borders {
    /// `rounded`: con Auto, si Hyprland redondea las ventanas.
    pub fn set(self, rounded: bool) -> border::Set {
        match self {
            Borders::Auto if rounded => border::ROUNDED,
            Borders::Auto | Borders::Plain => border::PLAIN,
            Borders::Rounded => border::ROUNDED,
            Borders::Double => border::DOUBLE,
            Borders::Thick => border::THICK,
            Borders::Dashed => DASHED,
            Borders::None => border::EMPTY,
        }
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

impl Case {
    pub fn apply(self, s: &str) -> String {
        match self {
            Case::Normal => s.to_string(),
            Case::Upper => s.to_uppercase(),
            Case::Lower => s.to_lowercase(),
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
    pub labels: Labels,
    pub case: Case,
    pub select: Select,
    pub tint: Tint,
    pub side_width: u16,
    pub lyrics_width: u16, // % del panel principal
    pub footer: bool,
    pub discord: String, // Application ID para el estado de Discord; vacío: no
    pub cover_colors: bool, // el color principal y el de acento, de la portada
    pub desktop: Desktop,
    pub crossfade: u64, // segundos; 0: sin fundido
    pub sound: Sound,
    pub offline_mb: u64, // tope de lo guardado sin conexión; 0: nada
    pub motion: bool, // ecualizador, títulos que se desplazan, aviso de canción nueva
    pub quality: Quality,
    pub vol_step: i64,
    pub seek: i64,
    pub notify_secs: u64,
    pub restore: bool,
    pub mouse: bool,
    pub wallust: PathBuf,
    /// claves que vienen de look.json (y ahí se guardan)
    pub look: Vec<String>,
    /// config.json tal cual, para no perder lo que look.json tapa
    base: Map<String, Value>,
}

pub const SIDE_WIDTHS: &[u16] = &[20, 24, 28, 32, 36, 40, 48];
pub const LYRICS_WIDTHS: &[u16] = &[30, 35, 40, 45, 50, 60];
pub const VOL_STEPS: &[i64] = &[1, 2, 5, 10];
pub const SEEKS: &[i64] = &[5, 10, 15, 30, 60];
pub const NOTIFY_SECS: &[u64] = &[2, 4, 6, 10];
pub const CROSSFADES: &[u64] = &[0, 3, 6, 9, 12];
pub const OFFLINE_MBS: &[u64] = &[0, 500, 1000, 2000, 5000];

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
            labels: Labels::Icons,
            case: Case::Normal,
            select: Select::Fill,
            tint: Tint::Color,
            side_width: 28,
            lyrics_width: 40,
            footer: true,
            discord: String::new(),
            cover_colors: false,
            desktop: Desktop::Off,
            crossfade: 0,
            sound: Sound::Normal,
            offline_mb: 0,
            motion: true,
            quality: Quality::Best,
            vol_step: 5,
            seek: 5,
            notify_secs: 4,
            restore: true,
            mouse: true,
            wallust: PathBuf::from(home).join(".config/kitty/kitty-themes/01-Wallust.conf"),
            look: Vec::new(),
            base: Map::new(),
        }
    }
}

fn read_obj(f: PathBuf) -> Map<String, Value> {
    std::fs::read_to_string(f)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn write(f: PathBuf, v: &Map<String, Value>) -> Result<(), String> {
    std::fs::create_dir_all(dir())
        .and_then(|_| std::fs::write(&f, serde_json::to_string_pretty(v).unwrap_or_default() + "\n"))
        .map_err(|e| format!("{}: {e}", f.display()))
}

impl Config {
    pub fn load() -> Config {
        let d = Config::default();
        let base = read_obj(file());
        let over = read_obj(look_file());
        let mut v = base.clone();
        v.extend(over.clone());
        let v = Value::Object(v);
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
            // antes era "icons": sí/no
            labels: v["labels"].as_str().and_then(Labels::parse).unwrap_or(match v["icons"].as_bool() {
                Some(false) => Labels::Short,
                _ => d.labels,
            }),
            case: pick(&v["case"], d.case),
            select: pick(&v["select"], d.select),
            tint: pick(&v["tint"], d.tint),
            side_width: num("side_width", d.side_width as u64, 16, 80) as u16,
            lyrics_width: num("lyrics_width", d.lyrics_width as u64, 20, 80) as u16,
            footer: v["footer"].as_bool().unwrap_or(d.footer),
            discord: v["discord"].as_str().unwrap_or("").to_string(),
            cover_colors: v["cover_colors"].as_bool().unwrap_or(d.cover_colors),
            desktop: v["desktop"].as_str().and_then(Desktop::parse).unwrap_or(d.desktop),
            crossfade: v["crossfade"].as_u64().unwrap_or(d.crossfade).min(30),
            sound: pick(&v["sound"], d.sound),
            offline_mb: v["offline_mb"].as_u64().unwrap_or(d.offline_mb),
            motion: v["motion"].as_bool().unwrap_or(d.motion),
            quality: pick(&v["quality"], d.quality),
            vol_step: num("vol_step", d.vol_step as u64, 1, 25) as i64,
            seek: num("seek", d.seek as u64, 1, 300) as i64,
            notify_secs: num("notify_secs", d.notify_secs, 1, 60),
            restore: v["restore"].as_bool().unwrap_or(d.restore),
            mouse: v["mouse"].as_bool().unwrap_or(d.mouse),
            wallust: v["wallust"].as_str().map(PathBuf::from).unwrap_or(d.wallust),
            look: over.keys().map(|k| if k == "icons" { "labels".into() } else { k.clone() }).collect(),
            base,
        }
    }

    pub fn in_look(&self, key: &str) -> bool {
        self.look.iter().any(|k| k == key)
    }

    pub fn save(&self) -> Result<(), String> {
        let v = json!({
            "lang": self.lang.key(),
            "theme": self.theme,
            "transparent": self.transparent,
            "borders": self.borders.key(),
            "bar": self.bar.key(),
            "labels": self.labels.key(),
            "case": self.case.key(),
            "select": self.select.key(),
            "tint": self.tint.key(),
            "side_width": self.side_width,
            "lyrics_width": self.lyrics_width,
            "footer": self.footer,
            "discord": self.discord,
            "cover_colors": self.cover_colors,
            "desktop": self.desktop.key(),
            "crossfade": self.crossfade,
            "sound": self.sound.key(),
            "offline_mb": self.offline_mb,
            "motion": self.motion,
            "quality": self.quality.key(),
            "vol_step": self.vol_step,
            "seek": self.seek,
            "notify_secs": self.notify_secs,
            "restore": self.restore,
            "mouse": self.mouse,
            "wallust": self.wallust,
        });
        let (mut base, mut look) = (Map::new(), Map::new());
        for (k, x) in v.as_object().into_iter().flatten() {
            if self.in_look(k) {
                look.insert(k.clone(), x.clone());
                // en config.json queda lo que había antes del look
                if let Some(old) = self.base.get(k) {
                    base.insert(k.clone(), old.clone());
                }
            } else {
                base.insert(k.clone(), x.clone());
            }
        }
        write(file(), &base)?;
        if !look.is_empty() {
            write(look_file(), &look)?;
        }
        Ok(())
    }
}
