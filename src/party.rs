//! Modo fiesta: rola como rocola de verdad. Una página en tu wifi donde los
//! demás ven qué suena y añaden canciones a la cola desde el móvil.
//!
//! Solo mientras está encendido, y solo con el código del enlace (va en el QR):
//! sin él, la página no dice nada. Cada móvil puede añadir unas pocas por
//! minuto, y solo canciones (un videoId); los títulos se pintan como texto.

use crate::api::{Api, Item, Kind};
use crate::Ev;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Canciones por minuto que puede añadir cada móvil.
const PER_MINUTE: usize = 5;

/// Lo que se le pide a rola desde la página.
pub enum Req {
    State(Sender<Value>),
    Add { item: Item, by: String },
}

pub struct Party {
    pub url: String,
    stop: Arc<AtomicBool>,
}

impl Drop for Party {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// La IP de este equipo en la red de casa (sin mandar nada: es solo para
/// saber por qué interfaz saldría).
fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:80").ok()?;
    Some(s.local_addr().ok()?.ip())
}

fn code() -> String {
    let mut b = [0u8; 6];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b.iter().map(|x| (b'a' + x % 26) as char).collect()
}

pub fn start(api: Api, tx: Sender<Ev>) -> Result<Party, String> {
    let ip = lan_ip().ok_or("sin red / no network")?;
    let listener = (8899..8910).find_map(|p| TcpListener::bind(("0.0.0.0", p)).ok()).ok_or("sin puerto libre / no free port")?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let key = code();
    let url = format!("http://{ip}:{port}/?k={key}");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let adds: Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>> = Arc::default();
    std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, addr)) => {
                    let (api, tx, key, adds) = (api.clone(), tx.clone(), key.clone(), adds.clone());
                    std::thread::spawn(move || {
                        let _ = serve(s, addr.ip(), &api, &tx, &key, &adds);
                    });
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    Ok(Party { url, stop })
}

/// `a=b&c=d` (con %xx y +) → mapa.
fn query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|kv| kv.split_once('=').or(Some((kv, ""))))
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| (k.to_string(), unescape(v)))
        .collect()
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(x) => {
                        out.push(x);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn reply(s: &mut TcpStream, status: &str, kind: &str, body: &str) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn serve(mut s: TcpStream, ip: IpAddr, api: &Api, tx: &Sender<Ev>, key: &str, adds: &Mutex<HashMap<IpAddr, Vec<Instant>>>) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    let q = query(q);
    // sin el código, nada
    if q.get("k").map(String::as_str) != Some(key) {
        return reply(&mut s, "403 Forbidden", "text/plain; charset=utf-8", "rola: pide el enlace a quien pone la música / ask the host for the link");
    }
    let json = |s: &mut TcpStream, v: Value| reply(s, "200 OK", "application/json", &v.to_string());
    match (method, path) {
        ("GET", "/") => reply(&mut s, "200 OK", "text/html; charset=utf-8", PAGE),
        ("GET", "/api/state") => {
            let (rtx, rrx) = mpsc::channel();
            let _ = tx.send(Ev::Party(Req::State(rtx)));
            let v = rrx.recv_timeout(Duration::from_secs(3)).unwrap_or(Value::Null);
            json(&mut s, v)
        }
        ("GET", "/api/search") => {
            let found = api.search(q.get("q").map(String::as_str).unwrap_or("")).unwrap_or_default();
            let songs: Vec<Value> = found
                .iter()
                .filter(|x| x.kind == Kind::Song && x.video_id.is_some())
                .take(12)
                .map(|x| json!({ "v": x.video_id, "title": x.title, "artist": x.artist, "album": x.album, "duration": x.duration, "thumb": x.cover_url() }))
                .collect();
            json(&mut s, json!(songs))
        }
        ("POST", "/api/add") => {
            let vid = q.get("v").cloned().unwrap_or_default();
            if vid.len() != 11 || !vid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return reply(&mut s, "400 Bad Request", "text/plain", "bad id");
            }
            {
                let mut a = adds.lock().unwrap_or_else(|e| e.into_inner());
                let mine = a.entry(ip).or_default();
                mine.retain(|t| t.elapsed() < Duration::from_secs(60));
                if mine.len() >= PER_MINUTE {
                    return reply(&mut s, "429 Too Many Requests", "application/json", &json!({ "ok": false, "wait": true }).to_string());
                }
                mine.push(Instant::now());
            }
            let clip = |k: &str, n: usize| q.get(k).map(|v| v.chars().filter(|c| !c.is_control()).take(n).collect::<String>()).unwrap_or_default();
            let item = Item {
                kind: Kind::Song,
                title: clip("title", 120),
                artist: clip("artist", 120),
                album: clip("album", 120),
                duration: clip("duration", 10),
                video_id: Some(vid),
                browse_id: None,
                thumb: clip("thumb", 300),
            };
            let _ = tx.send(Ev::Party(Req::Add { item, by: clip("by", 20) }));
            json(&mut s, json!({ "ok": true }))
        }
        _ => reply(&mut s, "404 Not Found", "text/plain", "404"),
    }
}

