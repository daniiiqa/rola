//! Seguir sonando con la ventana cerrada, a lo dtach: `rola` lanza (si no lo
//! hay ya) un proceso de fondo que corre el rola de siempre (`--inner`) en una
//! terminal virtual propia, y la ventana solo le pasa teclas y pinta lo que
//! dibuja. Cerrar la ventana no le llega al de dentro (sigue sonando, con su
//! MPRIS, waybar, Discord…); abrir `rola` otra vez se engancha a esa sesión.
//! `q` dentro cierra el de dentro, y con él la sesión.
//!
//! Ventana → fondo: tramas de 1 byte de tipo y 4 de largo (0: teclas, 1:
//! tamaño, cols y filas en u16). Fondo → ventana: lo que dibuja, tal cual.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Client = Arc<Mutex<Option<(u64, UnixStream)>>>;

const DATA: u8 = 0;
const SIZE: u8 = 1;

/// La señal que recibe el de dentro cuando se engancha otra ventana.
pub const REATTACH: libc::c_int = libc::SIGUSR1;
/// Y cuando se va la última: que no dibuje para nadie (ni corra cava).
pub const DETACH: libc::c_int = libc::SIGUSR2;

pub fn socket() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(dir).join(format!("rola-session-{}.sock", unsafe { libc::getuid() }))
}

/// ¿Se puede (y se debe) ir por sesión? Hace falta una terminal de verdad.
pub fn wanted() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

fn winsize(fd: i32) -> (u16, u16) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    (ws.ws_col.max(20), ws.ws_row.max(5))
}

fn set_winsize(fd: i32, cols: u16, rows: u16) {
    let ws = libc::winsize { ws_col: cols, ws_row: rows, ws_xpixel: 0, ws_ypixel: 0 };
    unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &ws) };
}

fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(5 + payload.len());
    v.push(kind);
    v.extend((payload.len() as u32).to_le_bytes());
    v.extend(payload);
    v
}

fn size_frame(cols: u16, rows: u16) -> Vec<u8> {
    let mut p = cols.to_le_bytes().to_vec();
    p.extend(rows.to_le_bytes());
    frame(SIZE, &p)
}

fn read_frame(s: &mut impl Read) -> Option<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    s.read_exact(&mut head).ok()?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    let mut p = vec![0u8; len.min(1 << 20)];
    s.read_exact(&mut p).ok()?;
    Some((head[0], p))
}

// ── la ventana ───────────────────────────────────────────────────────────────

static RESIZED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_winch(_: libc::c_int) {
    RESIZED.store(true, Ordering::Relaxed);
}

