//! Lo poco que se le pregunta a Hyprland, por su socket (sin lanzar hyprctl).

use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

fn socket() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let run = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    // Hyprland ≥ 0.40 lo pone en XDG_RUNTIME_DIR; antes, en /tmp
    [PathBuf::from(run).join("hypr"), PathBuf::from("/tmp/hypr")]
        .into_iter()
        .map(|d| d.join(&sig).join(".socket.sock"))
        .find(|p| p.exists())
}

/// El redondeo de las ventanas (`decoration:rounding`); None fuera de Hyprland.
pub fn rounding() -> Option<i64> {
    let mut s = UnixStream::connect(socket()?).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
    s.write_all(b"j/getoption decoration:rounding").ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    serde_json::from_str::<Value>(&out).ok()?["int"].as_i64()
}
