//! El aviso del escritorio con cada canción (org.freedesktop.Notifications,
//! por D-Bus: mako, swaync, dunst…). Con su portada, y cada uno sustituye al
//! anterior en vez de apilarse.

use crate::api::Api;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use zbus::blocking::Connection;
use zbus::zvariant::Value;

pub struct Song {
    pub title: String,
    pub artist: String,
    pub cover: Option<String>,
    pub away_only: bool, // solo si no estás mirando rola
}

/// Un hilo que avisa; None sin bus de sesión.
pub fn start(api: Api) -> Option<Sender<Song>> {
    let conn = Connection::session().ok()?;
    let (tx, rx) = mpsc::channel::<Song>();
    std::thread::spawn(move || {
        let dir = PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()));
        let mut last = 0u32;
        let mut n = 0;
        while let Ok(mut song) = rx.recv() {
            // saltando canciones rápido: solo la última
            while let Ok(s) = rx.try_recv() {
                song = s;
            }
            if song.away_only && crate::hypr::looking_at_me() {
                continue;
            }
            // dos archivos por turnos: el aviso anterior aún puede estar leyendo el suyo
            n = (n + 1) % 2;
            let file = dir.join(format!("rola-cover-{}-{n}.png", std::process::id()));
            let image = song
                .cover
                .and_then(|u| api.get_bytes(&u).ok())
                .and_then(|b| image::load_from_memory(&b).ok())
                .and_then(|img| crate::api::square(img).thumbnail(256, 256).save(&file).ok())
                .map(|_| file.display().to_string())
                .unwrap_or_default();
            let mut hints: HashMap<&str, Value> = HashMap::new();
            hints.insert("urgency", Value::U8(0));
            if !image.is_empty() {
                hints.insert("image-path", Value::from(image.clone()));
            }
            let body = (
                "rola",
                last,
                image.as_str(),
                song.title.as_str(),
                song.artist.as_str(),
                Vec::<&str>::new(),
                hints,
                -1i32,
            );
            let reply = conn.call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &body,
            );
            if let Ok(id) = reply.and_then(|m| m.body().deserialize::<u32>()) {
                last = id;
            }
        }
        for n in 0..2 {
            let _ = std::fs::remove_file(dir.join(format!("rola-cover-{}-{n}.png", std::process::id())));
        }
    });
    Some(tx)
}
