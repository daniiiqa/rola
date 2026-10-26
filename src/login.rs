//! Entrar con la sesión que ya tienes abierta en el navegador: yt-dlp sabe
//! leer (y descifrar) las cookies de casi todos, así que rola se las pide a él
//! y se queda solo con las de youtube.com. Nada de herramientas de desarrollador.

use crate::api::Account;
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

#[derive(Clone)]
pub struct Browser {
    pub name: &'static str,
    pub arg: String, // lo que va en --cookies-from-browser
    used: Option<SystemTime>,
}

/// (nombre, carpeta bajo ~) de los de la familia de Firefox: yt-dlp los lee
/// todos como «firefox» si le das la carpeta del perfil.
const FIREFOXES: &[(&str, &str)] = &[
    ("Firefox", ".mozilla/firefox"),
    ("Zen", ".zen"),
    ("LibreWolf", ".librewolf"),
    ("Floorp", ".floorp"),
    ("Waterfox", ".waterfox"),
];

/// (nombre, carpeta bajo ~/.config, nombre para yt-dlp)
const CHROMES: &[(&str, &str, &str)] = &[
    ("Chrome", "google-chrome", "chrome"),
    ("Chromium", "chromium", "chromium"),
    ("Brave", "BraveSoftware/Brave-Browser", "brave"),
    ("Vivaldi", "vivaldi", "vivaldi"),
    ("Edge", "microsoft-edge", "edge"),
    ("Opera", "opera", "opera"),
];

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Los navegadores que hay, el último que se usó primero.
pub fn browsers() -> Vec<Browser> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut out = Vec::new();
    for (name, dir) in FIREFOXES {
        // de sus perfiles, el que tenga las cookies más recientes
        let best = std::fs::read_dir(home.join(dir))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| Some((mtime(&e.path().join("cookies.sqlite"))?, e.path())))
            .max_by_key(|(t, _)| *t);
        if let Some((t, p)) = best {
            out.push(Browser { name, arg: format!("firefox:{}", p.display()), used: Some(t) });
        }
    }
    for (name, dir, arg) in CHROMES {
        // como yt-dlp: bajo XDG_CONFIG_HOME si está puesto
        let conf = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".config"));
        let d = conf.join(dir);
        if d.is_dir() {
            let used = ["Default/Cookies", "Default/Network/Cookies"].iter().filter_map(|f| mtime(&d.join(f))).max();
            out.push(Browser { name, arg: arg.to_string(), used });
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.used));
    out
}

pub fn name_of(arg: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    FIREFOXES
        .iter()
        .find(|(_, d)| arg.starts_with(&format!("firefox:{home}/{d}/")))
        .map(|(n, _)| n.to_string())
        .or_else(|| CHROMES.iter().find(|c| c.2 == arg).map(|c| c.0.to_string()))
        .unwrap_or_else(|| arg.to_string())
}

/// Las cookies de youtube.com de ese navegador, ya con la primera cuenta que
/// tenga sesión, y todas las que hay (para elegir si son varias). Err con un
/// motivo legible si yt-dlp no pudo; vacío si no hay sesión (comprobado con
/// YouTube, no solo que haya cookies).
pub fn import(arg: &str) -> Result<(Value, Vec<Account>), String> {
    // yt-dlp vuelca el tarro entero (todas las webs): a un archivo solo
    // nuestro, del que se lee lo de YouTube y se borra al momento
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let tmp = PathBuf::from(dir).join(format!("rola-cookies-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(b"# Netscape HTTP Cookie File\n"))
        .map_err(|e| e.to_string())?;
    let out = Command::new("yt-dlp")
        .args(["--no-warnings", "--cookies-from-browser", arg, "--cookies"])
        .arg(&tmp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output();
    let jar = std::fs::read_to_string(&tmp).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp);
    let out = out.map_err(|e| format!("yt-dlp: {e}"))?;
    let cookie = youtube_cookies(&jar);
    if cookie.is_empty() {
        // yt-dlp se queja de «no URL» aunque haya ido bien: solo cuenta lo demás
        let err = String::from_utf8_lossy(&out.stderr);
        let why = err.lines().find(|l| l.contains("ERROR") && !l.contains("URL")).unwrap_or("").trim().to_string();
        return Err(why);
    }
    let mut auth = json!({ "cookie": cookie, "authuser": "0", "browser": arg });
    // hay cookies de YouTube, pero ¿con sesión, y de quién? (Err vacío: de nadie)
    let accounts = crate::api::Api::new(Path::new("")).accounts(&auth);
    let first = accounts.first().ok_or_else(String::new)?;
    pick(&mut auth, first);
    Ok((auth, accounts))
}

/// La sesión pasa a ser la de esa cuenta.
pub fn pick(auth: &mut Value, a: &Account) {
    auth["authuser"] = json!(a.authuser);
    auth["name"] = json!(a.name);
}

/// De un cookies.txt (formato Netscape), las de youtube.com como `a=b; c=d`.
fn youtube_cookies(jar: &str) -> String {
    jar.lines()
        .filter_map(|l| {
            // las HttpOnly vienen como «#HttpOnly_.youtube.com»
            let l = l.strip_prefix("#HttpOnly_").unwrap_or(l);
            let f: Vec<&str> = l.split('\t').collect();
            let host = f.first()?.trim_start_matches('.');
            (f.len() >= 7 && (host == "youtube.com" || host.ends_with(".youtube.com"))).then(|| format!("{}={}", f[5], f[6]))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Guarda la sesión (solo legible por ti).
pub fn save(file: &Path, auth: &Value) -> Result<(), String> {
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(file, auth.to_string()).map_err(|e| e.to_string())?;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_youtube_cookies() {
        let jar = "# Netscape HTTP Cookie File\n\
            .youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tabc\n\
            #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\tHSID\tdef\n\
            music.youtube.com\tFALSE\t/\tTRUE\t0\tX\t1\n\
            .google.com\tTRUE\t/\tTRUE\t0\tSID\tnope\n\
            .notyoutube.com\tTRUE\t/\tTRUE\t0\tY\tnope\n";
        assert_eq!(youtube_cookies(jar), "SAPISID=abc; HSID=def; X=1");
    }
}