/// Se engancha a la sesión (lanzándola si no la hay) y hace de ventana hasta
/// que se cierre. Err si no se pudo: entonces rola va como siempre, sin sesión.
pub fn attach() -> Result<(), String> {
    let path = socket();
    let mut sock = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => {
            // sin sesión (o una muerta): una nueva, suelta de esta terminal
            let _ = std::fs::remove_file(&path);
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let mut cmd = Command::new(exe);
            cmd.arg("--serve").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            cmd.spawn().map_err(|e| e.to_string())?;
            let mut s = None;
            for _ in 0..100 {
                if let Ok(c) = UnixStream::connect(&path) {
                    s = Some(c);
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            s.ok_or("no arrancó la sesión de fondo")?
        }
    };

    let (cols, rows) = winsize(1);
    sock.write_all(&size_frame(cols, rows)).map_err(|e| e.to_string())?;

    // la terminal en crudo: las teclas van tal cual al de dentro
    let mut orig: libc::termios = unsafe { std::mem::zeroed() };
    unsafe { libc::tcgetattr(0, &mut orig) };
    let mut raw = orig;
    unsafe { libc::cfmakeraw(&mut raw) };
    unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) };
    unsafe { libc::signal(libc::SIGWINCH, on_winch as *const () as libc::sighandler_t) };

    // teclas → sesión
    let mut up = sock.try_clone().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut stdin = std::io::stdin();
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || up.write_all(&frame(DATA, &buf[..n])).is_err() {
                break;
            }
        }
    });
    // tamaño nuevo → sesión
    let mut resize = sock.try_clone().map_err(|e| e.to_string())?;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(50));
        if RESIZED.swap(false, Ordering::Relaxed) {
            let (c, r) = winsize(1);
            if resize.write_all(&size_frame(c, r)).is_err() {
                break;
            }
        }
    });
    // lo que dibuja → pantalla
    let mut out = std::io::stdout();
    let mut buf = [0u8; 16384];
    while let Ok(n) = sock.read(&mut buf) {
        if n == 0 || out.write_all(&buf[..n]).and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    // la terminal como estaba (y sin pantalla alternativa ni ratón, por si
    // se fue porque otra ventana tomó la sesión)
    let _ = out.write_all(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?25h\x1b[?1049l");
    let _ = out.flush();
    unsafe { libc::tcsetattr(0, libc::TCSANOW, &orig) };
    Ok(())
}

// ── el fondo ─────────────────────────────────────────────────────────────────

/// Una terminal virtual: (maestro, ruta del esclavo).
fn openpty() -> std::io::Result<(OwnedFd, PathBuf)> {
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        if m < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let master = OwnedFd::from_raw_fd(m);
        if libc::grantpt(m) != 0 || libc::unlockpt(m) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut name = [0 as libc::c_char; 128];
        if libc::ptsname_r(m, name.as_mut_ptr(), name.len()) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let path = std::ffi::CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned();
        Ok((master, PathBuf::from(path)))
    }
}

/// `rola --serve`: espera la primera ventana, arranca el de dentro con su
/// tamaño y le hace de puente mientras viva.
pub fn serve() {
    let path = socket();
    let Ok(listener) = UnixListener::bind(&path) else { return }; // ya hay otra
    let _ = std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600));
    let Ok((mut first, _)) = listener.accept() else { return };
    let Some((SIZE, p)) = read_frame(&mut first) else { return };
    let (cols, rows) = (u16::from_le_bytes([p[0], p[1]]), u16::from_le_bytes([p[2], p[3]]));

    let Ok((master, slave)) = openpty() else { return };
    set_winsize(master.as_raw_fd(), cols, rows);
    let Ok(tty) = File::options().read(true).write(true).open(&slave) else { return };
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = Command::new(exe);
    cmd.arg("--inner").stdin(tty.try_clone().unwrap()).stdout(tty.try_clone().unwrap()).stderr(tty);
    unsafe {
        cmd.pre_exec(|| {
            // su propia sesión, con la terminal virtual de terminal de control
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY, 0);
            Ok(())
        });
    }
    let Ok(mut child) = cmd.spawn() else { return };
    // sus copias del esclavo, fuera: si no, al irse el de dentro el maestro
    // no se enteraría (y la sesión se quedaría colgada)
    drop(cmd);
    let pid = child.id() as libc::pid_t;

    let master = Arc::new(File::from(master));
    // la ventana de ahora, con su número (para saber si la que se va es ella)
    let client: Client = Arc::default();

    // lo que entra de una ventana, al de dentro
    let feed = move |mut conn: UnixStream, id: u64, master: Arc<File>, client: Client| {
        std::thread::spawn(move || {
            while let Some((kind, p)) = read_frame(&mut conn) {
                match kind {
                    DATA => drop((&*master).write_all(&p)),
                    SIZE if p.len() == 4 => {
                        set_winsize(master.as_raw_fd(), u16::from_le_bytes([p[0], p[1]]), u16::from_le_bytes([p[2], p[3]]))
                    }
                    _ => {}
                }
            }
            // se fue (ventana cerrada): si era la de ahora, ya no hay ninguna
            let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
            if c.as_ref().is_some_and(|(n, _)| *n == id) {
                *c = None;
                unsafe { libc::kill(pid, DETACH) };
            }
        });
    };

    *client.lock().unwrap() = first.try_clone().ok().map(|s| (0, s));
    feed(first, 0, master.clone(), client.clone());

    // las que vengan después: toman la sesión (la anterior se suelta)
    {
        let (master, client) = (master.clone(), client.clone());
        std::thread::spawn(move || {
            for (id, conn) in (1..).zip(listener.incoming().flatten()) {
                let mut conn = conn;
                let Some((SIZE, p)) = read_frame(&mut conn) else { continue };
                set_winsize(master.as_raw_fd(), u16::from_le_bytes([p[0], p[1]]), u16::from_le_bytes([p[2], p[3]]));
                let Ok(out) = conn.try_clone() else { continue };
                if let Some((_, old)) = client.lock().unwrap_or_else(|e| e.into_inner()).replace((id, out)) {
                    let _ = old.shutdown(std::net::Shutdown::Both);
                }
                feed(conn, id, master.clone(), client.clone());
                // que se prepare la pantalla nueva y lo dibuje todo
                unsafe { libc::kill(pid, REATTACH) };
            }
        });
    }

    // lo que dibuja, a la ventana de ahora; sin ventana, se tira (si no se
    // leyera, se le llenaría la terminal y se quedaría parado a mitad de dibujo)
    let mut buf = [0u8; 16384];
    loop {
        match (&*master).read(&mut buf) {
            Ok(0) | Err(_) => break, // el de dentro se fue (q)
            Ok(n) => {
                let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
                if c.as_mut().is_some_and(|(_, s)| s.write_all(&buf[..n]).is_err()) {
                    *c = None;
                    unsafe { libc::kill(pid, DETACH) };
                }
            }
        }
    }
    if let Some((_, c)) = client.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = c.shutdown(std::net::Shutdown::Both);
    }
    let _ = std::fs::remove_file(&path);
    let _ = child.wait();
}
