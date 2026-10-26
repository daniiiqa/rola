//! Lo que escuchas, a Last.fm. Como con Spotify, la app la pones tú: en
//! last.fm/api/account/create sale una API key y su secreto; rola pide permiso
//! en el navegador y se queda con la sesión. Lo mismo que ListenBrainz:
//! «sonando ahora» al empezar y la escucha al acabar (ver scrobble.rs).

use crate::scrobble::{Job, Track};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

const API: &str = "https://ws.audioscrobbler.com/2.0/";

/// La app (tuya) y la sesión que te dio Last.fm.
#[derive(Clone)]
pub struct Session {
    pub key: String,
    pub secret: String,
    pub sk: String,
    pub user: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).user_agent("rola (https://github.com/daniiiqa/rola)").build()
}

/// api_sig: los parámetros por orden, pegados, con el secreto al final, en md5.
fn sign(params: &[(&str, &str)], secret: &str) -> String {
    let mut p: Vec<_> = params.iter().filter(|(k, _)| *k != "format").collect();
    p.sort_by_key(|(k, _)| *k);
    let mut s: String = p.iter().map(|(k, v)| format!("{k}{v}")).collect();
    s.push_str(secret);
    md5_hex(s.as_bytes())
}

/// Una llamada firmada; el JSON de vuelta, o el mensaje de error de Last.fm.
/// `retry`: si vale la pena intentarlo luego (sin red, Last.fm caído).
fn call(params: &[(&str, &str)], secret: &str, post: bool) -> Result<Value, (bool, String)> {
    let sig = sign(params, secret);
    let mut all: Vec<(&str, &str)> = params.to_vec();
    all.push(("api_sig", &sig));
    all.push(("format", "json"));
    let res = if post {
        agent().post(API).send_form(&all)
    } else {
        all.iter().fold(agent().get(API), |r, (k, v)| r.query(k, v)).call()
    };
    let body = match res {
        Ok(r) => r.into_string().map_err(|e| (true, e.to_string()))?,
        Err(ureq::Error::Status(code, r)) => {
            let b = r.into_string().unwrap_or_default();
            if code >= 500 {
                return Err((true, format!("HTTP {code}")));
            }
            b
        }
        Err(e) => return Err((true, e.to_string())),
    };
    let v: Value = serde_json::from_str(&body).map_err(|e| (false, e.to_string()))?;
    match v["error"].as_i64() {
        // 11, 16: no disponible ahora mismo; 29: demasiadas
        Some(code) => Err((matches!(code, 11 | 16 | 29), v["message"].as_str().unwrap_or("Last.fm").to_string())),
        None => Ok(v),
    }
}

/// Primer paso: un token para que lo autorices en el navegador.
pub fn token(key: &str, secret: &str) -> Result<String, String> {
    let v = call(&[("method", "auth.getToken"), ("api_key", key)], secret, false).map_err(|e| e.1)?;
    v["token"].as_str().map(String::from).ok_or_else(|| "Last.fm no dio token".into())
}

pub fn auth_url(key: &str, token: &str) -> String {
    format!("https://www.last.fm/api/auth/?api_key={key}&token={token}")
}

/// Segundo: la sesión, en cuanto lo autorizas (se pregunta cada 3 s, 2 min).
pub fn session(key: &str, secret: &str, token: &str) -> Result<Session, String> {
    let mut last = String::new();
    for _ in 0..40 {
        std::thread::sleep(Duration::from_secs(3));
        match call(&[("method", "auth.getSession"), ("api_key", key), ("token", token)], secret, false) {
            Ok(v) => {
                let s = &v["session"];
                return Ok(Session {
                    key: key.into(),
                    secret: secret.into(),
                    sk: s["key"].as_str().ok_or("Last.fm: sin sesión")?.into(),
                    user: s["name"].as_str().unwrap_or("").into(),
                });
            }
            // 14: aún no lo autorizas; lo demás, de verdad
            Err((_, m)) => last = m,
        }
    }
    Err(last)
}

pub fn load(file: &Path) -> Option<Session> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    let s = |k: &str| v[k].as_str().map(String::from);
    Some(Session { key: s("key")?, secret: s("secret")?, sk: s("sk")?, user: s("user").unwrap_or_default() })
}

pub fn save(file: &Path, s: &Session) -> Result<(), String> {
    let v = json!({ "key": s.key, "secret": s.secret, "sk": s.sk, "user": s.user });
    std::fs::write(file, v.to_string()).map_err(|e| e.to_string())?;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())
}

fn params(job: &Job) -> (Vec<(&'static str, String)>, bool) {
    let (t, at): (&Track, Option<i64>) = match job {
        Job::Now(t) => (t, None),
        Job::Listen(t, at) => (t, Some(*at)),
    };
    let method = if at.is_some() { "track.scrobble" } else { "track.updateNowPlaying" };
    let mut p = vec![("method", method.to_string()), ("artist", t.artist.clone()), ("track", t.title.clone())];
    if !t.album.is_empty() {
        p.push(("album", t.album.clone()));
    }
    if let Some(s) = t.secs {
        p.push(("duration", (s.round() as u64).to_string()));
    }
    if let Some(at) = at {
        p.push(("timestamp", at.to_string()));
    }
    (p, at.is_some())
}

/// Un hilo que envía; las escuchas que fallan por la red se reintentan luego.
pub fn start(s: Session) -> Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::spawn(move || {
        // true: enviada o no vale la pena reintentar
        let send = |j: &Job| {
            let (mut p, _) = params(j);
            p.push(("api_key", s.key.clone()));
            p.push(("sk", s.sk.clone()));
            let refs: Vec<(&str, &str)> = p.iter().map(|(k, v)| (*k, v.as_str())).collect();
            !matches!(call(&refs, &s.secret, true), Err((true, _)))
        };
        let mut pending: VecDeque<Job> = VecDeque::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(job) => {
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

// ── md5 (RFC 1321): Last.fm firma así; no vale una dependencia ──────────────

fn md5_hex(input: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = input.to_vec();
    let bits = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend(bits.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    [a0, b0, c0, d0].iter().flat_map(|x| x.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(b"The quick brown fox jumps over the lazy dog"), "9e107d9d372bb6826bd81d3542a419d6");
        assert_eq!(md5_hex(&[b'a'; 1000]), "cabe45dcc9ae5b66ba86600cca6b8ba8");
    }

    #[test]
    fn signature() {
        // orden por clave, sin `format`, el secreto al final
        let p = [("method", "auth.getToken"), ("api_key", "K"), ("format", "json")];
        assert_eq!(sign(&p, "S"), md5_hex(b"api_keyKmethodauth.getTokenS"));
    }

    #[test]
    fn scrobble_params() {
        let t = Track { artist: "Neton Vega".into(), title: "RIKI".into(), album: String::new(), secs: Some(181.5), url: None };
        let (p, post) = params(&Job::Listen(t, 1_790_000_000));
        assert!(post);
        assert!(p.contains(&("method", "track.scrobble".into())));
        assert!(p.contains(&("timestamp", "1790000000".into())));
        assert!(p.contains(&("duration", "182".into())));
        assert!(!p.iter().any(|(k, _)| *k == "album"));
    }
}
