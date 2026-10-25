//! El menú de ajustes (`o`): cada cambio se ve al momento y se guarda al cerrar.

use crate::config::{self, step_in, Choice};
use crate::i18n::Strings;
use crate::{theme, App, Popup};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use serde_json::json;

#[derive(Clone, Copy, PartialEq)]
pub enum Setting {
    Account,
    Spotify,
    Lang,
    Restore,
    Mouse,
    Notify,
    Desktop,
    Theme,
    Background,
    Borders,
    Bar,
    Labels,
    Case,
    Select,
    Tint,
    Side,
    Lyrics,
    Footer,
    Motion,
    Cover,
    Quality,
    VolStep,
    Seek,
    Autoplay,
}

/// Título de la sección y sus ajustes.
type Section = (fn(&Strings) -> &'static str, &'static [Setting]);

/// Secciones con sus ajustes, en el orden en que salen.
pub const SECTIONS: &[Section] = &[
    (|s| s.sec_general, &[Setting::Account, Setting::Spotify, Setting::Lang, Setting::Restore, Setting::Mouse, Setting::Notify, Setting::Desktop]),
    (
        |s| s.sec_look,
        &[
            Setting::Theme,
            Setting::Background,
            Setting::Borders,
            Setting::Bar,
            Setting::Labels,
            Setting::Case,
            Setting::Select,
            Setting::Tint,
            Setting::Side,
            Setting::Lyrics,
            Setting::Footer,
            Setting::Motion,
            Setting::Cover,
        ],
    ),
    (|s| s.sec_playback, &[Setting::Quality, Setting::VolStep, Setting::Seek, Setting::Autoplay]),
];

pub fn all() -> impl Iterator<Item = Setting> {
    SECTIONS.iter().flat_map(|(_, v)| v.iter().copied())
}

impl Setting {
    pub fn label(self, s: &Strings) -> &'static str {
        match self {
            Setting::Account => s.set_account,
            Setting::Spotify => s.set_spotify,
            Setting::Lang => s.set_lang,
            Setting::Restore => s.set_restore,
            Setting::Mouse => s.set_mouse,
            Setting::Notify => s.set_notify,
            Setting::Desktop => s.set_desktop,
            Setting::Theme => s.set_theme,
            Setting::Background => s.set_background,
            Setting::Borders => s.set_borders,
            Setting::Bar => s.set_bar,
            Setting::Labels => s.set_labels,
            Setting::Case => s.set_case,
            Setting::Select => s.set_select,
            Setting::Tint => s.set_tint,
            Setting::Side => s.set_side,
            Setting::Lyrics => s.set_lyrics,
            Setting::Footer => s.set_footer,
            Setting::Motion => s.set_motion,
            Setting::Cover => s.set_cover,
            Setting::Quality => s.set_quality,
            Setting::VolStep => s.set_vol_step,
            Setting::Seek => s.set_seek,
            Setting::Autoplay => s.set_autoplay,
        }
    }

    /// Su clave en config.json / look.json (portada y autoplay van con la sesión).
    pub fn key(self) -> Option<&'static str> {
        Some(match self {
            Setting::Lang => "lang",
            Setting::Restore => "restore",
            Setting::Mouse => "mouse",
            Setting::Notify => "notify_secs",
            Setting::Desktop => "desktop",
            Setting::Theme => "theme",
            Setting::Background => "transparent",
            Setting::Borders => "borders",
            Setting::Bar => "bar",
            Setting::Labels => "labels",
            Setting::Case => "case",
            Setting::Select => "select",
            Setting::Tint => "tint",
            Setting::Side => "side_width",
            Setting::Lyrics => "lyrics_width",
            Setting::Footer => "footer",
            Setting::Motion => "motion",
            Setting::Quality => "quality",
            Setting::VolStep => "vol_step",
            Setting::Seek => "seek",
            Setting::Account | Setting::Spotify | Setting::Cover | Setting::Autoplay => return None,
        })
    }
}

fn idx<T: Choice>(x: T) -> usize {
    T::ALL.iter().position(|&y| y == x).unwrap_or(0)
}

