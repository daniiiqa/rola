//! El ratón: cada dibujo apunta dónde quedó cada cosa (`Hits`) y aquí se
//! traduce el clic a lo mismo que haría su tecla.

use crate::{App, Focus, Popup, Side};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use serde_json::json;
use std::time::{Duration, Instant};

/// Los botones de la barra de abajo.
#[derive(Clone, Copy, PartialEq)]
pub enum Button {
    Play,
    Title,
    Shuffle,
    Repeat,
    Autoplay,
}

/// Dónde se dibujó cada cosa en el último fotograma.
#[derive(Default)]
pub struct Hits {
    pub search: Rect,
    pub side: Rect,  // las filas de la barra lateral
    pub rows: Rect,  // las filas de la tabla (sin la cabecera)
    pub lyrics: Rect,
    pub lyric_rows: Vec<(u16, f64)>, // fila de pantalla → segundo de su línea (letra con tiempos)
    pub bar: Rect,   // toda la barra de abajo
    pub prog: Rect,  // solo el tramo de la barra de progreso
    pub buttons: Vec<(Rect, Button)>,
    pub popup: Rect,
    pub popup_rows: Vec<(u16, usize)>, // fila de pantalla → ajuste (o playlist)
}

/// Un clic más cerca que esto del anterior, en la misma fila, es doble.
const DOUBLE: Duration = Duration::from_millis(400);

fn inside(r: Rect, x: u16, y: u16) -> bool {
    r.contains(Position { x, y })
}

impl App {
    fn press(&mut self, code: KeyCode) {
        self.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        let h = self.hits.take();
        match m.kind {
            MouseEventKind::ScrollDown => self.wheel(&h, x, y, 1),
            MouseEventKind::ScrollUp => self.wheel(&h, x, y, -1),
            MouseEventKind::Down(b) => self.click(&h, x, y, b),
            MouseEventKind::Drag(MouseButton::Left) if inside(h.prog, x, y) => self.seek_to(&h, x),
            _ => {}
        }
        *self.hits.borrow_mut() = h;
    }

    fn wheel(&mut self, h: &Hits, x: u16, y: u16, d: isize) {
        if self.popup.is_some() {
            return self.press(if d > 0 { KeyCode::Down } else { KeyCode::Up });
        }
        if inside(h.side, x, y) {
            self.side_move(d);
        } else if inside(h.rows, x, y) {
            self.table_move(d);
        } else if inside(h.lyrics, x, y) {
            self.lyrics_scroll(d);
        } else if inside(h.bar, x, y) {
            self.set_volume(-(d as i64) * self.cfg.vol_step);
        }
    }

    fn click(&mut self, h: &Hits, x: u16, y: u16, b: MouseButton) {
        let double = self.last_click.is_some_and(|(t, cx, cy)| t.elapsed() < DOUBLE && cy == y && cx.abs_diff(x) < 3);
        self.last_click = Some((Instant::now(), x, y));
        if self.popup.is_some() {
            return self.popup_click(h, x, y, b);
        }
        if inside(h.search, x, y) {
            self.focus = Focus::Search;
        } else if inside(h.side, x, y) {
            let i = self.side_state.offset() + (y - h.side.y) as usize;
            if self.side.get(i).is_some_and(|(_, s)| !matches!(s, Side::Sep | Side::Hint | Side::Head(_))) {
                self.focus = Focus::Side;
                self.side_state.select(Some(i));
                self.side_activate();
            }
        } else if inside(h.rows, x, y) {
            let Some(v) = self.views.last_mut() else { return };
            let i = v.state.offset() + (y - h.rows.y) as usize;
            if i >= self.items().len() {
                return;
            }
            self.focus = Focus::Main;
            if let Some(v) = self.views.last_mut() {
                v.state.select(Some(i));
            }
            match b {
                MouseButton::Right => self.press(KeyCode::Char('z')),
                MouseButton::Left if double => self.press(KeyCode::Enter),
                _ => {}
            }
        } else if inside(h.lyrics, x, y) {
            self.focus = Focus::Lyrics;
            // en la letra con tiempos, clic en una línea salta a ella
            if let Some(&(_, at)) = h.lyric_rows.iter().find(|(r, _)| *r == y) {
                self.mpv.cmd(json!(["seek", at, "absolute"]));
                self.lyrics_hand = None;
            }
        } else if inside(h.prog, x, y) {
            self.seek_to(h, x);
        } else if let Some(&(_, btn)) = h.buttons.iter().find(|(r, _)| inside(*r, x, y)) {
            match btn {
                Button::Play => self.play_pause(),
                Button::Shuffle => self.set_shuffle(!self.shuffle),
                Button::Repeat => self.press(KeyCode::Char('r')),
                Button::Autoplay => self.press(KeyCode::Char('a')),
                Button::Title => {
                    if let Some(i) = self.side.iter().position(|(_, s)| *s == Side::Queue) {
                        self.side_state.select(Some(i));
                        self.side_activate();
                    }
                }
            }
        }
    }

    fn seek_to(&mut self, h: &Hits, x: u16) {
        if !self.loaded || h.prog.width == 0 {
            return;
        }
        let pct = (x - h.prog.x) as f64 / h.prog.width.saturating_sub(1).max(1) as f64 * 100.0;
        self.mpv.cmd(json!(["seek", pct.clamp(0.0, 100.0), "absolute-percent"]));
    }

    fn popup_click(&mut self, h: &Hits, x: u16, y: u16, b: MouseButton) {
        if !inside(h.popup, x, y) || matches!(self.popup, Some(Popup::Help)) {
            // fuera (o en la ayuda) cierra, como Esc; los ajustes se guardan
            return self.press(KeyCode::Esc);
        }
        let Some(&(_, i)) = h.popup_rows.iter().find(|(r, _)| *r == y) else { return };
        match &mut self.popup {
            Some(Popup::Settings { sel }) if *sel == i => {
                self.press(if b == MouseButton::Right { KeyCode::Left } else { KeyCode::Right });
            }
            Some(Popup::Settings { sel }) => *sel = i,
            Some(Popup::AddTo { state, .. }) | Some(Popup::Login { state, .. }) | Some(Popup::Info { state, .. }) => {
                state.select(Some(i));
                self.press(KeyCode::Enter);
            }
            _ => {}
        }
    }
}
