//! Cliente mínimo de la API interna de YouTube Music (InnerTube, cliente WEB_REMIX).
//!
//! En vez de seguir rutas exactas del JSON (que YouTube cambia seguido), se recorre
//! la respuesta entera buscando los renderers que pintan filas y tarjetas.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ORIGIN: &str = "https://music.youtube.com";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
const TYPE_WORDS: &[&str] = &[
    "Song", "Video", "Album", "Single", "EP", "Playlist", "Artist", "Episode", "Podcast", "Profile",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Song,
    Video,
    Album,
    Artist,
    Playlist,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: Kind,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    pub video_id: Option<String>,
    pub browse_id: Option<String>,
}

impl Item {
    pub fn playable(&self) -> bool {
        matches!(self.kind, Kind::Song | Kind::Video) && self.video_id.is_some()
    }
}

struct Auth {
    cookie: String,
    authuser: String,
    sapisid: String,
}

#[derive(Clone)]
pub struct Api {
    agent: ureq::Agent,
    auth: Option<Arc<Auth>>,
}

impl Api {
    pub fn new(auth_file: &Path) -> Api {
        let auth = std::fs::read_to_string(auth_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| {
                let cookie = v["cookie"].as_str()?.to_string();
                let sapisid = sapisid_of(&cookie)?;
                let authuser = v["authuser"].as_str().unwrap_or("0").to_string();
                Some(Arc::new(Auth { cookie, authuser, sapisid }))
            });
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .user_agent(UA)
            .build();
        Api { agent, auth }
    }

    pub fn authed(&self) -> bool {
        self.auth.is_some()
    }

