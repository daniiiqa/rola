//! Enlaces directos al audio, resueltos con yt-dlp antes de hacer falta: mpv
//! tarda ~3 s en empezar una canción si tiene que preguntarle él a yt-dlp, y
//! ~0.3 s si ya le das el enlace.

use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn resolve(video_id: &str, format: &str) -> Option<String> {
    let out = Command::new("yt-dlp")
        .args(["-q", "--no-warnings", "--no-playlist", "-f", format, "-g"])
        // el mismo cliente que usa mpv (el de por defecto da 403 al stremear)
        .args(["--extractor-args", "youtube:player_client=web_music"])
        .arg(format!("https://music.youtube.com/watch?v={video_id}"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let url = String::from_utf8(out.stdout).ok()?.lines().next()?.trim().to_string();
    (out.status.success() && url.starts_with("http")).then_some(url)
}

/// Los enlaces de googlevideo caducan (`expire=` en segundos unix); con un
/// par de minutos de margen para que no muera a medio cargar.
pub fn fresh(url: &str) -> bool {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("expire="))
        .and_then(|e| e.parse::<u64>().ok())
        .is_some_and(|e| e > now + 120)
}
