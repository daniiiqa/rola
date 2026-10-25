//! Tu biblioteca de Spotify dentro de rola: Me gusta, álbumes guardados y las
//! playlists tuyas (o en las que colaboras: desde 2026 la API no da el
//! contenido de las demás). Suena por YouTube Music: cada canción se busca
//! allí y se recuerda la elegida.
//!
//! Entrar es OAuth con PKCE contra una app de desarrollador propia (Spotify ya
//! no deja compartir una): sin secreto, solo el Client ID y un token renovable.

use crate::api::{Api, Item, Kind};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const REDIRECT: &str = "http://127.0.0.1:8898/callback";
const ADDR: &str = "127.0.0.1:8898";
const API: &str = "https://api.spotify.com/v1";
const SCOPES: &str = "user-library-read playlist-read-private playlist-read-collaborative";

#[derive(Clone)]
pub struct Spotify {
    agent: ureq::Agent,
    file: PathBuf,
    tok: Arc<Mutex<Value>>, // client_id, refresh, access, expires, user, user_id
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn random(n: usize) -> String {
    const CH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut buf = vec![0u8; n];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf));
    buf.iter().map(|b| CH[*b as usize % CH.len()] as char).collect()
}

fn b64url(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = c.iter().enumerate().fold(0u32, |a, (i, &b)| a | (b as u32) << (16 - 8 * i));
        for i in 0..c.len() + 1 {
            out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&")
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build()
}

fn json_of(r: ureq::Response) -> Result<Value, String> {
    serde_json::from_str(&r.into_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn http_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            let v: Value = serde_json::from_str(&body).unwrap_or_default();
            let msg = v["error_description"].as_str().or(v["error"]["message"].as_str()).or(v["error"].as_str()).unwrap_or("");
            format!("Spotify: HTTP {code} {msg}")
        }
        e => format!("Spotify: {e}"),
    }
}

/// Pide un token a accounts.spotify.com (con código o renovando).
fn token(agent: &ureq::Agent, pairs: &[(&str, &str)]) -> Result<Value, String> {
    agent
        .post("https://accounts.spotify.com/api/token")
        .set("content-type", "application/x-www-form-urlencoded")
        .send_string(&form(pairs))
        .map_err(http_err)
        .and_then(json_of)
}

/// El navegador vuelve a 127.0.0.1:8898 con el código; esto lo espera
/// (hasta 3 minutos) y le contesta con una página que se puede cerrar.
fn wait_code(listener: &TcpListener, state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_secs(180);
    while Instant::now() < until {
        let Ok((mut s, _)) = listener.accept() else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let _ = s.set_nonblocking(false);
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        let _ = BufReader::new(&s).read_line(&mut line);
        let query = line.split_whitespace().nth(1).and_then(|p| p.strip_prefix("/callback?")).unwrap_or("");
        let get = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}="))).map(String::from);
        if !line.contains("/callback") {
            let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            continue; // favicon y demás
        }
        let (code, ok) = match (get("code"), get("state")) {
            (Some(c), Some(st)) if st == state => (Ok(c), true),
            _ => (Err(get("error").unwrap_or_else(|| "denied".into())), false),
        };
        let msg = if ok { "rola: Spotify conectado / connected ✓ — puedes cerrar esta pestaña / you can close this tab" } else { "rola: Spotify no conectado / not connected" };
        let page = format!("<!doctype html><meta charset=utf-8><body style=\"font:16px sans-serif;padding:3em\">{msg}");
        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
        return code.map_err(|e| format!("Spotify: {e}"));
    }
    Err("Spotify: timeout".into())
}

