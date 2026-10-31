//! El cel por Bluetooth: cuando la música del teléfono suena por la laptop,
//! mpris-proxy (de BlueZ) la vuelve un reproductor MPRIS más («Daniel's S21 Media
//! Player»). rola la enseña en su barra mientras rola misma no suena, y sus teclas
//! (espacio, n, p) manejan al cel. Un hilo mira una vez por segundo.

use crate::Ev;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time::Duration;
use zbus::blocking::{fdo::DBusProxy, Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
/// Así nombra mpris-proxy a lo que llega por Bluetooth.
const PROXY_SUFFIX: &str = " Media Player";

#[derive(Clone, PartialEq, Debug)]
pub struct Cel {
    pub name: String, // «Daniel's S21»
    pub title: String,
    pub artist: String,
    pub pos: Option<f64>, // s
    pub dur: Option<f64>, // s
    pub playing: bool,
    bus: String,
}

fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        // xesam:artist es una lista
        Value::Array(a) => a.iter().map(text).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "),
        Value::Value(inner) => text(inner),
        _ => String::new(),
    }
}

fn micros(v: &Value) -> Option<f64> {
    match v {
        Value::I64(n) => Some(*n as f64 / 1e6),
        Value::U64(n) => Some(*n as f64 / 1e6),
        Value::Value(inner) => micros(inner),
        _ => None,
    }
}

fn read(conn: &Connection, bus: &str) -> Option<Cel> {
    let root = Proxy::new(conn, bus.to_string(), PATH, "org.mpris.MediaPlayer2").ok()?;
    let identity: String = root.get_property("Identity").ok()?;
    let name = identity.strip_suffix(PROXY_SUFFIX)?.to_string();
    let player = Proxy::new(conn, bus.to_string(), PATH, PLAYER).ok()?;
    let status: String = player.get_property("PlaybackStatus").ok()?;
    if status == "Stopped" {
        return None;
    }
    let meta: HashMap<String, OwnedValue> = player.get_property("Metadata").unwrap_or_default();
    let get = |k: &str| meta.get(k).map(|v| text(v)).unwrap_or_default();
    let title = get("xesam:title");
    if title.is_empty() {
        return None;
    }
    let position: Option<i64> = player.get_property("Position").ok();
    Some(Cel {
        name,
        title,
        artist: get("xesam:artist"),
        pos: position.map(|p| p as f64 / 1e6),
        dur: meta.get("mpris:length").and_then(|v| micros(v)),
        playing: status == "Playing",
        bus: bus.to_string(),
    })
}

/// El cel que suena (o, si ninguno, el que está en pausa).
fn scan(conn: &Connection) -> Option<Cel> {
    let names = DBusProxy::new(conn).ok()?.list_names().ok()?;
    let mut found: Vec<Cel> = names
        .iter()
        .map(|n| n.as_str())
        .filter(|n| n.starts_with(PREFIX) && !n.ends_with(".rola") && !n.ends_with(".playerctld"))
        .filter_map(|n| read(conn, n))
        .collect();
    found.sort_by_key(|c| !c.playing);
    found.into_iter().next()
}

/// Mira el cel en un hilo y avisa (Ev::Cel) cada vez que cambia algo.
pub fn watch(tx: Sender<Ev>) {
    std::thread::spawn(move || {
        let Ok(conn) = Connection::session() else { return };
        let mut last: Option<Cel> = None;
        loop {
            let now = scan(&conn);
            if now != last {
                if tx.send(Ev::Cel(now.clone())).is_err() {
                    return;
                }
                last = now;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

impl Cel {
    /// PlayPause, Next, Previous o Pause, al cel (en otro hilo: no espera).
    pub fn send(&self, method: &'static str) {
        let bus = self.bus.clone();
        std::thread::spawn(move || {
            if let Ok(conn) = Connection::session() {
                if let Ok(p) = Proxy::new(&conn, bus, PATH, PLAYER) {
                    let _ = p.call_method(method, &());
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Array;

    #[test]
    fn artists_join() {
        let a = Array::from(vec!["Eslabon Armado", "Peso Pluma"]);
        assert_eq!(text(&Value::Array(a)), "Eslabon Armado, Peso Pluma");
        assert_eq!(text(&Value::from("Loco Loco")), "Loco Loco");
        assert_eq!(micros(&Value::I64(194_443_000)), Some(194.443));
    }
}
