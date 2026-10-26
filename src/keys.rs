//! Teclas propias: `~/.config/rola/keys.json` con `"acción": "tecla"` (o una
//! lista de teclas). La tecla nueva hace lo de la acción; la de siempre sigue
//! valiendo, salvo que se la des a otra. Solo fuera de ventanas y de lo que se
//! escribe (búsqueda, filtro): ahí las teclas son las de siempre.
//!
//! ```json
//! { "pause": "p", "previous": "b", "next": ["ctrl+n", "F9"] }
//! ```

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
use std::collections::HashMap;

/// Cada acción con su tecla de siempre (la que hay en `App::key`).
pub const ACTIONS: &[(&str, &str)] = &[
    ("search", "/"),
    ("filter", "f"),
    ("play", "enter"),
    ("pause", "space"),
    ("next", "n"),
    ("previous", "p"),
    ("seek_back", "<"),
    ("seek_forward", ">"),
    ("volume_up", "+"),
    ("volume_down", "-"),
    ("shuffle", "s"),
    ("repeat", "r"),
    ("autoplay", "a"),
    ("enqueue", "z"),
    ("radio", "R"),
    ("like", "L"),
    ("dislike", "D"),
    ("add_to_playlist", "P"),
    ("remove", "d"),
    ("move_down", "J"),
    ("move_up", "K"),
    ("lyrics", "y"),
    ("cover", "c"),
    ("info", "i"),
    ("sleep", "t"),
    ("sound", "e"),
    ("copy_link", "Y"),
    ("wrapped", "W"),
    ("party", "F"),
    ("settings", "o"),
    ("help", "?"),
    ("down", "j"),
    ("up", "k"),
    ("top", "g"),
    ("bottom", "G"),
    ("left", "h"),
    ("right", "l"),
    ("panel", "tab"),
    ("back", "backspace"),
    ("quit", "q"),
];

pub fn path() -> std::path::PathBuf {
    crate::config::dir().join("keys.json")
}

/// «ctrl+n», «F9», «space», «J»… como tecla. Las letras van tal cual
/// (mayúscula y minúscula son teclas distintas).
pub fn parse(s: &str) -> Option<KeyEvent> {
    let mut mods = KeyModifiers::NONE;
    let mut rest = s.trim();
    loop {
        let low = rest.to_ascii_lowercase();
        let m = if low.starts_with("ctrl+") {
            KeyModifiers::CONTROL
        } else if low.starts_with("alt+") {
            KeyModifiers::ALT
        } else {
            break;
        };
        mods |= m;
        rest = &rest[rest.find('+')? + 1..];
    }
    let code = match rest.to_ascii_lowercase().as_str() {
        "space" | "spc" => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "esc" | "escape" => KeyCode::Esc,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "delete" | "del" | "supr" => KeyCode::Delete,
        f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => KeyCode::F(f[1..].parse().ok()?),
        _ => {
            let mut cs = rest.chars();
            let c = cs.next()?;
            if cs.next().is_some() {
                return None;
            }
            KeyCode::Char(c)
        }
    };
    Some(KeyEvent::new(code, mods))
}

/// Lo que importa de una tecla para compararla: la mayúscula ya va en el
/// carácter, así que el Shift sobra.
fn norm(k: &KeyEvent) -> (KeyCode, KeyModifiers) {
    let mods = match k.code {
        KeyCode::Char(_) => k.modifiers - KeyModifiers::SHIFT,
        _ => k.modifiers,
    };
    (k.code, mods & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT))
}

#[derive(Default)]
pub struct Keys {
    map: HashMap<(KeyCode, KeyModifiers), KeyEvent>, // tecla tuya → la de siempre
    pub shown: Vec<(String, String)>,                // (tecla, acción), para la ayuda
}

impl Keys {
    /// Del archivo (sin archivo: ninguna). Lo que no se entiende, en el Err
    /// junto a lo que sí.
    pub fn load() -> (Keys, Vec<String>) {
        match std::fs::read_to_string(path()) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(v) => Keys::from_json(&v),
                Err(e) => (Keys::default(), vec![format!("keys.json: {e}")]),
            },
            Err(_) => (Keys::default(), Vec::new()),
        }
    }

    pub fn from_json(v: &Value) -> (Keys, Vec<String>) {
        let mut keys = Keys::default();
        let mut bad = Vec::new();
        for (action, val) in v.as_object().into_iter().flatten() {
            let Some(&(_, default)) = ACTIONS.iter().find(|(a, _)| a == action) else {
                bad.push(action.clone());
                continue;
            };
            let target = parse(default).expect("las de siempre se entienden");
            let list: Vec<&str> = match val {
                Value::String(s) => vec![s.as_str()],
                Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            for k in list {
                match parse(k) {
                    Some(ev) => {
                        keys.map.insert(norm(&ev), target);
                        keys.shown.push((k.to_string(), action.clone()));
                    }
                    None => bad.push(format!("{action}: «{k}»")),
                }
            }
        }
        (keys, bad)
    }

    /// La tecla que se pulsó, como la de siempre de su acción (si es tuya).
    pub fn translate(&self, k: KeyEvent) -> KeyEvent {
        match self.map.get(&norm(&k)) {
            Some(t) => KeyEvent { code: t.code, modifiers: t.modifiers, ..k },
            None => k,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_parse() {
        for (a, k) in ACTIONS {
            assert!(parse(k).is_some(), "{a}: {k}");
        }
    }

    #[test]
    fn remaps() {
        let (keys, bad) = Keys::from_json(&json!({ "pause": "p", "next": ["ctrl+n", "F9"], "nope": "x", "radio": "ctrl+" }));
        let ev = |c, m| KeyEvent::new(c, m);
        assert_eq!(keys.translate(ev(KeyCode::Char('p'), KeyModifiers::NONE)).code, KeyCode::Char(' '));
        assert_eq!(keys.translate(ev(KeyCode::Char('n'), KeyModifiers::CONTROL)), ev(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(keys.translate(ev(KeyCode::F(9), KeyModifiers::NONE)).code, KeyCode::Char('n'));
        // la de siempre sigue valiendo, y las demás no se tocan
        assert_eq!(keys.translate(ev(KeyCode::Char(' '), KeyModifiers::NONE)).code, KeyCode::Char(' '));
        assert_eq!(keys.translate(ev(KeyCode::Char('x'), KeyModifiers::NONE)).code, KeyCode::Char('x'));
        // mayúsculas con Shift (como llegan de la terminal) cuentan igual
        let (k2, _) = Keys::from_json(&json!({ "like": "H" }));
        assert_eq!(k2.translate(ev(KeyCode::Char('H'), KeyModifiers::SHIFT)).code, KeyCode::Char('L'));
        assert_eq!(bad, vec!["nope".to_string(), "radio: «ctrl+»".to_string()]);
    }
}
