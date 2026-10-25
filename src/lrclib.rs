//! Letras sincronizadas de LRCLIB (lrclib.net: abierta, sin clave). YouTube
//! Music solo da la letra en texto plano; con los tiempos, la línea que suena
//! se resalta y la letra avanza con la canción.

use crate::api::Item;
use serde_json::Value;
use std::time::Duration;

/// (segundo en que empieza, línea)
pub type Lines = Vec<(f64, String)>;

/// La letra con tiempos de esa canción, si LRCLIB la tiene (de la misma
/// versión: duración parecida).
pub fn synced(it: &Item) -> Option<Lines> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .user_agent("rola (https://github.com/daniiiqa/rola)")
        .build();
    let artist = it.artist.split([',', '&']).next().unwrap_or("").trim();
    // «Canción (feat. X)» suele estar como «Canción»
    let short = it.title.split(" (").next().unwrap_or(&it.title).trim();
    let mut titles = vec![it.title.trim()];
    if short != titles[0] {
        titles.push(short);
    }
    for title in titles {
        let res = agent
            .get("https://lrclib.net/api/search")
            .query("track_name", title)
            .query("artist_name", artist)
            .call()
            .ok()?
            .into_string()
            .ok()?;
        let found: Vec<Value> = serde_json::from_str(&res).ok()?;
        if let Some(l) = pick(&found, it) {
            return Some(l);
        }
    }
    None
}

/// De lo encontrado, el de duración más cercana (y del mismo artista).
fn pick(found: &[Value], it: &Item) -> Option<Lines> {
    let dur = secs(&it.duration);
    let artist = norm(it.artist.split([',', '&']).next().unwrap_or(""));
    found
        .iter()
        .filter(|x| x["syncedLyrics"].as_str().is_some_and(|s| !s.trim().is_empty()))
        .filter_map(|x| {
            let off = match (dur, x["duration"].as_f64()) {
                (Some(a), Some(b)) => (a - b).abs(),
                _ => 0.0,
            };
            let same = norm(x["artistName"].as_str().unwrap_or("")).contains(&artist);
            // otra versión (en vivo, extendida…) iría a destiempo
            (off <= 8.0).then_some((!same, off, x))
        })
        .min_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, _, x)| parse(x["syncedLyrics"].as_str().unwrap_or("")))
        .filter(|l| !l.is_empty())
}

/// «[mm:ss.xx] texto», a veces con varios tiempos por línea.
pub fn parse(lrc: &str) -> Lines {
    let mut out = Vec::new();
    for line in lrc.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            let Some(t) = stamp(tag.0) else { break }; // [ar:…] y demás
            times.push(t);
            rest = tag.1;
        }
        for t in times {
            out.push((t, rest.trim().to_string()));
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn stamp(s: &str) -> Option<f64> {
    let (m, s) = s.split_once(':')?;
    Some(m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?)
}

fn secs(d: &str) -> Option<f64> {
    d.split(':').try_fold(0.0, |a, p| p.parse::<f64>().ok().map(|n| a * 60.0 + n)).filter(|&s| s > 0.0)
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Kind;
    use serde_json::json;

    #[test]
    fn lrc_lines() {
        let l = parse("[ar:Queen]\n[00:07.13] Caught in a landslide\n[01:00.00][02:00.50]Coro\n[00:00.50]\n");
        assert_eq!(l.len(), 4);
        assert_eq!(l[0], (0.5, String::new()));
        assert_eq!(l[1], (7.13, "Caught in a landslide".into()));
        assert_eq!(l[2], (60.0, "Coro".into()));
        assert_eq!(l[3], (120.5, "Coro".into()));
    }

    #[test]
    fn picks_same_version() {
        let it = Item { kind: Kind::Song, title: "Song".into(), artist: "Band, Other".into(), album: String::new(), duration: "3:30".into(), video_id: None, browse_id: None, thumb: String::new() };
        let found = vec![
            json!({"artistName": "Band", "duration": 290.0, "syncedLyrics": "[00:01.00]live"}),
            json!({"artistName": "Covers Inc", "duration": 210.0, "syncedLyrics": "[00:01.00]cover"}),
            json!({"artistName": "Band", "duration": 211.0, "syncedLyrics": "[00:01.00]studio"}),
            json!({"artistName": "Band", "duration": 210.0, "plainLyrics": "plain"}),
        ];
        assert_eq!(pick(&found, &it).unwrap()[0].1, "studio");
        // solo la versión en vivo: no, iría a destiempo
        assert!(pick(&found[..1], &it).is_none());
    }
}
