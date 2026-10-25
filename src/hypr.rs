//! Lo poco que se le pregunta a Hyprland, por su socket (sin lanzar hyprctl).

use crate::Ev;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

fn socket(name: &str) -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let run = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    // Hyprland ≥ 0.40 lo pone en XDG_RUNTIME_DIR; antes, en /tmp
    [PathBuf::from(run).join("hypr"), PathBuf::from("/tmp/hypr")]
        .into_iter()
        .map(|d| d.join(&sig).join(name))
        .find(|p| p.exists())
}

/// El redondeo de las ventanas (`decoration:rounding`); None fuera de Hyprland.
pub fn rounding() -> Option<i64> {
    let mut s = UnixStream::connect(socket(".socket.sock")?).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
    s.write_all(b"j/getoption decoration:rounding").ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    serde_json::from_str::<Value>(&out).ok()?["int"].as_i64()
}

/// ¿La ventana con el foco es la terminal de rola? (su proceso es uno de
/// nuestros antepasados). Fuera de Hyprland no se sabe: no.
pub fn looking_at_me() -> bool {
    let active = (|| {
        let mut s = UnixStream::connect(socket(".socket.sock")?).ok()?;
        s.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
        s.write_all(b"j/activewindow").ok()?;
        let mut out = String::new();
        s.read_to_string(&mut out).ok()?;
        serde_json::from_str::<Value>(&out).ok()?["pid"].as_u64()
    })();
    let Some(active) = active else { return false };
    let mut pid = std::process::id() as u64;
    while pid > 1 {
        if pid == active {
            return true;
        }
        // el padre: cuarto campo de /proc/<pid>/stat (tras el nombre entre paréntesis)
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { break };
        let Some(ppid) = stat.rsplit_once(')').and_then(|(_, r)| r.split_whitespace().nth(1)?.parse().ok()) else { break };
        pid = ppid;
    }
    false
}

/// Avisa cada vez que Hyprland recarga su config: es lo último que hace
/// LookSwitch.sh, así que para entonces el look nuevo ya está entero.
pub fn listen(tx: Sender<Ev>) {
    let Some(path) = socket(".socket2.sock") else { return };
    std::thread::spawn(move || {
        let Ok(s) = UnixStream::connect(path) else { return };
        for line in BufReader::new(s).lines() {
            let Ok(line) = line else { break };
            if line.starts_with("configreloaded>>") && tx.send(Ev::Look).is_err() {
                break;
            }
        }
    });
}
