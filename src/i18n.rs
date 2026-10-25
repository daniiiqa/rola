//! Textos de la interfaz en cada idioma. `{}` se rellena con `fill`.

use crate::config::Choice;

#[derive(Clone, Copy, PartialEq)]
pub enum Lang {
    Es,
    En,
}

impl Choice for Lang {
    const ALL: &'static [Self] = &[Lang::Es, Lang::En];
    fn key(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }
}

impl Lang {
    /// Por $LANG la primera vez; luego manda lo que diga el config.
    pub fn detect() -> Lang {
        let l = std::env::var("LC_ALL").or_else(|_| std::env::var("LANG")).unwrap_or_default();
        if l.starts_with("es") {
            Lang::Es
        } else {
            Lang::En
        }
    }

    pub fn s(self) -> &'static Strings {
        match self {
            Lang::Es => &ES,
            Lang::En => &EN,
        }
    }
}

/// Cambia cada `{}` de `tpl`, en orden, por los de `args`.
pub fn fill(tpl: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut parts = tpl.split("{}");
    out.push_str(parts.next().unwrap_or(""));
    for (i, p) in parts.enumerate() {
        out.push_str(args.get(i).copied().unwrap_or(""));
        out.push_str(p);
    }
    out
}

type Keys = &'static [(&'static str, &'static str)];

