//! `rola next | prev | pause | play [búsqueda]`: manejar el rola que está
//! abierto (o sonando de fondo) desde la línea de órdenes, para atajos de
//! Hyprland o un menú de fuzzel/rofi. Va por su MPRIS y se acaba: nada más
//! corriendo. `play` con búsqueda la busca aquí y le pasa la canción (OpenUri);
//! allí suena con su radio detrás.

use crate::api::{Api, Kind};
use crate::i18n::{fill, Strings};
use zbus::blocking::Connection;

const DEST: &str = "org.mpris.MediaPlayer2.rola";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// Las órdenes que se entienden; None si `args` no es una.
pub fn run(args: &[String], s: &Strings, auth: &std::path::Path) -> Option<i32> {
    let (cmd, rest) = args.split_first()?;
    let method = match cmd.as_str() {
        "next" => "Next",
        "prev" | "previous" => "Previous",
        "pause" => "PlayPause",
        "play" if rest.is_empty() => "Play",
        "play" => "OpenUri",
        _ => return None,
    };
    // sin rola abierto no hay a quién llamar
    let call = |body: &dyn Fn(&Connection) -> zbus::Result<zbus::message::Message>| match Connection::session().and_then(|c| body(&c)) {
        Ok(_) => 0,
        Err(_) => {
            eprintln!("{}", s.remote_not_running);
            1
        }
    };
    if method != "OpenUri" {
        return Some(call(&|c| c.call_method(Some(DEST), PATH, Some(PLAYER), method, &())));
    }
    // play «algo»: la mejor canción de la búsqueda (si no, lo primero que suene)
    let q = rest.join(" ");
    let found = Api::new(auth).search(&q).unwrap_or_default();
    let Some(it) = found.iter().find(|i| i.kind == Kind::Song).or_else(|| found.iter().find(|i| i.playable())) else {
        eprintln!("{}", fill(s.remote_not_found, &[&q]));
        return Some(1);
    };
    let uri = format!("https://music.youtube.com/watch?v={}", it.video_id.as_deref().unwrap_or(""));
    let code = call(&|c| c.call_method(Some(DEST), PATH, Some(PLAYER), "OpenUri", &(uri.as_str(),)));
    if code == 0 {
        println!("{}", fill(s.remote_playing, &[&it.title, &it.artist]));
    }
    Some(code)
}

/// El video de un enlace de YouTube (o YouTube Music), o el id tal cual.
pub fn video_id(uri: &str) -> Option<String> {
    let id = if let Some((_, q)) = uri.split_once('?') {
        q.split('&').find_map(|p| p.strip_prefix("v="))?
    } else if let Some(rest) = uri.strip_prefix("https://youtu.be/") {
        rest
    } else {
        uri
    };
    (id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert_eq!(video_id("https://music.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(video_id("https://www.youtube.com/watch?list=x&v=dQw4w9WgXcQ&t=3").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(video_id("https://youtu.be/dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(video_id("dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(video_id("https://example.com/nope"), None);
    }
}
