//! Lo que escuchas, apuntado en local (una línea JSON por canción oída al
//! menos 30 s), y el resumen de «rola wrapped». Nada sale de tu equipo.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// Lo que hace falta oír de una canción para que cuente.
pub const MIN_SECS: f64 = 30.0;

pub struct Play {
    pub t: i64, // segundos unix de cuando acabó
    pub title: String,
    pub artist: String,
    pub secs: f64,
}

pub fn log(file: &Path, p: &Play, vid: &str) {
    let line = json!({ "t": p.t, "vid": vid, "title": p.title, "artist": p.artist, "secs": (p.secs * 10.0).round() / 10.0 });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{line}");
    }
}

pub fn load(file: &Path) -> Vec<Play> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            Some(Play {
                t: v["t"].as_i64()?,
                title: v["title"].as_str()?.to_string(),
                artist: v["artist"].as_str().unwrap_or("").to_string(),
                secs: v["secs"].as_f64()?,
            })
        })
        .collect()
}

/// La hora local de un instante: (año, mes 1..12, día del año, día del mes, hora).
pub fn local(t: i64) -> (i32, u32, u32, u32, u32) {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    unsafe { libc::localtime_r(&tt, &mut tm) };
    (tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_yday as u32, tm.tm_mday as u32, tm.tm_hour as u32)
}

/// Este mes, este año o desde siempre.
#[derive(Clone, Copy, PartialEq)]
pub enum Period {
    Month,
    Year,
    All,
}

impl Period {
    pub const ALL: [Period; 3] = [Period::Month, Period::Year, Period::All];

    fn holds(self, t: i64, now: i64) -> bool {
        let (a, b) = (local(t), local(now));
        match self {
            Period::Month => a.0 == b.0 && a.1 == b.1,
            Period::Year => a.0 == b.0,
            Period::All => true,
        }
    }
}

#[derive(Default)]
pub struct Summary {
    pub secs: f64,
    pub plays: usize,
    pub songs: usize,                         // distintas
    pub artists: Vec<(String, f64)>,          // los que más, por tiempo
    pub top: Vec<(String, String, usize)>,    // canciones más puestas: título, artista, veces
    pub hours: [f64; 24],                     // tiempo por hora del día
    pub streak: u32,                          // días seguidos escuchando, hasta hoy
}

pub fn summary(plays: &[Play], period: Period, now: i64) -> Summary {
    let mine: Vec<&Play> = plays.iter().filter(|p| period.holds(p.t, now)).collect();
    let mut s = Summary { plays: mine.len(), ..Default::default() };
    let mut artists: HashMap<String, f64> = HashMap::new();
    let mut songs: HashMap<(String, String), usize> = HashMap::new();
    for p in &mine {
        s.secs += p.secs;
        s.hours[local(p.t).4 as usize] += p.secs;
        // con varios artistas, cuenta para cada uno
        for a in p.artist.split([',', '&']).map(str::trim).filter(|a| !a.is_empty()) {
            *artists.entry(a.to_string()).or_default() += p.secs;
        }
        *songs.entry((p.title.clone(), p.artist.clone())).or_default() += 1;
    }
    s.songs = songs.len();
    s.artists = artists.into_iter().collect();
    s.artists.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    s.artists.truncate(5);
    s.top = songs.into_iter().map(|((t, a), n)| (t, a, n)).collect();
    s.top.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    s.top.truncate(5);
    // la racha: hoy (o ayer, si hoy aún no) y los días seguidos de antes
    let day = |t: i64| {
        let (y, _, yd, _, _) = local(t);
        y as i64 * 400 + yd as i64
    };
    let days: std::collections::HashSet<i64> = plays.iter().map(|p| day(p.t)).collect();
    let mut t = now;
    if !days.contains(&day(t)) {
        t -= 86_400;
    }
    while days.contains(&day(t)) {
        s.streak += 1;
        t -= 86_400;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(t: i64, title: &str, artist: &str, secs: f64) -> Play {
        Play { t, title: title.into(), artist: artist.into(), secs }
    }

    #[test]
    fn sums_and_tops() {
        let now = 1_790_000_000; // un día cualquiera
        let plays = vec![
            play(now - 100, "Tu Boda", "Oscar Maydon, Fuerza Regida", 200.0),
            play(now - 50, "Tu Boda", "Oscar Maydon, Fuerza Regida", 200.0),
            play(now - 86_400, "RIKI", "Neton Vega", 150.0),
            play(now - 3 * 86_400, "Mar Azul", "Natanael Cano", 100.0),
            play(now - 400 * 86_400, "Viejita", "Alguien", 1000.0),
        ];
        let s = summary(&plays, Period::All, now);
        assert_eq!((s.plays, s.songs), (5, 4));
        assert_eq!(s.secs, 1650.0);
        assert_eq!(s.artists[0], ("Alguien".to_string(), 1000.0));
        assert_eq!(s.artists[1].1, 400.0); // Fuerza Regida y Oscar Maydon, 400 cada uno
        assert_eq!(s.top[0], ("Tu Boda".into(), "Oscar Maydon, Fuerza Regida".into(), 2));
        assert_eq!(s.streak, 2); // hoy y ayer; anteayer no
        assert_eq!(s.hours.iter().sum::<f64>(), 1650.0);
        // el año: la de hace 400 días no entra
        assert!(summary(&plays, Period::Year, now).plays <= 4);
    }

    #[test]
    fn log_and_load() {
        let f = std::env::temp_dir().join(format!("rola-stats-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&f);
        log(&f, &play(5, "A", "B", 31.25), "vid1");
        log(&f, &play(6, "C", "", 40.0), "vid2");
        let back = load(&f);
        let _ = std::fs::remove_file(&f);
        assert_eq!(back.len(), 2);
        assert_eq!((back[0].title.as_str(), back[0].secs), ("A", 31.3));
    }
}
