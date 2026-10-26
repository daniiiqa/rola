//! mpv por IPC. El MPRIS no es cosa suya sino de rola (ver mpris.rs).

use crate::Ev;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::Duration;

pub struct Mpv {
    child: Child,
    sock: UnixStream,
    path: PathBuf,
}

impl Mpv {
    pub fn start(volume: i64, format: &str, tx: Sender<Ev>) -> Result<Mpv, String> {
        let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        // sockets de sesiones que murieron sin limpiar (terminal cerrada a la mala)
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(pid) = name.strip_prefix("rola-mpv-").and_then(|s| s.strip_suffix(".sock")) {
                if !PathBuf::from(format!("/proc/{pid}")).exists() {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let path = PathBuf::from(dir).join(format!("rola-mpv-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut cmd = Command::new("mpv");
        cmd.args([
            "--idle=yes",
            "--no-video",
            "--no-terminal",
            "--force-window=no",
            "--title=rola",
            // ni mpv-mpris ni otros scripts del sistema: el MPRIS lo pone rola,
            // que es quien sabe qué va después (ytdl_hook viene dentro, sigue)
            "--load-scripts=no",
            // solo si rola no pudo resolver el enlace (ver stream.rs): web_music
            // no da 403 nada más pedirlo, aunque sí peor calidad
            "--ytdl-raw-options-append=extractor-args=youtube:player_client=web_music",
        ])
        .arg(format!("--input-ipc-server={}", path.display()))
        .arg(format!("--volume={volume}"))
        .arg(format!("--ytdl-format={format}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        // si rola muere (ventana cerrada, kill), mpv se va con él
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| format!("no se pudo abrir mpv: {e}"))?;

        let mut sock = None;
        for _ in 0..100 {
            if let Ok(s) = UnixStream::connect(&path) {
                sock = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let sock = sock.ok_or("mpv no abrió el socket IPC")?;
        let reader = sock.try_clone().map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                let ev = match msg["event"].as_str() {
                    Some("property-change") => {
                        Ev::Prop(msg["name"].as_str().unwrap_or("").to_string(), msg["data"].clone())
                    }
                    Some("end-file") => match msg["reason"].as_str() {
                        Some(r @ ("eof" | "error")) => Ev::End(r == "error"),
                        _ => continue,
                    },
                    _ => continue,
                };
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });

        let mut mpv = Mpv { child, sock, path };
        for (i, p) in ["time-pos", "duration", "pause", "volume"].iter().enumerate() {
            mpv.cmd(json!(["observe_property", i + 1, p]));
        }
        Ok(mpv)
    }

    pub fn cmd(&mut self, args: Value) {
        let _ = writeln!(self.sock, "{}", json!({ "command": args }));
    }

    /// `direct`: el enlace del audio ya resuelto con yt-dlp (arranca al momento);
    /// sin él, mpv lo resuelve con su hook de yt-dlp (un par de segundos).
    pub fn load(&mut self, video_id: &str, direct: Option<&str>, title: &str, start: Option<f64>) {
        let page = format!("https://music.youtube.com/watch?v={video_id}");
        let mut opts = Vec::new();
        if let Some(t) = start.filter(|&t| t > 1.0) {
            opts.push(format!("start={t:.2}"));
        }
        if direct.is_some() {
            // el enlace directo no trae título; %n% escapa comas en la lista
            opts.push(format!("force-media-title=%{}%{title}", title.len()));
        }
        let url = direct.unwrap_or(&page);
        // loadfile url flags index options (mpv ≥ 0.38)
        match opts.is_empty() {
            true => self.cmd(json!(["loadfile", url, "replace"])),
            false => self.cmd(json!(["loadfile", url, "replace", -1, opts.join(",")])),
        }
        self.cmd(json!(["set_property", "pause", false]));
    }
}

/// Un mpv suelto que toca el final de la que se va, apagándose en `secs`:
/// la mitad del fundido (la otra es la siguiente entrando en el de siempre).
pub fn ghost(url: &str, from: f64, secs: f64, volume: i64, sound: Option<&str>) -> Option<Child> {
    let mut cmd = Command::new("mpv");
    cmd.args(["--no-video", "--no-terminal", "--force-window=no", "--load-scripts=no"])
        .arg(format!("--start={from:.2}"))
        .arg(format!("--length={:.2}", secs + 0.3))
        .arg(format!("--volume={volume}"))
        // afade cuenta en tiempo de la canción, que con --start empieza en `from`
        // con el mismo sonido que el de siempre (graves, voces…) delante
        .arg(format!("--af=lavfi=[{}afade=t=out:st={from:.2}:d={secs:.2}]", sound.map(|g| format!("{g},")).unwrap_or_default()))
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    cmd.spawn().ok()
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.cmd(json!(["quit"]));
        for _ in 0..20 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
    }
}