    fn post(&self, endpoint: &str, mut body: Value) -> Result<Value, String> {
        body["context"] = json!({
            "client": {"clientName": "WEB_REMIX", "clientVersion": client_version(), "hl": "en"},
            "user": {},
        });
        let mut req = self
            .agent
            .post(&format!("{ORIGIN}/youtubei/v1/{endpoint}?alt=json&prettyPrint=false"))
            .set("content-type", "application/json")
            .set("origin", ORIGIN)
            .set("x-origin", ORIGIN);
        if let Some(a) = &self.auth {
            req = req
                .set("cookie", &a.cookie)
                .set("x-goog-authuser", &a.authuser)
                .set("authorization", &sapisid_hash(&a.sapisid));
        }
        let resp = req.send_string(&body.to_string()).map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                let msg = r
                    .into_string()
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .and_then(|v| v["error"]["message"].as_str().map(String::from))
                    .unwrap_or_default();
                format!("HTTP {code} {msg}")
            }
            e => e.to_string(),
        })?;
        let text = resp.into_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    /// browse + continuaciones (playlists largas, me gusta…), hasta `pages` extra.
    fn browse(&self, browse_id: &str, pages: usize) -> Result<(Value, Vec<Item>), String> {
        let first = self.post("browse", json!({ "browseId": browse_id }))?;
        let mut items = parse_items(&first);
        let mut tok = continuation(&first);
        for _ in 0..pages {
            let Some(t) = tok else { break };
            let v = self.post("browse", json!({ "continuation": t }))?;
            let more = parse_items(&v);
            if more.is_empty() {
                break;
            }
            items.extend(more);
            tok = continuation(&v);
        }
        Ok((first, items))
    }

    pub fn home(&self) -> Result<Vec<Item>, String> {
        Ok(self.browse("FEmusic_home", 0)?.1)
    }

    pub fn liked(&self) -> Result<Vec<Item>, String> {
        Ok(self.browse("VLLM", 40)?.1)
    }

    pub fn history(&self) -> Result<Vec<Item>, String> {
        Ok(self.browse("FEmusic_history", 0)?.1)
    }

    pub fn library_playlists(&self) -> Result<Vec<Item>, String> {
        let (_, items) = self.browse("FEmusic_liked_playlists", 10)?;
        Ok(items.into_iter().filter(|i| i.kind == Kind::Playlist).collect())
    }

    pub fn library_albums(&self) -> Result<Vec<Item>, String> {
        Ok(self.browse("FEmusic_liked_albums", 10)?.1)
    }

    pub fn library_artists(&self) -> Result<Vec<Item>, String> {
        Ok(self.browse("FEmusic_library_corpus_track_artists", 10)?.1)
    }

    pub fn playlist(&self, id: &str) -> Result<Vec<Item>, String> {
        let (_, items) = self.browse(id, 40)?;
        Ok(items.into_iter().filter(|i| i.playable()).collect())
    }

    pub fn album(&self, id: &str) -> Result<Vec<Item>, String> {
        let (v, mut items) = self.browse(id, 0)?;
        items.retain(|i| i.playable());
        let (title, artist) = match find_key(&v, "musicResponsiveHeaderRenderer") {
            Some(h) => (runs_text(&h["title"]), runs_text(&h["straplineTextOne"])),
            None => (String::new(), String::new()),
        };
        for it in &mut items {
            if it.artist.is_empty() {
                it.artist = artist.clone();
            }
            if it.album.is_empty() {
                it.album = title.clone();
            }
        }
        Ok(items)
    }

    pub fn artist(&self, id: &str) -> Result<Vec<Item>, String> {
        let id = id.strip_prefix("MPLA").unwrap_or(id);
        let (v, mut items) = self.browse(id, 0)?;
        let name = ["musicImmersiveHeaderRenderer", "musicVisualHeaderRenderer"]
            .iter()
            .find_map(|k| find_key(&v, k))
            .map(|h| runs_text(&h["title"]))
            .unwrap_or_default();
        for it in &mut items {
            if it.artist.is_empty() && it.kind != Kind::Artist {
                it.artist = name.clone();
            }
        }
        Ok(items)
    }

    /// La búsqueda sin filtro viene con artistas mal etiquetados; por tipo sale
    /// bien, así que se piden cuatro en paralelo.
    pub fn search(&self, q: &str) -> Result<Vec<Item>, String> {
        let filters: [(&str, usize); 4] = [
            ("EgWKAQIgAWoMEA4QChADEAQQCRAF", 2),           // artists
            ("EgWKAQIIAWoMEA4QChADEAQQCRAF", 25),          // songs
            ("EgWKAQIYAWoMEA4QChADEAQQCRAF", 6),           // albums
            ("Eg-KAQwIABAAGAAgACgBMABqChAEEAMQCRAFEAo%3D", 6), // playlists
        ];
        let results: Vec<Result<Vec<Item>, String>> = std::thread::scope(|s| {
            let hs: Vec<_> = filters
                .iter()
                .map(|(p, n)| {
                    s.spawn(move || {
                        let v = self.post("search", json!({ "query": q, "params": p }))?;
                        let mut items = parse_items(&v);
                        items.retain(|i| i.playable() || i.browse_id.is_some());
                        items.truncate(*n);
                        Ok(items)
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("panic".into()))).collect()
        });
        let mut out = Vec::new();
        let mut err = None;
        for r in results {
            match r {
                Ok(v) => out.extend(v),
                Err(e) => err = Some(e),
            }
        }
        match (out.is_empty(), err) {
            (true, Some(e)) => Err(e),
            _ => Ok(out),
        }
    }

    /// Radio a partir de una canción (lo que YT Music pone al acabar la cola).
    pub fn radio(&self, video_id: &str) -> Result<Vec<Item>, String> {
        let v = self.post(
            "next",
            json!({
                "videoId": video_id,
                "playlistId": format!("RDAMVM{video_id}"),
                "params": "wAEB",
                "isAudioOnly": true,
                "enablePersistentPlaylistPanel": true,
                "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
            }),
        )?;
        let mut seen = std::collections::HashSet::new();
        Ok(parse_items(&v)
            .into_iter()
            .filter(|i| i.playable() && seen.insert(i.video_id.clone()))
            .collect())
    }

    pub fn like(&self, video_id: &str) -> Result<(), String> {
        self.post("like/like", json!({ "target": { "videoId": video_id } })).map(|_| ())
    }
}

// ── login ────────────────────────────────────────────────────────────────────

/// Saca cookie y x-goog-authuser de los headers pegados desde el navegador.
/// Acepta "nombre: valor" por línea, o nombre y valor en líneas alternas.
pub fn parse_headers(raw: &str) -> Option<Value> {
    let lines: Vec<&str> = raw.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut cookie = None;
    let mut authuser = None;
    for (i, l) in lines.iter().enumerate() {
        let (k, v) = match l.split_once(": ") {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (l.trim_end_matches(':'), lines.get(i + 1).copied().unwrap_or("")),
        };
        match k.to_ascii_lowercase().as_str() {
            "cookie" if cookie.is_none() => cookie = Some(v.to_string()),
            "x-goog-authuser" if authuser.is_none() => authuser = Some(v.to_string()),
            _ => {}
        }
    }
    let cookie = cookie?;
    sapisid_of(&cookie)?;
    Some(json!({ "cookie": cookie, "authuser": authuser.unwrap_or_else(|| "0".into()) }))
}

fn sapisid_of(cookie: &str) -> Option<String> {
    let mut fallback = None;
    for part in cookie.split(';') {
        let (k, v) = part.trim().split_once('=')?;
        match k {
            "__Secure-3PAPISID" => return Some(v.to_string()),
            "SAPISID" => fallback = Some(v.to_string()),
            _ => {}
        }
    }
    fallback
}

fn sapisid_hash(sapisid: &str) -> String {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let hex = sha1_smol::Sha1::from(format!("{ts} {sapisid} {ORIGIN}")).digest().to_string();
    format!("SAPISIDHASH {ts}_{hex}")
}

/// "1.AAAAMMDD.01.00" con la fecha de hoy, como manda la web.
fn client_version() -> String {
    let days = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64 / 86400;
    // civil_from_days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("1.{y:04}{m:02}{d:02}.01.00")
}

// ── parseo ───────────────────────────────────────────────────────────────────

fn parse_items(v: &Value) -> Vec<Item> {
    let mut out = Vec::new();
    walk(v, &mut out);
    out
}

fn walk(v: &Value, out: &mut Vec<Item>) {
    match v {
        Value::Object(m) => {
            for (k, val) in m {
                let it = match k.as_str() {
                    "musicResponsiveListItemRenderer" => parse_row(val),
                    "musicTwoRowItemRenderer" => parse_card(val),
                    "playlistPanelVideoRenderer" => parse_panel(val),
                    _ => {
                        walk(val, out);
                        continue;
                    }
                };
                out.extend(it);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
        _ => {}
    }
}

fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|x| find_key(x, key))),
        Value::Array(a) => a.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

/// Último token de continuación (los nuevos van en continuationItemRenderer).
fn continuation(v: &Value) -> Option<String> {
    fn go(v: &Value, last: &mut Option<String>) {
        match v {
            Value::Object(m) => {
                if let Some(t) = m.get("continuationItemRenderer").and_then(|c| {
                    c["continuationEndpoint"]["continuationCommand"]["token"].as_str()
                }) {
                    *last = Some(t.to_string());
                }
                m.values().for_each(|x| go(x, last));
            }
            Value::Array(a) => a.iter().for_each(|x| go(x, last)),
            _ => {}
        }
    }
    let mut last = None;
    go(v, &mut last);
    last
}

fn runs(v: &Value) -> &[Value] {
    v["runs"].as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn runs_text(v: &Value) -> String {
    runs(v).iter().filter_map(|r| r["text"].as_str()).collect()
}

fn page_type(nav: &Value) -> Option<(&str, &str)> {
    let b = &nav["browseEndpoint"];
    let id = b["browseId"].as_str()?;
    let pt = b["browseEndpointContextSupportedConfigs"]["browseEndpointContextMusicConfig"]["pageType"]
        .as_str()
        .unwrap_or("");
    Some((id, pt))
}

fn kind_of_page(pt: &str) -> Option<Kind> {
    match pt {
        "MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK" => Some(Kind::Album),
        "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" | "MUSIC_PAGE_TYPE_LIBRARY_ARTIST" => {
            Some(Kind::Artist)
        }
        "MUSIC_PAGE_TYPE_PLAYLIST" => Some(Kind::Playlist),
        _ => None,
    }
}

fn is_duration(s: &str) -> bool {
    s.contains(':') && s.len() <= 8 && s.chars().all(|c| c.is_ascii_digit() || c == ':')
}

/// Texto suelto que puede ser un artista (no separador, tipo, año, "1.2B plays"…).
fn plain_name(s: &str) -> bool {
    !s.is_empty()
        && !matches!(s, "•" | "&" | "," | "·")
        && !TYPE_WORDS.contains(&s)
        && !s.starts_with(|c: char| c.is_ascii_digit())
}

/// Clasifica los runs de subtítulo: artistas con link, álbum con link, duración.
#[derive(Default)]
struct Meta {
    artists: Vec<String>,
    plain: Vec<String>,
    album: String,
    duration: String,
}

impl Meta {
    fn take(&mut self, runs: &[Value], allow_plain: bool) {
        for r in runs {
            let text = r["text"].as_str().unwrap_or("").trim();
            match page_type(&r["navigationEndpoint"]).map(|(_, pt)| kind_of_page(pt)) {
                Some(Some(Kind::Artist)) => self.artists.push(text.to_string()),
                Some(Some(Kind::Album)) => self.album = text.to_string(),
                _ if is_duration(text) => self.duration = text.to_string(),
                None if allow_plain && plain_name(text) => self.plain.push(text.to_string()),
                _ => {}
            }
        }
    }

    fn artist(&self) -> String {
        if self.artists.is_empty() { self.plain.join(", ") } else { self.artists.join(", ") }
    }
}

fn finish(kind: Kind, title: String, meta: Meta, video_id: Option<String>, browse_id: Option<String>) -> Item {
    let browse_id = match (kind, browse_id) {
        (Kind::Playlist, Some(b)) if !b.starts_with("VL") => Some(format!("VL{b}")),
        (_, b) => b,
    };
    Item {
        kind,
        artist: if kind == Kind::Artist { String::new() } else { meta.artist() },
        album: meta.album,
        duration: meta.duration,
        title,
        video_id,
        browse_id,
    }
}

fn video_kind(watch: &Value) -> Kind {
    match watch["watchEndpointMusicSupportedConfigs"]["watchEndpointMusicConfig"]["musicVideoType"].as_str() {
        Some("MUSIC_VIDEO_TYPE_ATV") | None => Kind::Song,
        _ => Kind::Video,
    }
}

fn parse_row(r: &Value) -> Option<Item> {
    let cols: Vec<&[Value]> = r["flexColumns"]
        .as_array()?
        .iter()
        .map(|c| runs(&c["musicResponsiveListItemFlexColumnRenderer"]["text"]))
        .collect();
    let first = cols.first()?;
    let title: String = first.iter().filter_map(|x| x["text"].as_str()).collect();
    if title.is_empty() {
        return None;
    }
    let mut meta = Meta::default();
    for (i, c) in cols.iter().enumerate().skip(1) {
        meta.take(c, i == 1);
    }
    if let Some(d) = r["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"]["text"]["runs"][0]["text"].as_str() {
        meta.duration = d.to_string();
    }

    // fila que abre algo (álbum, artista, playlist)
    if let Some((id, pt)) = page_type(&r["navigationEndpoint"]) {
        return Some(finish(kind_of_page(pt)?, title, meta, None, Some(id.to_string())));
    }
    let watch = &first[0]["navigationEndpoint"]["watchEndpoint"];
    let vid = r["playlistItemData"]["videoId"]
        .as_str()
        .or_else(|| watch["videoId"].as_str())
        .or_else(|| find_key(&r["overlay"], "watchEndpoint").and_then(|w| w["videoId"].as_str()))?;
    Some(finish(video_kind(watch), title, meta, Some(vid.to_string()), None))
}

fn parse_card(r: &Value) -> Option<Item> {
    let title = runs_text(&r["title"]);
    let mut meta = Meta::default();
    meta.take(runs(&r["subtitle"]), true);
    let nav = &r["navigationEndpoint"];
    if let Some((id, pt)) = page_type(nav) {
        return Some(finish(kind_of_page(pt)?, title, meta, None, Some(id.to_string())));
    }
    if let Some(v) = nav["watchEndpoint"]["videoId"].as_str() {
        return Some(finish(video_kind(&nav["watchEndpoint"]), title, meta, Some(v.to_string()), None));
    }
    let pl = nav["watchPlaylistEndpoint"]["playlistId"].as_str()?;
    Some(finish(Kind::Playlist, title, meta, None, Some(pl.to_string())))
}

fn parse_panel(r: &Value) -> Option<Item> {
    let vid = r["videoId"].as_str()?;
    let mut meta = Meta::default();
    meta.take(runs(&r["longBylineText"]), true);
    meta.duration = runs_text(&r["lengthText"]);
    let watch = &r["navigationEndpoint"]["watchEndpoint"];
    Some(finish(video_kind(watch), runs_text(&r["title"]), meta, Some(vid.to_string()), None))
}

#[cfg(test)]
mod tests {
    // tocan la red de verdad: cargo test -- --ignored --nocapture
    use super::*;

    fn api() -> Api {
        Api::new(Path::new("/nonexistent"))
    }

    fn show(label: &str, items: &[Item]) {
        println!("── {label}: {}", items.len());
        for i in items.iter().take(6) {
            println!("  {:?} | {} | {} | {} | {} | {:?} {:?}", i.kind, i.title, i.artist, i.album, i.duration, i.video_id, i.browse_id);
        }
    }

    #[test]
    #[ignore]
    fn live() {
        let a = api();
        let s = a.search("daft punk").unwrap();
        show("search", &s);
        assert!(s.iter().any(|i| i.kind == Kind::Artist));
        assert!(s.iter().filter(|i| i.playable()).all(|i| !i.artist.is_empty()));
        let album = s.iter().find(|i| i.kind == Kind::Album).unwrap();
        show("album", &a.album(album.browse_id.as_ref().unwrap()).unwrap());
        let artist = s.iter().find(|i| i.kind == Kind::Artist).unwrap();
        show("artist", &a.artist(artist.browse_id.as_ref().unwrap()).unwrap());
        let pl = s.iter().find(|i| i.kind == Kind::Playlist).unwrap();
        show("playlist", &a.playlist(pl.browse_id.as_ref().unwrap()).unwrap());
        let song = s.iter().find(|i| i.playable()).unwrap();
        show("radio", &a.radio(song.video_id.as_ref().unwrap()).unwrap());
        show("home", &a.home().unwrap());
    }

    #[test]
    fn headers() {
        let raw = "Host: music.youtube.com\nX-Goog-AuthUser: 1\nCookie: a=b; __Secure-3PAPISID=xyz/abc; c=d\n";
        let v = parse_headers(raw).unwrap();
        assert_eq!(v["authuser"], "1");
        assert_eq!(sapisid_of(v["cookie"].as_str().unwrap()).unwrap(), "xyz/abc");
        assert!(client_version().starts_with("1.20"));
    }
}
