//! Sin conexión: las que más pones (3 veces o más), guardadas en
//! ~/.cache/rola/audio. Suenan al instante y sin internet. Se bajan de una en
//! una con yt-dlp, y si se pasa del tamaño elegido se van primero las que
//! llevan más tiempo sin sonar.

use crate::Ev;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::time::SystemTime;

/// Veces que hay que haber oído una canción para guardarla.
pub const PLAYS: usize = 3;

pub fn dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"));
    base.join("rola/audio")
}

/// Un archivo acabado (yt-dlp deja .part mientras baja).
fn done(p: &Path) -> bool {
    !matches!(p.extension().and_then(|e| e.to_str()), Some("part" | "ytdl" | "tmp"))
}

/// Las ya guardadas (por su videoId).
pub fn list() -> HashSet<String> {
    std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| done(p))
        .filter_map(|p| Some(p.file_stem()?.to_str()?.to_string()))
        .collect()
}

pub fn find(vid: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| done(p) && p.file_stem().and_then(|s| s.to_str()) == Some(vid))
}

/// Recién puesta: la última en irse si falta sitio.
pub fn touch(p: &Path) {
    if let Ok(f) = std::fs::File::options().append(true).open(p) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Lo que ocupa, en MB.
pub fn size_mb() -> u64 {
    files().iter().map(|f| f.1).sum::<u64>() / 1_000_000
}

fn files() -> Vec<(PathBuf, u64, SystemTime)> {
    std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path(), m.len(), m.modified().ok()?))
        })
        .collect()
}

/// Hasta `cap` MB: fuera las que llevan más tiempo sin sonar. Devuelve las borradas.
pub fn evict(cap_mb: u64) -> Vec<String> {
    let mut fs = files();
    fs.sort_by_key(|f| f.2);
    let mut total: u64 = fs.iter().map(|f| f.1).sum();
    let mut gone = Vec::new();
    for (p, len, _) in fs {
        if total <= cap_mb * 1_000_000 {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total -= len;
            if let Some(s) = p.file_stem().and_then(|s| s.to_str()) {
                gone.push(s.to_string());
            }
        }
    }
    gone
}

/// Un hilo que baja de una en una: (videoId, formato, tope en MB).
pub fn start(tx: Sender<Ev>) -> Sender<(String, String, u64)> {
    let (jobs, rx) = mpsc::channel::<(String, String, u64)>();
    std::thread::spawn(move || {
        let d = dir();
        let _ = std::fs::create_dir_all(&d);
        for (vid, format, cap) in rx {
            if find(&vid).is_some() {
                continue;
            }
            let ok = Command::new("yt-dlp")
                .args(["-q", "--no-warnings", "--no-playlist", "-f", &format])
                .arg("-o")
                .arg(d.join("%(id)s.%(ext)s"))
                .arg(format!("https://music.youtube.com/watch?v={vid}"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            let gone = evict(cap);
            if ok && !gone.contains(&vid) && tx.send(Ev::Cached { vid, gone }).is_err() {
                break;
            }
        }
    });
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_the_oldest() {
        let d = std::env::temp_dir().join(format!("rola-cache-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::env::set_var("XDG_CACHE_HOME", &d);
        let a = dir();
        std::fs::create_dir_all(&a).unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(1000);
        for (name, age) in [("old11111111", old), ("new11111111", SystemTime::now())] {
            let p = a.join(format!("{name}.webm"));
            std::fs::write(&p, vec![0u8; 700_000]).unwrap();
            std::fs::File::options().append(true).open(&p).unwrap().set_modified(age).unwrap();
        }
        std::fs::write(a.join("half1111111.webm.part"), b"x").unwrap();
        assert_eq!(list().len(), 2); // el .part no cuenta
        assert_eq!(evict(1), vec!["old11111111".to_string()]);
        assert!(find("new11111111").is_some() && find("old11111111").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