/// El QR de `text` en medios bloques (dos filas de módulos por fila de texto),
/// con su margen blanco: oscuro = ' ' sobre fondo claro.
pub fn qr(text: &str) -> Vec<String> {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else { return Vec::new() };
    let w = code.width();
    let colors = code.to_colors();
    let dark = |x: isize, y: isize| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < w && colors[y as usize * w + x as usize] == qrcode::Color::Dark;
    let m = 2isize; // margen, en módulos
    let mut out = Vec::new();
    let mut y = -m;
    while y < w as isize + m {
        let row: String = (-m..w as isize + m)
            .map(|x| match (dark(x, y), dark(x, y + 1)) {
                (false, false) => '█',
                (false, true) => '▀',
                (true, false) => '▄',
                (true, true) => ' ',
            })
            .collect();
        out.push(row);
        y += 2;
    }
    out
}

const PAGE: &str = r##"<!doctype html>
<html lang="es"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>rola · fiesta</title>
<style>
:root{--bg:#141217;--fg:#eee;--mut:#9a95a0;--pri:#e0a070;--acc:#e07090;--card:#1f1c24}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.4 system-ui,sans-serif;padding:16px;max-width:640px;margin:auto}
h1{font-size:22px;margin:0 0 12px;background:linear-gradient(90deg,var(--pri),var(--acc));-webkit-background-clip:text;background-clip:text;color:transparent}
.now{display:flex;gap:12px;align-items:center;background:var(--card);border-radius:14px;padding:12px}
.now img{width:72px;height:72px;border-radius:10px;object-fit:cover;background:#000}
.t{font-weight:600}.a{color:var(--mut);font-size:14px}
h2{font-size:13px;letter-spacing:.08em;text-transform:uppercase;color:var(--mut);margin:20px 0 8px}
ol{margin:0;padding-left:22px}li{margin:4px 0}
form{display:flex;gap:8px}input{flex:1;min-width:0;padding:12px;border-radius:10px;border:1px solid #333;background:var(--card);color:var(--fg);font-size:16px}
button{padding:10px 14px;border:0;border-radius:10px;background:var(--pri);color:#111;font-weight:600;font-size:15px}
.r{display:flex;gap:10px;align-items:center;padding:8px 0;border-bottom:1px solid #2a2630}
.r img{width:44px;height:44px;border-radius:6px;object-fit:cover}.r div{flex:1;min-width:0}
.r .t,.r .a{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#toast{position:fixed;left:50%;bottom:24px;transform:translateX(-50%);background:var(--acc);color:#111;padding:10px 16px;border-radius:20px;font-weight:600;opacity:0;transition:opacity .3s}
#nick{width:100%;margin-bottom:8px}
</style></head><body>
<h1>rola · fiesta</h1>
<div class="now"><img id="cover" alt=""><div><div class="t" id="title">…</div><div class="a" id="artist"></div></div></div>
<h2 id="hnext">Después</h2><ol id="next"></ol>
<h2 id="hadd">Pon una canción</h2>
<input id="nick" placeholder="Tu nombre" maxlength="20">
<form id="f"><input id="q" placeholder="Buscar en YouTube Music" autocomplete="off"><button>🔎</button></form>
<div id="res"></div><div id="toast"></div>
<script>
const $=id=>document.getElementById(id);
const k=new URLSearchParams(location.search).get('k');
const en=!navigator.language.startsWith('es');
const T=en?{next:'Up next',add:'Add a song',name:'Your name',q:'Search YouTube Music',added:'Added ✓',wait:'Easy there, try in a minute',empty:'Nothing playing'}:{next:'Después',add:'Pon una canción',name:'Tu nombre',q:'Buscar en YouTube Music',added:'Añadida ✓',wait:'Con calma: prueba en un minuto',empty:'No suena nada'};
$('hnext').textContent=T.next;$('hadd').textContent=T.add;$('nick').placeholder=T.name;$('q').placeholder=T.q;
try{$('nick').value=localStorage.getItem('rola-name')||''}catch(_){/* sin almacenamiento */}
$('nick').onchange=()=>{try{localStorage.setItem('rola-name',$('nick').value)}catch(_){/* sin almacenamiento */}};
function el(tag,cls,text){const e=document.createElement(tag);if(cls)e.className=cls;if(text!=null)e.textContent=text;return e}
function toast(m){const t=$('toast');t.textContent=m;t.style.opacity=1;setTimeout(()=>t.style.opacity=0,1800)}
async function state(){
  try{
    const s=await (await fetch('/api/state?k='+k)).json();if(!s)return;
    const r=document.documentElement.style;
    if(s.colors){r.setProperty('--pri',s.colors.primary);r.setProperty('--acc',s.colors.accent)}
    $('title').textContent=s.now?s.now.title:T.empty;$('artist').textContent=s.now?s.now.artist:'';
    if(s.now&&s.now.thumb)$('cover').src=s.now.thumb;
    $('next').replaceChildren(...(s.next||[]).map(x=>{const li=el('li');li.append(el('span','t',x.title),el('span','a',' · '+x.artist));return li}));
  }catch(_){/* rola cerró: se reintenta solo */}
}
$('f').onsubmit=async e=>{
  e.preventDefault();const q=$('q').value.trim();if(!q)return;
  $('res').replaceChildren(el('div','a','…'));
  const list=await (await fetch('/api/search?k='+k+'&q='+encodeURIComponent(q))).json();
  $('res').replaceChildren(...list.map(x=>{
    const row=el('div','r');const img=el('img');img.src=x.thumb||'';const d=el('div');
    d.append(el('div','t',x.title),el('div','a',x.artist+(x.duration?' · '+x.duration:'')));
    const b=el('button',null,'+');
    b.onclick=async()=>{
      const p=new URLSearchParams({k,v:x.v,title:x.title,artist:x.artist,album:x.album||'',duration:x.duration||'',thumb:x.thumb||'',by:$('nick').value});
      const r=await fetch('/api/add?'+p,{method:'POST'});
      toast(r.ok?T.added:T.wait);if(r.ok){b.disabled=true;state()}
    };
    row.append(img,d,b);return row}));
};
state();setInterval(state,3000);
</script></body></html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_strings() {
        let q = query("k=abc&q=bad+bunny%20titi&by=Ana%C3%AD&x");
        assert_eq!(q["k"], "abc");
        assert_eq!(q["q"], "bad bunny titi");
        assert_eq!(q["by"], "Anaí");
        assert_eq!(q["x"], "");
        assert_eq!(unescape("100%"), "100%");
        assert_eq!(unescape("%zz"), "%zz");
    }

    #[test]
    fn qr_is_square_ish() {
        let rows = qr("http://192.168.1.5:8899/?k=abcdef");
        assert!(!rows.is_empty());
        let w = rows[0].chars().count();
        assert!(rows.iter().all(|r| r.chars().count() == w));
        assert!((rows.len() * 2).abs_diff(w) <= 2);
    }
}