impl Spotify {
    pub fn load(file: &Path) -> Option<Spotify> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
        v["refresh"].as_str()?;
        Some(Spotify { agent: agent(), file: file.to_path_buf(), tok: Arc::new(Mutex::new(v)) })
    }

    /// Abre el navegador para que des permiso y espera la vuelta.
    pub fn login(client_id: &str, file: &Path) -> Result<Spotify, String> {
        let client_id = client_id.trim();
        let verifier = random(64);
        let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
        let state = random(16);
        // escuchar antes de abrir el navegador, por si vuelve muy rápido
        let listener = TcpListener::bind(ADDR).map_err(|e| format!("{ADDR}: {e}"))?;
        let url = format!(
            "https://accounts.spotify.com/authorize?{}",
            form(&[
                ("response_type", "code"),
                ("client_id", client_id),
                ("redirect_uri", REDIRECT),
                ("scope", SCOPES),
                ("state", &state),
                ("code_challenge_method", "S256"),
                ("code_challenge", &challenge),
            ])
        );
        let _ = std::process::Command::new("xdg-open")
            .arg(&url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        let code = wait_code(&listener, &state)?;
        let agent = agent();
        let t = token(
            &agent,
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT),
                ("client_id", client_id),
                ("code_verifier", &verifier),
            ],
        )?;
        let tok = json!({
            "client_id": client_id,
            "refresh": t["refresh_token"],
            "access": t["access_token"],
            "expires": now() + t["expires_in"].as_u64().unwrap_or(3600),
        });
        let sp = Spotify { agent, file: file.to_path_buf(), tok: Arc::new(Mutex::new(tok)) };
        let me = sp.get(&format!("{API}/me"))?;
        if let Ok(mut t) = sp.tok.lock() {
            t["user"] = json!(me["display_name"].as_str().or(me["id"].as_str()).unwrap_or("Spotify"));
            t["user_id"] = me["id"].clone();
        }
        sp.save()?;
        Ok(sp)
    }

    fn save(&self) -> Result<(), String> {
        let v = self.tok.lock().map(|t| t.clone()).unwrap_or_default();
        crate::login::save(&self.file, &v)
    }

    pub fn user(&self) -> String {
        self.tok.lock().ok().and_then(|t| t["user"].as_str().map(String::from)).unwrap_or_default()
    }

    pub fn client_id(&self) -> String {
        self.tok.lock().ok().and_then(|t| t["client_id"].as_str().map(String::from)).unwrap_or_default()
    }

    /// El token de acceso, renovado si está por caducar (o si `force`).
    fn access(&self, force: bool) -> Result<String, String> {
        let mut t = self.tok.lock().map_err(|e| e.to_string())?;
        if !force && t["expires"].as_u64().unwrap_or(0) > now() + 60 {
            return Ok(t["access"].as_str().unwrap_or("").to_string());
        }
        let (refresh, client) = (t["refresh"].as_str().unwrap_or("").to_string(), t["client_id"].as_str().unwrap_or("").to_string());
        let n = token(&self.agent, &[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &client)])?;
        t["access"] = n["access_token"].clone();
        t["expires"] = json!(now() + n["expires_in"].as_u64().unwrap_or(3600));
        // a veces trae uno nuevo; si no, sigue valiendo el de antes
        if n["refresh_token"].is_string() {
            t["refresh"] = n["refresh_token"].clone();
        }
        let v = t.clone();
        drop(t);
        let _ = crate::login::save(&self.file, &v);
        Ok(v["access"].as_str().unwrap_or("").to_string())
    }

    fn get(&self, url: &str) -> Result<Value, String> {
        let mut force = false;
        for _ in 0..3 {
            let tok = self.access(force)?;
            match self.agent.get(url).set("authorization", &format!("Bearer {tok}")).call() {
                Ok(r) => return json_of(r),
                Err(ureq::Error::Status(401, _)) => force = true,
                Err(ureq::Error::Status(429, r)) => {
                    let s = r.header("retry-after").and_then(|s| s.parse().ok()).unwrap_or(2u64);
                    std::thread::sleep(Duration::from_secs(s.min(10)));
                }
                Err(e) => return Err(http_err(e)),
            }
        }
        Err("Spotify: no responde".into())
    }

    /// Todas las páginas de una lista (`items` y `next`), hasta 40.
    fn paged(&self, url: &str, inner: Option<&str>) -> Result<Vec<Value>, String> {
        let (mut out, mut next) = (Vec::new(), Some(url.to_string()));
        for _ in 0..40 {
            let Some(u) = next.take() else { break };
            let v = self.get(&u)?;
            let page = inner.map_or(&v, |k| &v[k]);
            out.extend(page["items"].as_array().cloned().unwrap_or_default());
            next = page["next"].as_str().map(String::from);
        }
        Ok(out)
    }

    /// Solo las tuyas o colaborativas: de las demás la API ya no da canciones.
    pub fn playlists(&self) -> Result<Vec<Item>, String> {
        let me = self.tok.lock().ok().and_then(|t| t["user_id"].as_str().map(String::from)).unwrap_or_default();
        Ok(self
            .paged(&format!("{API}/me/playlists?limit=50"), None)?
            .iter()
            .filter(|p| p["owner"]["id"].as_str() == Some(&me) || p["collaborative"] == true)
            .filter_map(|p| {
                Some(Item {
                    kind: Kind::Playlist,
                    title: p["name"].as_str()?.to_string(),
                    artist: String::new(),
                    album: String::new(),
                    duration: String::new(),
                    video_id: None,
                    browse_id: Some(format!("sp:playlist:{}", p["id"].as_str()?)),
                    thumb: String::new(),
                })
            })
            .collect())
    }

    pub fn liked(&self) -> Result<Vec<Item>, String> {
        Ok(self.paged(&format!("{API}/me/tracks?limit=50"), None)?.iter().filter_map(|x| track(&x["track"], None)).collect())
    }

    pub fn playlist(&self, id: &str) -> Result<Vec<Item>, String> {
        // desde 2026 «items» con «item» dentro (antes «tracks» / «track»)
        let rows = self.paged(&format!("{API}/playlists/{id}/items?limit=50"), None)?;
        Ok(rows.iter().filter_map(|x| track(if x["item"].is_object() { &x["item"] } else { &x["track"] }, None)).collect())
    }

    pub fn albums(&self) -> Result<Vec<Item>, String> {
        Ok(self
            .paged(&format!("{API}/me/albums?limit=50"), None)?
            .iter()
            .filter_map(|x| {
                let a = &x["album"];
                Some(Item {
                    kind: Kind::Album,
                    title: a["name"].as_str()?.to_string(),
                    artist: artists(a),
                    album: String::new(),
                    duration: String::new(),
                    video_id: None,
                    browse_id: Some(format!("sp:album:{}", a["id"].as_str()?)),
                    thumb: image(a),
                })
            })
            .collect())
    }

    pub fn album(&self, id: &str) -> Result<Vec<Item>, String> {
        let a = self.get(&format!("{API}/albums/{id}"))?;
        let mut rows = a["tracks"]["items"].as_array().cloned().unwrap_or_default();
        if let Some(next) = a["tracks"]["next"].as_str() {
            rows.extend(self.paged(next, None)?);
        }
        Ok(rows.iter().filter_map(|t| track(t, Some(&a))).collect())
    }
}