pub struct Strings {
    pub lang_name: &'static str,
    // barra lateral
    pub home: &'static str,
    pub liked: &'static str,
    pub history: &'static str,
    pub albums: &'static str,
    pub artists: &'static str,
    pub queue: &'static str,
    pub no_account_hint: &'static str,
    // paneles
    pub search_placeholder: &'static str,
    pub lyrics: &'static str,
    pub nothing_playing: &'static str,
    pub idle_hint: &'static str,
    pub no_lyrics: &'static str,
    pub load_failed: &'static str,
    pub loading: &'static str,
    pub empty: &'static str,
    pub col_title: &'static str,
    pub col_artist: &'static str,
    pub col_album: &'static str,
    pub keys_title: &'static str,
    pub add_to: &'static str,
    // avisos
    pub removed: &'static str,
    pub need_account: &'static str,
    pub no_playlists: &'static str,
    pub play_failed: &'static str,
    pub many_errors: &'static str,
    pub autoplay_on: &'static str,
    pub autoplay_off: &'static str,
    pub radio_of: &'static str,
    pub queued: &'static str,
    pub like: &'static str,
    pub saved: &'static str,
    pub quality_next: &'static str,
    // ajustes
    pub settings: &'static str,
    pub sec_general: &'static str,
    pub sec_look: &'static str,
    pub sec_playback: &'static str,
    pub set_lang: &'static str,
    pub set_restore: &'static str,
    pub set_notify: &'static str,
    pub set_theme: &'static str,
    pub set_background: &'static str,
    pub set_borders: &'static str,
    pub set_bar: &'static str,
    pub set_icons: &'static str,
    pub set_side: &'static str,
    pub set_lyrics: &'static str,
    pub set_footer: &'static str,
    pub set_cover: &'static str,
    pub set_quality: &'static str,
    pub set_vol_step: &'static str,
    pub set_seek: &'static str,
    pub set_autoplay: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub bg_terminal: &'static str,
    pub bg_theme: &'static str,
    pub icons_nerd: &'static str,
    pub icons_plain: &'static str,
    pub borders: [&'static str; 6],
    pub bars: [&'static str; 5],
    pub qualities: [&'static str; 3],
    pub custom_themes: &'static str,
    // teclas
    pub help: Keys,
    pub foot_add: Keys,
    pub foot_help: Keys,
    pub foot_settings: Keys,
    pub foot_search: Keys,
    pub foot_lyrics: Keys,
    pub foot_queue: Keys,
    pub foot_main: Keys,
    // línea de órdenes
    pub cli_help: &'static str,
    pub login_intro: &'static str,
    pub login_no_cookie: &'static str,
    pub login_save_failed: &'static str,
    pub login_ok: &'static str,
    pub login_test_failed: &'static str,
}

pub static ES: Strings = Strings {
    lang_name: "Español",
    home: "Inicio",
    liked: "Me gusta",
    history: "Historial",
    albums: "Álbumes",
    artists: "Artistas",
    queue: "Cola",
    no_account_hint: "sin cuenta: ytm --login",
    search_placeholder: "buscar en YouTube Music  ( / )",
    lyrics: "letra",
    nothing_playing: "nada sonando",
    idle_hint: "nada sonando — Enter sobre una canción",
    no_lyrics: "esta canción no tiene letra en YouTube Music",
    load_failed: "no se pudo cargar: {}",
    loading: "cargando…",
    empty: "nada por aquí",
    col_title: "título",
    col_artist: "artista",
    col_album: "álbum",
    keys_title: "teclas",
    add_to: "añadir «{}» a…",
    removed: "quitada de la cola: {}",
    need_account: "necesitas cuenta: ytm --login",
    no_playlists: "no hay playlists en tu biblioteca (¿aún cargando?)",
    play_failed: "no se pudo reproducir: {}",
    many_errors: "varios fallos seguidos; ¿yt-dlp desactualizado?",
    autoplay_on: "autoplay (radio al acabar la cola) activado",
    autoplay_off: "autoplay (radio al acabar la cola) apagado",
    radio_of: "radio de {}",
    queued: "a la cola: {}",
    like: "me gusta: {}",
    saved: "ajustes guardados",
    quality_next: "se nota desde la próxima canción",
    settings: "ajustes",
    sec_general: "General",
    sec_look: "Apariencia",
    sec_playback: "Reproducción",
    set_lang: "Idioma",
    set_restore: "Recuperar la sesión al abrir",
    set_notify: "Duración de los avisos",
    set_theme: "Tema",
    set_background: "Fondo",
    set_borders: "Bordes",
    set_bar: "Barra de progreso",
    set_icons: "Iconos",
    set_side: "Ancho de la barra lateral",
    set_lyrics: "Ancho de la letra",
    set_footer: "Atajos al pie",
    set_cover: "Portada",
    set_quality: "Calidad del audio",
    set_vol_step: "Paso del volumen",
    set_seek: "Salto con < >",
    set_autoplay: "Autoplay al acabar la cola",
    on: "sí",
    off: "no",
    bg_terminal: "el de la terminal",
    bg_theme: "el del tema",
    icons_nerd: "Nerd Font",
    icons_plain: "texto",
    borders: ["como Hyprland", "redondeados", "rectos", "dobles", "gruesos", "sin bordes"],
    bars: ["línea", "línea con bola", "bloques", "puntos", "ascii"],
    qualities: ["la mejor", "media (≤128k)", "baja (ahorra datos)"],
    custom_themes: "temas propios: ~/.config/ytm/themes/*.conf",
    help: &[
        ("/", "buscar"),
        ("Enter", "reproducir / abrir"),
        ("espacio", "pausa"),
        ("n  p", "siguiente / anterior"),
        ("<  >", "atrasar / adelantar"),
        ("+  -", "volumen"),
        ("s", "aleatorio"),
        ("r", "repetir (no / todo / una)"),
        ("a", "autoplay: radio al acabar la cola"),
        ("z", "a la cola, detrás de la actual"),
        ("R", "radio de la canción"),
        ("L", "me gusta"),
        ("P", "añadir a una playlist"),
        ("y", "letra"),
        ("c", "portada"),
        ("o", "ajustes (idioma, tema…)"),
        ("d  Supr", "quitar de la cola (en Cola)"),
        ("J  K", "mover en la cola (en Cola)"),
        ("h  l  Tab", "cambiar de panel"),
        ("j  k  g  G", "moverse (^d ^u media página)"),
        ("Backspace", "atrás"),
        ("?", "esta ayuda"),
        ("q", "salir"),
    ],
    foot_add: &[("enter", "añadir"), ("j/k", "elegir"), ("esc", "cancelar")],
    foot_help: &[("cualquier tecla", "cerrar")],
    foot_settings: &[("j/k", "elegir"), ("h/l", "cambiar"), ("esc", "guardar y cerrar")],
    foot_search: &[("enter", "buscar"), ("esc", "volver"), ("^u", "borrar")],
    foot_lyrics: &[("j/k", "desplazar"), ("g/G", "inicio/fin"), ("h", "volver"), ("y", "cerrar letra")],
    foot_queue: &[
        ("enter", "saltar a"),
        ("d", "quitar"),
        ("J/K", "mover"),
        ("spc", "pausa"),
        ("n/p", "sig/ant"),
        ("P", "a playlist"),
        ("y", "letra"),
        ("?", "ayuda"),
    ],
    foot_main: &[
        ("/", "buscar"),
        ("spc", "pausa"),
        ("n/p", "sig/ant"),
        ("z", "a la cola"),
        ("R", "radio"),
        ("L", "me gusta"),
        ("P", "a playlist"),
        ("y", "letra"),
        ("o", "ajustes"),
        ("?", "ayuda"),
        ("q", "salir"),
    ],
    cli_help: "ytm — YouTube Music en la terminal\n\n  ytm            abre la app\n  ytm --login    conecta tu cuenta",
    login_intro: "Conectar tu cuenta de YouTube Music:

  1. Abre https://music.youtube.com con tu sesión iniciada.
  2. F12 → pestaña Red (Network) → filtra por  browse
  3. Haz scroll o cambia de página para que aparezca una petición POST \"browse\".
  4. Clic derecho en ella → Copiar → Copiar encabezados de la petición
     (Firefox/Zen: \"Copiar encabezados de solicitud\").
  5. Pégalos aquí y termina con Enter y luego Ctrl+D.
",
    login_no_cookie: "\nno encontré la cookie con SAPISID en lo pegado; ¿copiaste los headers de la petición?",
    login_save_failed: "no se pudo guardar {}: {}",
    login_ok: "\nlisto: {} playlists en tu biblioteca. Guardado en {}",
    login_test_failed: "\nguardado en {}, pero la prueba falló: {}",
};

pub static EN: Strings = Strings {
    lang_name: "English",
    home: "Home",
    liked: "Liked",
    history: "History",
    albums: "Albums",
    artists: "Artists",
    queue: "Queue",
    no_account_hint: "no account: ytm --login",
    search_placeholder: "search YouTube Music  ( / )",
    lyrics: "lyrics",
    nothing_playing: "nothing playing",
    idle_hint: "nothing playing — Enter on a song",
    no_lyrics: "this song has no lyrics on YouTube Music",
    load_failed: "couldn't load: {}",
    loading: "loading…",
    empty: "nothing here",
    col_title: "title",
    col_artist: "artist",
    col_album: "album",
    keys_title: "keys",
    add_to: "add “{}” to…",
    removed: "removed from queue: {}",
    need_account: "you need an account: ytm --login",
    no_playlists: "no playlists in your library (still loading?)",
    play_failed: "couldn't play: {}",
    many_errors: "several failures in a row; is yt-dlp outdated?",
    autoplay_on: "autoplay (radio when the queue ends) on",
    autoplay_off: "autoplay (radio when the queue ends) off",
    radio_of: "{} radio",
    queued: "queued: {}",
    like: "like: {}",
    saved: "settings saved",
    quality_next: "takes effect from the next song",
    settings: "settings",
    sec_general: "General",
    sec_look: "Appearance",
    sec_playback: "Playback",
    set_lang: "Language",
    set_restore: "Restore session on start",
    set_notify: "Notification duration",
    set_theme: "Theme",
    set_background: "Background",
    set_borders: "Borders",
    set_bar: "Progress bar",
    set_icons: "Icons",
    set_side: "Sidebar width",
    set_lyrics: "Lyrics width",
    set_footer: "Key hints footer",
    set_cover: "Cover art",
    set_quality: "Audio quality",
    set_vol_step: "Volume step",
    set_seek: "Seek with < >",
    set_autoplay: "Autoplay when queue ends",
    on: "on",
    off: "off",
    bg_terminal: "terminal's",
    bg_theme: "theme's",
    icons_nerd: "Nerd Font",
    icons_plain: "text",
    borders: ["like Hyprland", "rounded", "plain", "double", "thick", "none"],
    bars: ["line", "line with knob", "blocks", "dots", "ascii"],
    qualities: ["best", "medium (≤128k)", "low (saves data)"],
    custom_themes: "custom themes: ~/.config/ytm/themes/*.conf",
    help: &[
        ("/", "search"),
        ("Enter", "play / open"),
        ("space", "pause"),
        ("n  p", "next / previous"),
        ("<  >", "seek back / forward"),
        ("+  -", "volume"),
        ("s", "shuffle"),
        ("r", "repeat (off / all / one)"),
        ("a", "autoplay: radio when the queue ends"),
        ("z", "queue, right after the current one"),
        ("R", "song radio"),
        ("L", "like"),
        ("P", "add to a playlist"),
        ("y", "lyrics"),
        ("c", "cover art"),
        ("o", "settings (language, theme…)"),
        ("d  Del", "remove from queue (in Queue)"),
        ("J  K", "move in queue (in Queue)"),
        ("h  l  Tab", "switch panel"),
        ("j  k  g  G", "move (^d ^u half page)"),
        ("Backspace", "back"),
        ("?", "this help"),
        ("q", "quit"),
    ],
    foot_add: &[("enter", "add"), ("j/k", "pick"), ("esc", "cancel")],
    foot_help: &[("any key", "close")],
    foot_settings: &[("j/k", "pick"), ("h/l", "change"), ("esc", "save & close")],
    foot_search: &[("enter", "search"), ("esc", "back"), ("^u", "clear")],
    foot_lyrics: &[("j/k", "scroll"), ("g/G", "top/end"), ("h", "back"), ("y", "close lyrics")],
    foot_queue: &[
        ("enter", "jump to"),
        ("d", "remove"),
        ("J/K", "move"),
        ("spc", "pause"),
        ("n/p", "next/prev"),
        ("P", "to playlist"),
        ("y", "lyrics"),
        ("?", "help"),
    ],
    foot_main: &[
        ("/", "search"),
        ("spc", "pause"),
        ("n/p", "next/prev"),
        ("z", "queue"),
        ("R", "radio"),
        ("L", "like"),
        ("P", "to playlist"),
        ("y", "lyrics"),
        ("o", "settings"),
        ("?", "help"),
        ("q", "quit"),
    ],
    cli_help: "ytm — YouTube Music in the terminal\n\n  ytm            open the app\n  ytm --login    connect your account",
    login_intro: "Connect your YouTube Music account:

  1. Open https://music.youtube.com while signed in.
  2. F12 → Network tab → filter by  browse
  3. Scroll or switch pages until a POST \"browse\" request shows up.
  4. Right-click it → Copy → Copy request headers
     (Firefox/Zen: \"Copy Request Headers\").
  5. Paste them here and finish with Enter, then Ctrl+D.
",
    login_no_cookie: "\ncouldn't find a cookie with SAPISID in what you pasted; did you copy the request headers?",
    login_save_failed: "couldn't save {}: {}",
    login_ok: "\ndone: {} playlists in your library. Saved to {}",
    login_test_failed: "\nsaved to {}, but the test request failed: {}",
};