impl App {
    pub fn setting_value(&self, k: Setting) -> String {
        let (c, s) = (&self.cfg, self.s());
        let yn = |b: bool| if b { s.on } else { s.off }.to_string();
        match k {
            Setting::Account => self.account_name(),
            Setting::Spotify => self.spotify.as_ref().map(|s| s.user()).unwrap_or_else(|| s.account_none.into()),
            Setting::Lang => s.lang_name.into(),
            Setting::Restore => yn(c.restore),
            Setting::Mouse => yn(c.mouse),
            Setting::Notify => format!("{} s", c.notify_secs),
            Setting::Desktop => s.desktops[idx(c.desktop)].into(),
            Setting::Theme => c.theme.clone(),
            Setting::Background => if c.transparent { s.bg_terminal } else { s.bg_theme }.into(),
            Setting::Borders if c.borders == config::Borders::Auto => {
                let now = if self.rounded { s.borders[1] } else { s.borders[2] };
                format!("{} ({now})", s.borders[0])
            }
            Setting::Borders => s.borders[idx(c.borders)].into(),
            Setting::Bar => s.bars[idx(c.bar)].into(),
            Setting::Labels => s.labels[idx(c.labels)].into(),
            Setting::Case => s.cases[idx(c.case)].into(),
            Setting::Select => s.selects[idx(c.select)].into(),
            Setting::Tint => s.tints[idx(c.tint)].into(),
            Setting::Side => c.side_width.to_string(),
            Setting::Lyrics => format!("{}%", c.lyrics_width),
            Setting::Footer => yn(c.footer),
            Setting::Motion => yn(c.motion),
            Setting::Cover => yn(self.cover_on),
            Setting::Quality => s.qualities[idx(c.quality)].into(),
            Setting::VolStep => format!("{}%", c.vol_step),
            Setting::Seek => format!("{} s", c.seek),
            Setting::Autoplay => yn(self.autoplay),
        }
    }

    fn change(&mut self, k: Setting, d: isize) {
        if matches!(k, Setting::Account | Setting::Spotify) {
            // los ajustes se guardan aquí: el popup de la cuenta los sustituye
            let _ = self.cfg.save();
            self.cfg_mtimes = config::mtimes();
            return if k == Setting::Account { self.open_login() } else { self.open_spotify() };
        }
        let c = &mut self.cfg;
        match k {
            Setting::Account | Setting::Spotify => {}
            Setting::Lang => {
                let old = c.lang;
                c.lang = c.lang.step(d);
                self.retitle(old);
            }
            Setting::Restore => c.restore = !c.restore,
            Setting::Mouse => {
                c.mouse = !c.mouse;
                crate::mouse_capture(c.mouse);
            }
            Setting::Notify => c.notify_secs = step_in(config::NOTIFY_SECS, c.notify_secs, d),
            Setting::Desktop => c.desktop = c.desktop.step(d),
            Setting::Theme => {
                let names = theme::names();
                let i = names.iter().position(|n| *n == c.theme).unwrap_or(0) as isize;
                c.theme = names[(i + d).rem_euclid(names.len() as isize) as usize].clone();
            }
            Setting::Background => c.transparent = !c.transparent,
            Setting::Borders => c.borders = c.borders.step(d),
            Setting::Bar => c.bar = c.bar.step(d),
            Setting::Labels => c.labels = c.labels.step(d),
            Setting::Case => c.case = c.case.step(d),
            Setting::Select => c.select = c.select.step(d),
            Setting::Tint => c.tint = c.tint.step(d),
            Setting::Side => c.side_width = step_in(config::SIDE_WIDTHS, c.side_width, d),
            Setting::Lyrics => c.lyrics_width = step_in(config::LYRICS_WIDTHS, c.lyrics_width, d),
            Setting::Footer => c.footer = !c.footer,
            Setting::Motion => c.motion = !c.motion,
            Setting::Cover => {
                self.cover_on = !self.cover_on;
                self.fetch_cover();
            }
            Setting::Quality => {
                c.quality = c.quality.step(d);
                let f = c.quality.ytdl_format();
                self.mpv.cmd(json!(["set_property", "ytdl-format", f]));
                self.streams.clear();
                if self.loaded {
                    self.notify(self.s().quality_next, false);
                }
            }
            Setting::VolStep => c.vol_step = step_in(config::VOL_STEPS, c.vol_step, d),
            Setting::Seek => c.seek = step_in(config::SEEKS, c.seek, d),
            Setting::Autoplay => self.autoplay = !self.autoplay,
        }
        self.reload_theme();
    }

    pub fn settings_key(&mut self, k: KeyEvent) {
        let Some(Popup::Settings { sel }) = &mut self.popup else { return };
        let n = all().count();
        let cur = *sel;
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('o') | KeyCode::Char(',') => {
                self.popup = None;
                match self.cfg.save() {
                    Ok(()) => self.notify(self.s().saved, false),
                    Err(e) => self.notify(e, true),
                }
                self.cfg_mtimes = config::mtimes();
            }
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => *sel = (cur + 1) % n,
            KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => *sel = (cur + n - 1) % n,
            KeyCode::Char('g') | KeyCode::Home => *sel = 0,
            KeyCode::Char('G') | KeyCode::End => *sel = n - 1,
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
                self.change(all().nth(cur).unwrap_or(Setting::Lang), 1)
            }
            KeyCode::Char('h') | KeyCode::Left => self.change(all().nth(cur).unwrap_or(Setting::Lang), -1),
            _ => {}
        }
    }
}
