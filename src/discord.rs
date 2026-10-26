//! «Escuchando una rola» en Discord (Rich Presence, por su socket local:
//! sin librerías). La canción, el artista, la portada, la barra de progreso y
//! un botón para escucharla. Si Discord no está abierto (o se reinicia), se
//! reintenta solo.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq)]
pub struct Song {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<String>,
    pub link: Option<(String, String)>, // (texto del botón, enlace)
    pub start_ms: i64,                  // cuándo habría empezado, contando lo ya oído
    pub end_ms: Option<i64>,
}

/// Lo que se ve: una canción sonando, o nada (en pausa o parado).
pub type Presence = Option<Song>;

/// Donde escucha Discord: el normal, Flatpak, Snap, Vesktop/arRPC…
fn sockets() -> Vec<PathBuf> {
    let run = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let dirs = [
        PathBuf::from(&run),
        PathBuf::from(&run).join("app/com.discordapp.Discord"),
        PathBuf::from(&run).join("app/dev.vencord.Vesktop"),
        PathBuf::from(&run).join("snap.discord"),
        PathBuf::from("/tmp"),
    ];
    dirs.iter().flat_map(|d| (0..10).map(move |i| d.join(format!("discord-ipc-{i}")))).collect()
}

fn frame(op: u32, v: &Value) -> Vec<u8> {
    let body = v.to_string();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body.as_bytes());
    out
}

fn read(s: &mut UnixStream) -> std::io::Result<(u32, Value)> {
    let mut head = [0u8; 8];
    s.read_exact(&mut head)?;
    let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    let mut body = vec![0; len.min(1 << 20)];
    s.read_exact(&mut body)?;
    Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

fn connect(app: &str) -> Option<UnixStream> {
    for p in sockets() {
        let Ok(mut s) = UnixStream::connect(&p) else { continue };
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        if s.write_all(&frame(0, &json!({ "v": 1, "client_id": app }))).is_err() {
            continue;
        }
        // READY (op 1); un error (otro Application ID, …) cierra
        if matches!(read(&mut s), Ok((1, v)) if v["evt"] == "READY") {
            return Some(s);
        }
    }
    None
}

/// Lo que se manda para esa canción (o para borrarla).
pub fn activity(p: &Presence, nonce: u64) -> Value {
    let Some(s) = p else {
        return json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id() }, "nonce": nonce.to_string() });
    };
    // Discord pide 2..128 caracteres en cada texto
    let fit = |t: &str| {
        let t: String = t.chars().take(128).collect();
        if t.chars().count() < 2 {
            format!("{t}  ")
        } else {
            t
        }
    };
    let mut a = json!({
        "type": 2, // «Escuchando»
        // en la lista de miembros, la canción («♫ Me Matas») en vez del nombre de la app
        "status_display_type": 2,
        "details": fit(&s.title),
        "state": fit(&s.artist),
        "timestamps": { "start": s.start_ms },
    });
    if let Some(end) = s.end_ms {
        a["timestamps"]["end"] = json!(end);
    }
    if let Some(c) = &s.cover {
        a["assets"] = json!({ "large_image": c });
        if !s.album.is_empty() {
            a["assets"]["large_text"] = json!(fit(&s.album));
        }
    }
    if let Some((label, url)) = &s.link {
        a["buttons"] = json!([{ "label": label, "url": url }]);
    }
    json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": a }, "nonce": nonce.to_string() })
}

/// Un hilo que habla con Discord. Se le manda lo que debe verse; él junta
/// los cambios seguidos (Discord admite pocos por minuto) y reconecta.
pub fn start(app: String) -> Sender<Presence> {
    let (tx, rx) = mpsc::channel::<Presence>();
    std::thread::spawn(move || {
        let mut conn: Option<UnixStream> = None;
        let mut want: Option<Presence> = None; // pendiente de mandar
        let mut sent: Option<Presence> = None;
        let mut last = Instant::now() - Duration::from_secs(60);
        let mut tried = Instant::now() - Duration::from_secs(60);
        let mut nonce = 0u64;
        loop {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(p) => want = Some(p),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            // lo último que haya llegado
            while let Ok(p) = rx.try_recv() {
                want = Some(p);
            }
            let Some(p) = want.clone() else { continue };
            if sent.as_ref() == Some(&p) {
                want = None;
                continue;
            }
            // no más de uno cada 4 s
            if last.elapsed() < Duration::from_secs(4) {
                continue;
            }
            if conn.is_none() && tried.elapsed() > Duration::from_secs(15) {
                tried = Instant::now();
                conn = connect(&app);
            }
            let Some(s) = conn.as_mut() else { continue };
            nonce += 1;
            let ok = s.write_all(&frame(1, &activity(&p, nonce))).is_ok() && read(s).is_ok();
            if ok {
                sent = Some(p);
                want = None;
                last = Instant::now();
            } else {
                conn = None; // Discord se cerró: a reconectar
            }
        }
        // al salir, que no se quede la canción puesta
        if let Some(mut s) = conn {
            let _ = s.write_all(&frame(1, &activity(&None, nonce + 1)));
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames() {
        let f = frame(1, &json!({ "a": 1 }));
        assert_eq!(&f[..4], &1u32.to_le_bytes());
        assert_eq!(&f[4..8], &7u32.to_le_bytes());
        assert_eq!(&f[8..], br#"{"a":1}"#);
    }

    #[test]
    fn activity_json() {
        let s = Song {
            title: "x".into(),
            artist: "Neton Vega".into(),
            album: String::new(),
            cover: Some("https://i.ytimg.com/vi/a/hqdefault.jpg".into()),
            link: Some(("Escuchar".into(), "https://music.youtube.com/watch?v=a".into())),
            start_ms: 1000,
            end_ms: Some(181_000),
        };
        let v = activity(&Some(s), 7);
        let a = &v["args"]["activity"];
        assert_eq!(a["type"], 2);
        assert_eq!(a["status_display_type"], 2);
        assert_eq!(a["details"], "x  "); // mínimo 2 caracteres
        assert_eq!(a["timestamps"]["end"], 181_000);
        assert!(a["assets"].get("large_text").is_none());
        assert_eq!(a["buttons"][0]["label"], "Escuchar");
        assert_eq!(v["nonce"], "7");
        // borrar: sin activity
        assert!(activity(&None, 8)["args"].get("activity").is_none());
    }
}
