//! Enlaces directos al audio, resueltos con yt-dlp antes de hacer falta.
//!
//! YouTube da el audio bueno (opus/aac ~130k) a los clientes normales, pero el
//! enlace no sirve (403) hasta unos segundos después de pedirlo: yt-dlp lo
//! cuenta en `available_at`. Resolviéndolo por adelantado (la siguiente, la
//! del cursor) ese rato ya pasó cuando toca sonar.

use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Link {
    pub url: String,
    ready: u64, // segundos unix desde los que deja de dar 403
}

pub fn resolve(video_id: &str, format: &str) -> Option<Link> {
    let out = Command::new("yt-dlp")
        .args(["-q", "--no-warnings", "--no-playlist", "-f", format])
        .args(["--print", "%(available_at|0)s %(url)s"])
        .arg(format!("https://music.youtube.com/watch?v={video_id}"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let line = String::from_utf8(out.stdout).ok()?;
    let (ready, url) = line.lines().next()?.trim().split_once(' ')?;
    let link = Link { url: url.to_string(), ready: ready.parse().unwrap_or(0) };
    (out.status.success() && link.url.starts_with("http")).then_some(link)
}

fn now() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}

impl Link {
    /// Los enlaces de googlevideo caducan (`expire=` en segundos unix); con un
    /// par de minutos de margen para que no muera a medio cargar.
    pub fn fresh(&self) -> bool {
        self.url
            .split(['?', '&'])
            .find_map(|p| p.strip_prefix("expire="))
            .and_then(|e| e.parse::<u64>().ok())
            .is_some_and(|e| e > now().as_secs() + 120)
    }

    /// Lo que falta para poder usarlo (cero si ya se puede).
    pub fn wait(&self) -> Duration {
        // `available_at` va en segundos enteros: medio de margen
        (Duration::from_secs(self.ready) + Duration::from_millis(500)).saturating_sub(now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_timing() {
        let exp = now().as_secs() + 3600;
        let ready = Link { url: format!("https://x/videoplayback?expire={exp}&itag=251"), ready: 0 };
        assert!(ready.fresh());
        assert!(ready.wait().is_zero());
        let later = Link { ready: now().as_secs() + 4, ..ready.clone() };
        assert!(later.wait() > Duration::from_secs(3));
        let old = Link { url: "https://x/videoplayback?expire=10".into(), ready: 0 };
        assert!(!old.fresh());
    }
}
