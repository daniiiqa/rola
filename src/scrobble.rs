//! Lo que escuchas, a ListenBrainz (listenbrainz.org: libre y sin app que
//! registrar, solo el token de tu perfil). «Sonando ahora» al empezar cada
//! canción y la escucha al acabar, si se oyó la mitad (o 4 minutos).

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

const API: &str = "https://api.listenbrainz.org/1";

pub struct Track {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub secs: Option<f64>, // lo que dura
    pub url: Option<String>,
}

pub enum Job {
    Now(Track),
    Listen(Track, i64), // con el segundo unix en que empezó
}

/// ¿Cuenta como escucha? Media canción o 4 minutos, como pide ListenBrainz.
pub fn counts(heard: f64, dur: Option<f64>) -> bool {
    heard >= 240.0 || dur.is_some_and(|d| d > 30.0 && heard >= d / 2.0)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).user_agent("rola (https://github.com/daniiiqa/rola)").build()
}

/// El usuario de ese token, o por qué no vale.
pub fn validate(token: &str) -> Result<String, String> {
    let res = agent().get(&format!("{API}/validate-token")).set("Authorization", &format!("Token {token}")).call();
    let body = match res {
        Ok(r) => r.into_string().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(_, r)) => r.into_string().unwrap_or_default(),
        Err(e) => return Err(e.to_string()),
    };
    let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    match (v["valid"].as_bool(), v["user_name"].as_str()) {
        (Some(true), Some(u)) => Ok(u.to_string()),
        _ => Err(v["message"].as_str().unwrap_or("token no válido / invalid token").to_string()),
    }
}

/// (token, usuario) guardados.
pub fn load(file: &Path) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    Some((v["token"].as_str()?.to_string(), v["user"].as_str().unwrap_or("").to_string()))
}

pub fn save(file: &Path, token: &str, user: &str) -> Result<(), String> {
    std::fs::write(file, json!({ "token": token, "user": user }).to_string()).map_err(|e| e.to_string())?;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())
}

fn body(job: &Job) -> Value {
    let (t, at) = match job {
        Job::Now(t) => (t, None),
        Job::Listen(t, at) => (t, Some(*at)),
    };
    let mut info = json!({ "media_player": "rola", "submission_client": "rola", "music_service": "music.youtube.com" });
    if let Some(s) = t.secs {
        info["duration_ms"] = json!((s * 1000.0) as u64);
    }
    if let Some(u) = &t.url {
        info["origin_url"] = json!(u);
    }
    let mut meta = json!({ "artist_name": t.artist, "track_name": t.title, "additional_info": info });
    if !t.album.is_empty() {
        meta["release_name"] = json!(t.album);
    }
    let mut listen = json!({ "track_metadata": meta });
    if let Some(at) = at {
        listen["listened_at"] = json!(at);
    }
    json!({ "listen_type": if at.is_some() { "single" } else { "playing_now" }, "payload": [listen] })
}

/// Un hilo que envía; las escuchas que fallan (sin red) se reintentan luego.
pub fn start(token: String) -> Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::spawn(move || {
        let agent = agent();
        let send = |j: &Job| {
            agent
                .post(&format!("{API}/submit-listens"))
                .set("Authorization", &format!("Token {token}"))
                .send_string(&body(j).to_string())
                .is_ok()
        };
        let mut pending: VecDeque<Job> = VecDeque::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(job) => {
                    // «sonando ahora» no se guarda: si falla, ya no importa
                    if !send(&job) && matches!(job, Job::Listen(..)) && pending.len() < 500 {
                        pending.push_back(job);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Some(j) = pending.front() {
                if !send(j) {
                    break;
                }
                pending.pop_front();
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_counts() {
        assert!(counts(100.0, Some(180.0)));
        assert!(!counts(80.0, Some(180.0)));
        assert!(counts(250.0, Some(900.0)));
        assert!(!counts(200.0, None));
    }

    #[test]
    fn listen_body() {
        let t = Track { artist: "Neton Vega".into(), title: "RIKI".into(), album: String::new(), secs: Some(181.5), url: Some("https://x".into()) };
        let b = body(&Job::Listen(t, 1_790_000_000));
        assert_eq!(b["listen_type"], "single");
        let l = &b["payload"][0];
        assert_eq!(l["listened_at"], 1_790_000_000);
        assert_eq!(l["track_metadata"]["track_name"], "RIKI");
        assert_eq!(l["track_metadata"]["additional_info"]["duration_ms"], 181_500);
        assert!(l["track_metadata"].get("release_name").is_none());
        let now = body(&Job::Now(Track { artist: "a".into(), title: "b".into(), album: "c".into(), secs: None, url: None }));
        assert_eq!(now["listen_type"], "playing_now");
        assert!(now["payload"][0].get("listened_at").is_none());
    }
}
