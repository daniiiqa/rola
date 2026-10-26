//! rola en la barra (waybar): mientras está abierta, rola deja lo que suena
//! en un archivo del runtime, y `rola --waybar` lo traduce a JSON de módulo
//! custom: la canción (o la línea de la letra que suena) en el color de la
//! portada, y en el tooltip el disco, el tiempo, el temporizador y la fiesta.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub fn path() -> PathBuf {
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("rola-status.json")
}

/// Lo escribe rola (por un temporal: quien lee nunca ve uno a medias).
pub fn write(v: &Value) {
    let p = path();
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, v.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

pub fn clear() {
    let _ = std::fs::remove_file(path());
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

fn mmss(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// El JSON del módulo para ese estado (vacío si rola no está o no suena nada).
pub fn module(st: &Value, lyrics: bool, max: usize) -> Value {
    let Some(title) = st["title"].as_str() else {
        return json!({ "text": "", "class": "stopped" });
    };
    let artist = st["artist"].as_str().unwrap_or("");
    let playing = st["playing"].as_bool().unwrap_or(false);
    let line = st["lyric"].as_str().filter(|l| lyrics && !l.is_empty());
    let shown = match line {
        Some(l) => format!("♪ {l}"),
        None if artist.is_empty() => title.to_string(),
        None => format!("{title} · {artist}"),
    };
    let mut text = escape(&clip(&shown, max));
    if let Some(c) = st["color"].as_str() {
        text = format!("<span color=\"{c}\">{text}</span>");
    }
    let mut tip = vec![format!("<b>{}</b>", escape(title)), escape(artist)];
    if let Some(a) = st["album"].as_str().filter(|a| !a.is_empty()) {
        tip.push(format!("<i>{}</i>", escape(a)));
    }
    if let (Some(p), Some(d)) = (st["pos"].as_f64(), st["dur"].as_f64()) {
        tip.push(format!("{} / {}", mmss(p), mmss(d)));
    }
    if let Some(s) = st["sleep"].as_str() {
        tip.push(format!("󰒲 {}", escape(s)));
    }
    if st["party"].as_bool() == Some(true) {
        tip.push("󰡉 fiesta / party".into());
    }
    json!({
        "text": text,
        "tooltip": tip.join("\n"),
        "class": if playing { "playing" } else { "paused" },
        "alt": if playing { "playing" } else { "paused" },
    })
}

/// `rola --waybar [--lyrics] [--max N]`: una línea JSON cada vez que cambia.
pub fn waybar(args: &[String]) {
    use std::io::Write;
    let lyrics = args.iter().any(|a| a == "--lyrics");
    let max = args.iter().position(|a| a == "--max").and_then(|i| args.get(i + 1)?.parse().ok()).unwrap_or(48);
    let mut last = String::new();
    loop {
        // el archivo de un rola que ya no vive no cuenta
        let st: Value = std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .filter(|v: &Value| v["pid"].as_u64().is_some_and(|p| PathBuf::from(format!("/proc/{p}")).exists()))
            .unwrap_or(Value::Null);
        let out = module(&st, lyrics, max).to_string();
        if out != last {
            let mut o = std::io::stdout();
            if writeln!(o, "{out}").and_then(|_| o.flush()).is_err() {
                return; // waybar se cerró
            }
            last = out;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_json() {
        let st = json!({ "pid": 1, "title": "Tu Boda", "artist": "Oscar Maydon & Co", "album": "<x>", "playing": true,
            "pos": 65.0, "dur": 190.0, "lyric": "Cuando te fuiste", "color": "#e07090", "sleep": "25m", "party": false });
        let m = module(&st, false, 48);
        assert_eq!(m["text"], "<span color=\"#e07090\">Tu Boda · Oscar Maydon &amp; Co</span>");
        assert_eq!(m["class"], "playing");
        let tip = m["tooltip"].as_str().unwrap();
        assert!(tip.contains("&lt;x&gt;") && tip.contains("1:05 / 3:10") && tip.contains("25m"));
        // con --lyrics, la línea que suena
        assert_eq!(module(&st, true, 48)["text"], "<span color=\"#e07090\">♪ Cuando te fuiste</span>");
        // corto
        assert_eq!(module(&json!({ "title": "abcdefghij" }), false, 5)["text"], "abcd…");
        // nada
        assert_eq!(module(&Value::Null, false, 48)["text"], "");
    }
}
