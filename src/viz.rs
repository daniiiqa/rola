//! El visualizador de verdad: cava (el de la terminal) escucha lo que suena y
//! cuenta cuánto hay en cada banda; rola solo lo dibuja. Corre únicamente
//! mientras se ve y algo suena (al pausar, apagarlo o cerrar la ventana, se
//! cierra). Ojo: cava oye todo el equipo, no solo a rola.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Bandas que se le piden a cava; al dibujar se estiran al ancho que haya.
pub const BANDS: usize = 32;
/// Hasta dónde llega cada banda (0..=MAX).
pub const MAX: u16 = 64;

pub struct Viz {
    child: Child,
    conf: std::path::PathBuf,
    pub bars: Arc<Mutex<Vec<u16>>>,
}

/// ¿Está cava? (una vez, al abrir)
pub fn available() -> bool {
    Command::new("cava").arg("-v").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

impl Viz {
    pub fn start() -> Option<Viz> {
        let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let conf = std::path::PathBuf::from(dir).join(format!("rola-cava-{}.conf", std::process::id()));
        // 20 fps alcanzan para que se vea vivo sin gastar de más
        let mut text = format!(
            "[general]\nbars = {BANDS}\nframerate = 20\n\n[output]\nmethod = raw\nraw_target = /dev/stdout\n\
             data_format = ascii\nascii_max_range = {MAX}\nbar_delimiter = 59\nframe_delimiter = 10\n"
        );
        // otra salida que escuchar (con varias tarjetas): su monitor de pulse/pipewire
        if let Ok(src) = std::env::var("ROLA_CAVA_SOURCE") {
            text.push_str(&format!("\n[input]\nmethod = pulse\nsource = {src}\n"));
        }
        std::fs::write(&conf, text).ok()?;
        let mut child = Command::new("cava")
            .arg("-p")
            .arg(&conf)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let bars: Arc<Mutex<Vec<u16>>> = Arc::new(Mutex::new(vec![0; BANDS]));
        let b = bars.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                let v: Vec<u16> = line.split(';').filter_map(|x| x.parse().ok()).collect();
                if v.len() == BANDS {
                    *b.lock().unwrap_or_else(|e| e.into_inner()) = v;
                }
            }
        });
        Some(Viz { child, conf, bars })
    }

    /// Lo de ahora, de 0 a 1 por banda.
    pub fn levels(&self) -> Vec<f64> {
        self.bars.lock().map(|v| v.iter().map(|&x| f64::from(x.min(MAX)) / f64::from(MAX)).collect()).unwrap_or_default()
    }
}

impl Drop for Viz {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.conf);
    }
}

/// `n` columnas a partir de las bandas (cada columna, la banda que le cae).
pub fn spread(levels: &[f64], n: usize) -> Vec<f64> {
    if levels.is_empty() {
        return vec![0.0; n];
    }
    (0..n).map(|c| levels[(c * levels.len() / n.max(1)).min(levels.len() - 1)]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads() {
        assert_eq!(spread(&[0.0, 1.0], 4), vec![0.0, 0.0, 1.0, 1.0]);
        assert_eq!(spread(&[0.2, 0.4, 0.6, 0.8], 2), vec![0.2, 0.6]);
        assert_eq!(spread(&[], 3), vec![0.0; 3]);
    }
}