fn artists(v: &Value) -> String {
    v["artists"].as_array().into_iter().flatten().filter_map(|a| a["name"].as_str()).collect::<Vec<_>>().join(", ")
}

fn image(album: &Value) -> String {
    album["images"][0]["url"].as_str().unwrap_or("").to_string()
}

/// Una canción de Spotify, aún sin su video_id de YouTube Music.
fn track(t: &Value, album: Option<&Value>) -> Option<Item> {
    if t["type"].as_str().is_some_and(|k| k != "track") || t["is_local"] == true {
        return None; // episodios de podcast, archivos locales
    }
    let al = album.unwrap_or(&t["album"]);
    let ms = t["duration_ms"].as_u64().unwrap_or(0) / 1000;
    Some(Item {
        kind: Kind::Song,
        title: t["name"].as_str()?.to_string(),
        artist: artists(t),
        album: al["name"].as_str().unwrap_or("").to_string(),
        duration: if ms > 0 { format!("{}:{:02}", ms / 60, ms % 60) } else { String::new() },
        video_id: None,
        browse_id: Some(format!("sp:track:{}", t["id"].as_str()?)),
        thumb: image(al),
    })
}

fn secs(d: &str) -> Option<i64> {
    d.split(':').try_fold(0i64, |a, p| p.parse::<i64>().ok().map(|n| a * 60 + n))
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// «Bohemian Rhapsody - Remastered 2011» y «Despacito (Remix) (feat. X)»:
/// el título de verdad, y lo de los paréntesis o tras « - » aparte.
fn split_title(t: &str) -> (String, Vec<String>) {
    let (mut core, mut extra, mut depth) = (String::new(), String::new(), 0);
    for c in t.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth > 0 => extra.push(c),
            _ => core.push(c),
        }
        if depth > 0 && matches!(c, '(' | '[') {
            extra.push(' ');
        }
    }
    if let Some((a, b)) = core.clone().split_once(" - ") {
        core = a.to_string();
        extra.push(' ');
        extra.push_str(b);
    }
    // lo que no cambia la canción: remasterizada, con quién, el año…
    const NOISE: &[&str] = &["remaster", "remastered", "feat", "ft", "with", "version", "edit", "radio", "single", "mono", "stereo", "original", "mix", "from", "the", "of", "and", "y", "con"];
    let tags = extra
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 1 && !w.chars().all(|c| c.is_ascii_digit()) && !NOISE.contains(&w.as_str()))
        .collect();
    (norm(&core), tags)
}

/// Cuánto se parece una de YouTube Music a la de Spotify (None: no es esa).
fn score(sp: &Item, yt: &Item) -> Option<i32> {
    let (core, tags) = split_title(&sp.title);
    let (ycore, ytags) = split_title(&yt.title);
    // mismo título de verdad y algún artista en común (o la misma tal cual)
    let same = sp.same_song(yt) || (core == ycore && Item { title: String::new(), ..sp.clone() }.same_song(&Item { title: String::new(), ..yt.clone() }));
    if !same || core.is_empty() {
        return None;
    }
    // los artistas de Spotify que YouTube pone entre paréntesis («(Peso Pluma)») no cuentan como extra
    let artists = norm(&sp.artist);
    let mut sc = 0;
    sc += tags.iter().filter(|t| ytags.contains(t)).count() as i32 * 3; // Remix con Remix, Live con Live
    sc -= tags.iter().filter(|t| !ytags.contains(t)).count() as i32 * 3;
    sc -= ytags.iter().filter(|t| !tags.contains(t) && !artists.contains(&norm(t))).count() as i32 * 2; // Cover, Live de más
    sc += (yt.kind == Kind::Song) as i32;
    sc += match (secs(&sp.duration), secs(&yt.duration)) {
        (Some(a), Some(b)) if (a - b).abs() <= 5 => 3,
        (Some(a), Some(b)) if (a - b).abs() <= 12 => 1,
        (Some(_), Some(_)) => -4,
        _ => 0,
    };
    Some(sc)
}

/// La de YouTube Music que mejor encaja (ver `score`), si alguna encaja.
pub fn best_match(api: &Api, it: &Item) -> Option<String> {
    let first = it.artist.split(',').next().unwrap_or("").trim();
    // sin « - Remastered…» se encuentra mejor
    let q = it.title.split(" - ").next().unwrap_or(&it.title);
    let found = api.search(&format!("{first} {q}")).ok()?;
    found
        .iter()
        .filter(|x| x.playable())
        .filter_map(|x| Some((score(it, x)?, x)))
        .filter(|(sc, _)| *sc >= -1)
        .max_by_key(|(sc, _)| *sc)
        .and_then(|(_, x)| x.video_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_bits() {
        // RFC 7636, apéndice B
        let v = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(b64url(&Sha256::digest(v.as_bytes())), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(enc("a b/c"), "a%20b%2Fc");
        assert_eq!(random(64).len(), 64);
    }

    #[test]
    fn titles_and_scores() {
        assert_eq!(split_title("Bohemian Rhapsody - Remastered 2011"), ("bohemianrhapsody".into(), vec![]));
        assert_eq!(split_title("Despacito (Remix) (feat. Justin Bieber)").1, vec!["remix", "justin", "bieber"]);
        let it = |t: &str, a: &str, d: &str, k: Kind| Item { kind: k, title: t.into(), artist: a.into(), album: String::new(), duration: d.into(), video_id: Some("v".into()), browse_id: None, thumb: String::new() };
        let sp = it("Despacito - Remix", "Luis Fonsi, Daddy Yankee, Justin Bieber", "3:48", Kind::Song);
        let remix = score(&sp, &it("Despacito (Remix) (feat. Justin Bieber)", "Luis Fonsi, Daddy Yankee", "3:51", Kind::Song)).unwrap();
        let orig = score(&sp, &it("Despacito", "Luis Fonsi, Daddy Yankee", "3:49", Kind::Song)).unwrap();
        assert!(remix > orig, "{remix} vs {orig}");
        let sp = it("Ella Baila Sola", "Eslabon Armado, Peso Pluma", "2:45", Kind::Song);
        assert!(score(&sp, &it("Ella Baila Sola (Peso Pluma)", "Eslabon Armado, Peso Pluma", "2:46", Kind::Song)).unwrap() > 0);
        assert!(score(&sp, &it("Ella Baila Sola (Cover)", "Ángel Ortiz y su Mariachi", "2:36", Kind::Song)).is_none());
        assert!(score(&sp, &it("Amor", "Emmanuel Cortes", "3:20", Kind::Song)).is_none());
    }

    #[test]
    fn callback_server() {
        use std::net::TcpStream;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // cada una a su hora: si la buena llegara antes, nadie atendería la otra
        let req = |path: &'static str, ms: u64| {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(c, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut out = String::new();
                let _ = c.read_to_string(&mut out);
                out
            })
        };
        // el favicon se ignora; vale el que trae el state bueno
        let (f, ok) = (req("/favicon.ico", 100), req("/callback?code=AQB123&state=xyz", 400));
        assert_eq!(wait_code(&l, "xyz").unwrap(), "AQB123");
        assert!(f.join().unwrap().starts_with("HTTP/1.1 404"));
        assert!(ok.join().unwrap().contains("connected"));
        // state cambiado (otra web intentando colarse): rechazado
        let bad = req("/callback?code=EVIL&state=other", 100);
        assert!(wait_code(&l, "xyz").is_err());
        bad.join().unwrap();
    }

    #[test]
    fn tracks_parse() {
        let t = json!({"type": "track", "id": "abc", "name": "Me Matas", "duration_ms": 204000,
            "artists": [{"name": "Eslabon Armado"}], "album": {"name": "Corta Venas", "images": [{"url": "https://i.scdn.co/x"}]}});
        let it = track(&t, None).unwrap();
        assert_eq!((it.title.as_str(), it.artist.as_str(), it.album.as_str(), it.duration.as_str()), ("Me Matas", "Eslabon Armado", "Corta Venas", "3:24"));
        assert_eq!(it.browse_id.as_deref(), Some("sp:track:abc"));
        assert!(track(&json!({"type": "episode", "id": "e", "name": "pod"}), None).is_none());
    }
}

#[cfg(test)]
mod live {
    use super::*;
    #[test]
    #[ignore]
    fn matching() {
        let api = Api::new(Path::new("/nonexistent"));
        let cases = [
            ("Me Matas", "Eslabon Armado", "3:24"),
            ("PIÉNSALO", "Junior H", "4:01"),
            ("Ella Baila Sola", "Eslabon Armado, Peso Pluma", "2:45"),
            ("Bohemian Rhapsody - Remastered 2011", "Queen", "5:55"),
            ("Here Comes The Sun - Remastered 2009", "The Beatles", "3:05"),
            ("Mr. Brightside", "The Killers", "3:42"),
            ("Tití Me Preguntó", "Bad Bunny", "4:03"),
            ("LA INOCENTE", "Mora, Feid", "3:21"),
            ("Blinding Lights", "The Weeknd", "3:20"),
            ("Shape of You", "Ed Sheeran", "3:53"),
            ("Despacito - Remix", "Luis Fonsi, Daddy Yankee, Justin Bieber", "3:48"),
            ("Something In The Way - Remastered 2021", "Nirvana", "3:52"),
        ];
        let mut ok = 0;
        for (t, a, d) in cases {
            let it = Item { kind: Kind::Song, title: t.into(), artist: a.into(), album: String::new(), duration: d.into(), video_id: None, browse_id: Some("sp:track:x".into()), thumb: String::new() };
            let m = best_match(&api, &it);
            ok += m.is_some() as usize;
            println!("{:45} {:30} -> {:?}", t, a, m);
        }
        println!("matched {ok}/{}", cases.len());
    }
}
