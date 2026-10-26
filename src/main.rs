//! rola — YouTube Music en la terminal, al estilo spotify-tui.
//!
//!     rola            abre la app
//!     rola --login    conecta tu cuenta (biblioteca, playlists, me gusta)

mod api;
mod bar;
mod cache;
mod config;
mod discord;
mod hypr;
mod i18n;
mod login;
mod lrclib;
mod mouse;
mod mpris;
mod mpv;
mod scrobble;
mod notif;
mod palette;
mod party;
mod settings;
mod spotify;
mod stats;
mod stream;
mod theme;
mod ui;

use api::{Api, Item, Kind};
use config::{Choice, Config};
use i18n::{fill, Lang, Strings};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::style::Color;
use ratatui::widgets::{ListState, TableState};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

pub enum Ev {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Media(mpris::Media), // desde fuera: playerctl, teclas multimedia…
    Resize,
    Prop(String, Value),
    End(bool),
    Loaded { seq: u64, title: String, reset: bool, radio: bool, res: Result<Vec<Item>, String> },
    Playlists(Vec<Item>),
    Radio { seed: String, res: Result<Vec<Item>, String> },
    Lyrics { vid: String, lyrics: Lyrics },
    Cover { vid: String, img: image::DynamicImage, pal: Option<(palette::Hsl, palette::Hsl)> },
    Notify(String, bool),
    Stream { vid: String, format: &'static str, link: Option<stream::Link> },
    Ready(String), // ya pasó el rato tras el que su enlace deja de dar 403
    LoggedIn(Result<String, String>), // nombre del navegador, o por qué no
    SpotifyIn(Result<String, String>),
    ScrobbleIn(Result<(String, String), String>),
    Party(party::Req), // lo que piden desde la página de la fiesta
    RetryHome,         // el inicio falló al arrancar: otra vez
    Info { vid: String, res: Result<api::SongInfo, String> },
    Cached { vid: String, gone: Vec<String> }, // una más guardada sin conexión (y las que salieron) // (token, usuario) de ListenBrainz, o por qué no           // usuario de Spotify, o por qué no
    SpPlaylists(Vec<Item>),
    Matched { sp: String, vid: Option<String> }, // canción de Spotify → la de YouTube Music
    Look, // Hyprland recargó su config (p. ej. al cambiar de look)
}

#[derive(Clone, PartialEq)]
enum Side {
    Home,
    Liked,
    History,
    Albums,
    Artists,
    Queue,
    Sep,
    Hint,
    Head(bool), // cabecera de sección con su logo: Spotify (true) o YouTube Music
    Playlist(String),
    SpLiked,
    SpAlbums,
}

/// Temporizador para dormir.
#[derive(Clone, Copy, PartialEq)]
pub enum Sleep {
    Off,
    EndOfSong,
    At(Instant),
}

/// Lo que tarda en bajar el volumen antes de pararse.
const SLEEP_FADE: f64 = 12.0;

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Search,
    Side,
    Main,
    Lyrics,
}

pub enum Lyrics {
    Loading,
    Missing,
    Text(String, String), // letra, fuente
    Synced(lrclib::Lines, String), // con tiempos: se resalta la que suena
    Err(String),
}

pub enum Popup {
    Help,
    Settings { sel: usize },
    AddTo { item: Item, state: ListState },
    /// navegadores de los que entrar; busy: leyendo uno; err: por qué no se pudo
    Login { browsers: Vec<login::Browser>, state: ListState, busy: Option<String>, err: Option<String> },
    /// la ficha de una canción (i); sin info aún, cargando
    Info { item: Item, info: Option<Box<api::SongInfo>>, err: Option<String>, state: ListState },
    /// Discord: el Application ID que se va pegando
    Discord { input: String, err: Option<String> },
    /// modo fiesta: el QR y el enlace
    Party,
    /// ListenBrainz: el token que se va pegando
    Scrobble { input: String, busy: bool, err: Option<String> },
    /// rola wrapped: lo escuchado, por periodo (0 mes, 1 año, 2 siempre)
    Wrapped { period: usize, plays: Vec<stats::Play> },
    /// conectar Spotify: el Client ID que se va escribiendo
    Spotify { input: String, busy: bool, err: Option<String> },
}

#[derive(Clone, Copy, PartialEq)]
enum Repeat {
    Off,
    All,
    One,
}

enum Load {
    Home,
    Liked,
    History,
    Albums,
    Artists,
    Playlist(String),
    Album(String),
    Artist(String),
    Search(String),
    SpLiked,
    SpAlbums,
}

struct View {
    title: String,
    items: Vec<Item>,
    queue: bool, // la cola en vivo (items vacío)
    radio: bool, // Enter en una canción pone su radio (inicio, búsqueda), como en YT Music
    state: TableState,
}

struct App {
    api: Api,
    tx: Sender<Ev>,
    mpv: mpv::Mpv,
    cfg: Config,
    theme: theme::Theme,      // el que se pinta: el del look, teñido por la portada si toca
    theme_base: theme::Theme, // el del look
    cover_pal: Option<(palette::Hsl, palette::Hsl)>,
    tint_from: (Color, Color), // de dónde viene el cambio de color (principal, acento)
    tint_at: Instant,
    theme_mtime: Option<SystemTime>,
    cfg_mtimes: [Option<SystemTime>; 2],
    rounded: bool, // Hyprland redondea las ventanas (para los bordes «como Hyprland»)
    streams: HashMap<String, stream::Link>, // video_id → enlace directo ya resuelto
    retried: Option<String>, // la que ya falló una vez con enlace directo
    upgrade: Option<String>, // suena por mpv (peor calidad) hasta que su enlace sirva
    fetching: HashSet<String>,
    direct: bool, // la actual se cargó con enlace directo
    waiting: Option<(String, Option<f64>)>, // a punto de sonar, esperando su enlace (video_id, desde)
    hover: Option<String>, // canción bajo el cursor en el último tick
    focus: Focus,
    input: String,
    side: Vec<(String, Side)>,
    side_state: ListState,
    views: Vec<View>,
    seq: u64,
    loading: Option<String>,
    queue: Vec<Item>,
    qi: Option<usize>,
    added: usize, // canciones metidas con z detrás de la actual
    loaded: bool, // mpv tiene la actual cargada (no al abrir con la cola restaurada)
    resume: Option<(Option<String>, f64)>, // dónde se quedó la sesión anterior (canción, segundo)
    playlists: Vec<Item>,
    lyrics_on: bool,
    lyrics: Option<(String, Lyrics)>, // video_id → letra
    lyrics_scroll: u16,
    lyrics_hand: Option<Instant>, // cuándo se movió a mano (deja de seguir a la canción un rato)
    lyrics_auto: Cell<u16>,       // dónde la dejó el último dibujo siguiendo a la canción
    lyrics_total: Cell<u16>,      // filas de la letra con tiempos (ya partidas al ancho)
    sleep: Sleep,
    desktop: Option<Sender<notif::Song>>, // avisos del escritorio
    ghost: Option<std::process::Child>,   // el final de la anterior apagándose (fundido)
    xfaded: u64,                          // track_n de la que ya empezó a fundirse
    fade_in: Option<f64>,                 // la siguiente que se cargue entra subiendo en tantos s
    af_on: bool,                          // mpv tiene puesto el filtro de entrada
    listened: f64,                        // segundos oídos de verdad de la actual (sin saltos)
    song_unix: i64,                       // cuándo empezó la actual (segundo unix)
    party: Option<party::Party>,          // modo fiesta encendido
    rpc: Option<Sender<discord::Presence>>, // el estado de Discord
    rpc_last: (u64, discord::Presence),     // lo último mandado (con su track_n)
    bar_last: Value,                        // lo último escrito para la barra
    cached: HashSet<String>,                // guardadas sin conexión
    plays: HashMap<String, usize>,          // veces oída cada una (para saber cuáles guardar)
    dl: Option<Sender<(String, String, u64)>>, // el hilo que las baja
    local: Option<String>,                  // la actual suena del disco (su ruta)
    lb: Option<(String, Sender<scrobble::Job>)>, // ListenBrainz: usuario y su hilo
    popup: Option<Popup>,
    picker: Picker,
    cover_on: bool,
    cover: Option<(String, Option<StatefulProtocol>)>, // video_id → portada (None: cargando)
    pos: Option<f64>,
    dur: Option<f64>,
    paused: bool,
    volume: i64,
    shuffle: bool,
    repeat: Repeat,
    autoplay: bool,
    radio_pending: Option<(String, bool)>, // radio pedida (semilla, sonar al llegar)
    radio_seed: Option<String>, // la última semilla pedida
    errors: u32, // fallos seguidos; con 3 deja de saltar
    note: Option<(String, bool, Instant)>,
    rng: u64,
    quit: bool,
    epoch: Instant,               // reloj de las animaciones
    song_start: Instant,          // para que el título largo empiece quieto
    splash: Option<Instant>,      // canción nueva: su nombre arriba un momento
    hits: RefCell<mouse::Hits>,   // dónde quedó cada cosa al dibujar
    last_click: Option<(Instant, u16, u16)>,
    mpris: Option<mpris::Mpris>,
    track_n: u64, // sube con cada canción que se pone (para MPRIS)
    login_skipped: bool, // dijo «luego» al aviso de conectar la cuenta
    spotify: Option<spotify::Spotify>,
    sp_playlists: Vec<Item>,
    sp_match: HashMap<String, Option<String>>, // canción de Spotify → video_id (None: no está)
    sp_dirty: bool,                            // hay emparejamientos sin guardar
    home_retried: bool,
    sp_saved: Instant,
    saved_at: Instant,                         // última vez que se guardó la sesión
    matcher: Arc<(Mutex<VecDeque<Item>>, Condvar)>, // las que faltan por buscar en YouTube Music
    matching: HashSet<String>,
    sp_wait: Option<String>, // la que toca sonar, esperando a que se encuentre
}

/// Lo que dura el aviso de canción nueva.
pub const SPLASH: Duration = Duration::from_millis(3500);
/// Lo que tarda en pasar de los colores de una portada a los de otra.
const TINT: Duration = Duration::from_millis(900);
/// Tras mover la letra a mano, lo que tarda en volver a seguir a la canción.
pub const LYRICS_HAND: Duration = Duration::from_secs(4);

/// Pide (o deja de pedir) a la terminal los eventos del ratón.
pub fn mouse_capture(on: bool) {
    use ratatui::crossterm::{event as ev, execute};
    let mut out = std::io::stdout();
    let _ = if on { execute!(out, ev::EnableMouseCapture) } else { execute!(out, ev::DisableMouseCapture) };
}

fn data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"))
        .join("rola")
}

fn state_file() -> PathBuf {
    std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"))
        .join("rola/state.json")
}

impl App {
    fn new(tx: Sender<Ev>, picker: Picker) -> Result<App, String> {
        let st: Value = std::fs::read_to_string(state_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({}));
        let volume = st["volume"].as_i64().unwrap_or(70);
        let cfg = Config::load();
        let mpv = mpv::Mpv::start(volume, cfg.quality.ytdl_format(), tx.clone())?;
        let api = Api::new(&data_dir().join("auth.json"));
        let mut app = App {
            api,
            tx: tx.clone(),
            mpv,
            theme: theme::load(&cfg),
            theme_base: theme::load(&cfg),
            cover_pal: None,
            tint_from: (Color::Reset, Color::Reset),
            tint_at: Instant::now(),
            theme_mtime: theme::mtime(theme::source(&cfg).as_deref()),
            cfg_mtimes: config::mtimes(),
            rounded: hypr::rounding().is_none_or(|r| r > 0),
            streams: HashMap::new(),
            retried: None,
            upgrade: None,
            fetching: HashSet::new(),
            direct: false,
            waiting: None,
            hover: None,
            cfg,
            focus: Focus::Main,
            input: String::new(),
            side: Vec::new(),
            side_state: ListState::default().with_selected(Some(0)),
            views: Vec::new(),
            seq: 0,
            loading: None,
            queue: Vec::new(),
            qi: None,
            added: 0,
            loaded: false,
            resume: None,
            playlists: Vec::new(),
            lyrics_on: st["lyrics"].as_bool().unwrap_or(false),
            lyrics: None,
            lyrics_scroll: 0,
            lyrics_hand: None,
            lyrics_auto: Cell::new(0),
            lyrics_total: Cell::new(0),
            sleep: Sleep::Off,
            desktop: None,
            ghost: None,
            xfaded: 0,
            fade_in: None,
            af_on: false,
            listened: 0.0,
            song_unix: 0,
            party: None,
            rpc: None,
            rpc_last: (0, None),
            bar_last: Value::Null,
            cached: cache::list(),
            plays: stats::counts(&data_dir().join("history.jsonl")),
            dl: None,
            local: None,
            lb: scrobble::load(&data_dir().join("listenbrainz.json")).map(|(tok, user)| (user, scrobble::start(tok))),
            popup: None,
            picker,
            cover_on: st["cover"].as_bool().unwrap_or(true),
            cover: None,
            pos: None,
            dur: None,
            paused: true,
            volume,
            shuffle: st["shuffle"].as_bool().unwrap_or(false),
            repeat: match st["repeat"].as_str() {
                Some("all") => Repeat::All,
                Some("one") => Repeat::One,
                _ => Repeat::Off,
            },
            autoplay: st["autoplay"].as_bool().unwrap_or(true),
            radio_pending: None,
            radio_seed: None,
            errors: 0,
            note: None,
            rng: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1,
            quit: false,
            epoch: Instant::now(),
            song_start: Instant::now(),
            splash: None,
            hits: RefCell::default(),
            last_click: None,
            mpris: mpris::Mpris::start(tx.clone()),
            track_n: 0,
            login_skipped: st["login_skipped"].as_bool().unwrap_or(false),
            spotify: spotify::Spotify::load(&data_dir().join("spotify.json")),
            sp_playlists: Vec::new(),
            sp_match: std::fs::read_to_string(data_dir().join("spotify-match.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            sp_dirty: false,
            home_retried: false,
            sp_saved: Instant::now(),
            saved_at: Instant::now(),
            matcher: Arc::default(),
            matching: HashSet::new(),
            sp_wait: None,
        };
        app.fill_side();
        app.start_matchers();
        if !app.cfg.restore {
            return Ok(app.start());
        }
        app.queue = st["queue"].as_array().into_iter().flatten().filter_map(Item::from_json).collect();
        if let Some(qi) = st["qi"].as_u64().map(|i| i as usize).filter(|&i| i < app.queue.len()) {
            let pos = st["pos"].as_f64().unwrap_or(0.0);
            app.qi = Some(qi);
            app.resume = Some((app.queue[qi].video_id.clone(), pos));
            app.pos = Some(pos);
            app.dur = parse_duration(&app.queue[qi].duration);
            if app.lyrics_on {
                app.fetch_lyrics();
            }
            app.fetch_cover();
        }
        Ok(app.start())
    }

    fn start(mut self) -> App {
        self.apply_sound();
        // lo que tocaba guardar y no dio tiempo (o se borró a mano)
        if self.cfg.offline_mb > 0 {
            self.offline_changed();
        }
        self.load_account();
        // sin cuenta: ofrecer entrar desde el navegador (una vez; luego, en ajustes)
        if !self.api.authed() && !self.login_skipped && !login::browsers().is_empty() {
            self.open_login();
        }
        self
    }

    /// Lo que depende de la cuenta: playlists de la barra lateral e inicio.
    fn load_account(&mut self) {
        self.playlists.clear();
        self.fill_side();
        self.load_spotify();
        if self.api.authed() {
            let (api, tx) = (self.api.clone(), self.tx.clone());
            std::thread::spawn(move || match api.library_playlists() {
                Ok(p) => drop(tx.send(Ev::Playlists(p))),
                Err(e) => drop(tx.send(Ev::Notify(format!("playlists: {e}"), true))),
            });
        }
        self.open(self.s().home, Load::Home, true);
    }

    // ── cuenta ──
    pub fn open_login(&mut self) {
        let browsers = login::browsers();
        if browsers.is_empty() && !self.api.authed() {
            return self.notify(self.s().login_none, true);
        }
        let state = ListState::default().with_selected(Some(0));
        self.popup = Some(Popup::Login { browsers, state, busy: None, err: None });
    }

    fn login_key(&mut self, k: KeyEvent) {
        let authed = self.api.authed();
        let Some(Popup::Login { browsers, state, busy, err }) = &mut self.popup else { return };
        if busy.is_some() {
            return; // leyendo; enseguida acaba
        }
        let n = browsers.len() + authed as usize; // la última fila: cerrar sesión
        let i = state.selected().unwrap_or(0);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.popup = None;
                self.login_skipped |= !authed;
            }
            KeyCode::Char('j') | KeyCode::Down => state.select(Some((i + 1).min(n.saturating_sub(1)))),
            KeyCode::Char('k') | KeyCode::Up => state.select(Some(i.saturating_sub(1))),
            KeyCode::Enter => match browsers.get(i).cloned() {
                Some(b) => {
                    *busy = Some(b.name.to_string());
                    *err = None;
                    let (tx, file, s) = (self.tx.clone(), data_dir().join("auth.json"), self.s());
                    std::thread::spawn(move || {
                        let res = login::import(&b.arg)
                            .and_then(|v| login::save(&file, &v))
                            .map(|_| b.name.to_string())
                            .map_err(|e| if e.is_empty() { fill(s.login_no_session, &[b.name]) } else { e });
                        let _ = tx.send(Ev::LoggedIn(res));
                    });
                }
                None if authed => {
                    let _ = std::fs::remove_file(data_dir().join("auth.json"));
                    self.api.reload();
                    self.popup = None;
                    self.notify(self.s().logged_out, false);
                    self.load_account();
                }
                None => {}
            },
            _ => {}
        }
    }

    /// Cómo sale la cuenta en los ajustes.
    pub fn account_name(&self) -> String {
        match (self.api.authed(), self.api.browser()) {
            (false, _) => self.s().account_none.into(),
            (true, Some(b)) => login::name_of(&b),
            (true, None) => self.s().account_pasted.into(),
        }
    }

    pub fn s(&self) -> &'static Strings {
        self.cfg.lang.s()
    }

    fn save_state(&self) {
        let st = json!({
            "volume": self.volume,
            "shuffle": self.shuffle,
            "repeat": match self.repeat { Repeat::Off => "off", Repeat::All => "all", Repeat::One => "one" },
            "autoplay": self.autoplay,
            "lyrics": self.lyrics_on,
            "cover": self.cover_on,
            "login_skipped": self.login_skipped,
            "queue": self.queue.iter().map(Item::to_json).collect::<Vec<_>>(),
            "qi": self.qi,
            "pos": if self.loaded { self.pos } else { self.resume.as_ref().map(|r| r.1) },
        });
        let f = state_file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, st.to_string());
    }

    fn notify(&mut self, msg: impl Into<String>, err: bool) {
        self.note = Some((msg.into(), err, Instant::now()));
    }

    fn rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Al azar, pero sin el mismo artista dos veces seguidas (contando con
    /// `prev`, la que suena antes de estas).
    fn shuffle_vec(&mut self, v: &mut [Item], prev: Option<&Item>) {
        for i in (1..v.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
        spread(v, prev);
    }

    // ── barra lateral ──
    fn fill_side(&mut self) {
        let (t, playlists) = (self.s(), &self.playlists);
        let mut s = vec![(t.home.to_string(), Side::Home)];
        if self.api.authed() {
            s.push((t.liked.into(), Side::Liked));
            s.push((t.history.into(), Side::History));
            s.push((t.albums.into(), Side::Albums));
            s.push((t.artists.into(), Side::Artists));
        }
        s.push((t.queue.into(), Side::Queue));
        if !playlists.is_empty() {
            s.push((String::new(), Side::Sep));
            // con Spotify conectado hay dos bibliotecas: cada una con su logo
            if self.spotify.is_some() {
                s.push(("YouTube Music".into(), Side::Head(false)));
            }
            for p in playlists {
                if let Some(id) = &p.browse_id {
                    s.push((p.title.clone(), Side::Playlist(id.clone())));
                }
            }
        } else if !self.api.authed() {
            s.push((String::new(), Side::Sep));
            s.push((t.no_account_hint.into(), Side::Hint));
        }
        if self.spotify.is_some() {
            s.push((String::new(), Side::Sep));
            s.push(("Spotify".into(), Side::Head(true)));
            s.push((t.sp_liked.into(), Side::SpLiked));
            s.push((t.albums.into(), Side::SpAlbums));
            for p in &self.sp_playlists {
                if let Some(id) = &p.browse_id {
                    s.push((p.title.clone(), Side::Playlist(id.clone())));
                }
            }
        }
        self.side = s;
    }

    fn side_move(&mut self, d: isize) {
        let n = self.side.len() as isize;
        let mut i = self.side_state.selected().unwrap_or(0) as isize;
        loop {
            i += d.signum();
            if i < 0 || i >= n {
                return;
            }
            if !matches!(self.side[i as usize].1, Side::Sep | Side::Hint | Side::Head(_)) {
                break;
            }
        }
        self.side_state.select(Some(i as usize));
    }

    fn side_activate(&mut self) {
        let Some((title, id)) = self.side_state.selected().and_then(|i| self.side.get(i)).cloned() else {
            return;
        };
        match id {
            Side::Home => self.open(&title, Load::Home, true),
            Side::Liked => self.open(&title, Load::Liked, true),
            Side::History => self.open(&title, Load::History, true),
            Side::Albums => self.open(&title, Load::Albums, true),
            Side::Artists => self.open(&title, Load::Artists, true),
            Side::Playlist(pid) => self.open(&title, Load::Playlist(pid), true),
            Side::SpLiked => self.open(&format!("Spotify › {title}"), Load::SpLiked, true),
            Side::SpAlbums => self.open(&format!("Spotify › {title}"), Load::SpAlbums, true),
            Side::Queue => {
                self.seq += 1;
                self.loading = None;
                self.views.clear();
                let sel = self.qi.unwrap_or(0);
                self.push(View { title, items: Vec::new(), queue: true, radio: false, state: TableState::default().with_selected(Some(sel)) });
            }
            Side::Sep | Side::Hint | Side::Head(_) => return,
        }
        self.focus = Focus::Main;
    }

    // ── vistas ──
    fn open(&mut self, title: &str, load: Load, reset: bool) {
        self.seq += 1;
        let (seq, api, tx, title) = (self.seq, self.api.clone(), self.tx.clone(), title.to_string());
        self.loading = Some(title.clone());
        let radio = matches!(load, Load::Home | Load::Search(_));
        let sp = self.spotify.clone();
        std::thread::spawn(move || {
            let spot = |f: &dyn Fn(&spotify::Spotify) -> Result<Vec<Item>, String>| sp.as_ref().map_or(Ok(Vec::new()), f);
            let res = match load {
                Load::SpLiked => spot(&|s| s.liked()),
                Load::SpAlbums => spot(&|s| s.albums()),
                Load::Playlist(id) if id.starts_with("sp:playlist:") => spot(&|s| s.playlist(&id[12..])),
                Load::Album(id) if id.starts_with("sp:album:") => spot(&|s| s.album(&id[9..])),
                Load::Home => api.home(),
                Load::Liked => api.liked(),
                Load::History => api.history(),
                Load::Albums => api.library_albums(),
                Load::Artists => api.library_artists(),
                Load::Playlist(id) => api.playlist(&id),
                Load::Album(id) => api.album(&id),
                Load::Artist(id) => api.artist(&id),
                Load::Search(q) => api.search(&q),
            };
            let _ = tx.send(Ev::Loaded { seq, title, reset, radio, res });
        });
    }

    fn push(&mut self, v: View) {
        self.views.push(v);
    }

    fn items(&self) -> &[Item] {
        match self.views.last() {
            Some(v) if v.queue => &self.queue,
            Some(v) => &v.items,
            None => &[],
        }
    }

    fn selected(&self) -> Option<(usize, Item)> {
        let i = self.views.last()?.state.selected()?;
        self.items().get(i).map(|it| (i, it.clone()))
    }

    fn table_move(&mut self, d: isize) {
        let n = self.items().len();
        let Some(v) = self.views.last_mut() else { return };
        if n == 0 {
            return;
        }
        let i = v.state.selected().unwrap_or(0) as isize + d;
        v.state.select(Some(i.clamp(0, n as isize - 1) as usize));
    }

    fn activate(&mut self) {
        let Some((i, it)) = self.selected() else { return };
        let queue_view = self.views.last().is_some_and(|v| v.queue);
        if queue_view {
            return self.play_index(i);
        }
        if it.playable() && self.views.last().is_some_and(|v| v.radio) {
            return self.start_radio(it);
        }
        // las de Spotify que aún se buscan en YouTube Music entran también
        let ok = |x: &Item| x.playable() || x.sp_track().is_some();
        if ok(&it) {
            let items: Vec<Item> = self.items().iter().filter(|x| ok(x)).cloned().collect();
            let start = self.items()[..i].iter().filter(|x| ok(x)).count();
            return self.play_from(items, start);
        }
        let Some(id) = it.browse_id.clone() else { return };
        let load = match it.kind {
            Kind::Album => Load::Album(id),
            Kind::Artist => Load::Artist(id),
            Kind::Playlist => Load::Playlist(id),
            _ => return,
        };
        self.open(&it.title, load, false);
    }

    // ── cola y reproducción ──
    fn current(&self) -> Option<&Item> {
        self.qi.and_then(|i| self.queue.get(i))
    }

    fn play_from(&mut self, mut items: Vec<Item>, start: usize) {
        self.radio_pending = None;
        if self.shuffle {
            let first = items.remove(start);
            self.shuffle_vec(&mut items, Some(&first));
            items.insert(0, first);
            self.queue = items;
            self.play_index(0);
        } else {
            self.queue = items;
            self.play_index(start);
        }
    }

    fn play_index(&mut self, i: usize) {
        self.log_play();
        self.sp_wait = None;
        if let Some(it) = self.queue.get(i).filter(|x| x.video_id.is_none()).cloned() {
            let Some(sp) = it.sp_track().map(String::from) else { return };
            self.qi = Some(i);
            if let Some(None) = self.sp_match.get(&sp) {
                // no está en YouTube Music: a la siguiente
                self.notify(fill(self.s().sp_not_found, &[&it.title]), true);
                return if i + 1 < self.queue.len() { self.play_index(i + 1) } else { self.stop() };
            }
            self.stop();
            self.sp_wait = Some(sp);
            return self.match_now(it);
        }
        let Some(vid) = self.queue.get(i).and_then(|x| x.video_id.clone()) else { return };
        let start = self.resume.take().filter(|r| r.0 == self.queue[i].video_id).map(|r| r.1);
        self.qi = Some(i);
        self.song_start = Instant::now();
        self.track_n += 1;
        self.splash = Some(Instant::now());
        self.song_unix = unix_now();
        self.announce(i);
        if let (Some((_, tx)), Some(it)) = (&self.lb, self.queue.get(i)) {
            let _ = tx.send(scrobble::Job::Now(self.track_of(it)));
        }
        // el fundido: la que entra sube desde cero; las demás, sin filtro
        let fade = self.fade_in.take();
        if fade.is_none() {
            self.kill_ghost();
        }
        if fade.is_some() || self.af_on {
            // con etiqueta: el del sonido (@eq) se queda como está
            self.mpv.cmd(json!(["af", "remove", "@fade"]));
            if let Some(x) = fade {
                self.mpv.cmd(json!(["af", "add", format!("@fade:lavfi=[afade=t=in:d={x:.2}]")]));
            }
            self.af_on = fade.is_some();
        }
        self.added = 0;
        self.loaded = true;
        self.pos = None;
        self.dur = None;
        self.waiting = None;
        self.upgrade = None;
        self.local = None;
        // guardada sin conexión: del disco, al instante
        if let Some(p) = self.cached.contains(&vid).then(|| cache::find(&vid)).flatten() {
            cache::touch(&p);
            let title = self.queue[i].title.clone();
            self.direct = true;
            self.local = Some(p.display().to_string());
            self.mpv.load(&vid, self.local.as_deref(), &title, start);
            if self.lyrics_on {
                self.fetch_lyrics();
            }
            return self.fetch_cover();
        }
        match self.streams.get(&vid).filter(|l| l.fresh()).cloned() {
            // lo bastante pronto: mejor esperar un momento que empezar en baja
            Some(link) if link.wait() < Duration::from_millis(1500) => self.start_link(vid, link, start),
            link => {
                // en frío el enlace bueno tarda ~8 s en servir: que empiece ya por
                // mpv (peor calidad) y se cambia al bueno en cuanto se pueda
                self.start_fallback(&vid, start);
                self.upgrade = Some(vid.clone());
                match link {
                    Some(l) => self.upgrade_when_ready(vid, &l),
                    None => self.prefetch(vid),
                }
            }
        }
        if self.lyrics_on {
            self.fetch_lyrics();
        }
        self.fetch_cover();
    }

    /// Pone la actual con su enlace directo, o espera a que sirva.
    fn start_link(&mut self, vid: String, link: stream::Link, start: Option<f64>) {
        let wait = link.wait();
        if wait.is_zero() {
            let title = self.current().map(|x| x.title.clone()).unwrap_or_default();
            self.direct = true;
            return self.mpv.load(&vid, Some(&link.url), &title, start);
        }
        self.mpv.cmd(json!(["stop"]));
        self.waiting = Some((vid.clone(), start));
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(wait);
            let _ = tx.send(Ev::Ready(vid));
        });
    }

    fn upgrade_when_ready(&self, vid: String, link: &stream::Link) {
        let (tx, wait) = (self.tx.clone(), link.wait());
        std::thread::spawn(move || {
            std::thread::sleep(wait);
            let _ = tx.send(Ev::Ready(vid));
        });
    }

    /// Pasa la actual del audio de mpv al enlace bueno, en el mismo punto.
    fn do_upgrade(&mut self, vid: &str) {
        let Some(link) = self.streams.get(vid).filter(|l| l.fresh()).cloned() else { return };
        self.upgrade = None;
        let title = self.current().map(|x| x.title.clone()).unwrap_or_default();
        let paused = self.paused;
        self.direct = true;
        self.mpv.load(vid, Some(&link.url), &title, self.pos);
        if paused {
            self.mpv.cmd(json!(["set_property", "pause", true]));
        }
    }

    /// Sin enlace propio: que lo resuelva mpv (peor calidad, pero suena).
    fn start_fallback(&mut self, vid: &str, start: Option<f64>) {
        let title = self.current().map(|x| x.title.clone()).unwrap_or_default();
        self.direct = false;
        self.mpv.load(vid, None, &title, start);
    }

    // ── Spotify ──
    fn load_spotify(&mut self) {
        let Some(sp) = self.spotify.clone() else { return };
        let tx = self.tx.clone();
        std::thread::spawn(move || match sp.playlists() {
            Ok(p) => drop(tx.send(Ev::SpPlaylists(p))),
            Err(e) => drop(tx.send(Ev::Notify(e, true))),
        });
    }

    /// Tres hilos que buscan en YouTube Music las canciones de Spotify.
    fn start_matchers(&self) {
        for _ in 0..3 {
            let (m, api, tx) = (self.matcher.clone(), self.api.clone(), self.tx.clone());
            std::thread::spawn(move || loop {
                let it = {
                    let (q, cv) = &*m;
                    let mut q = q.lock().unwrap_or_else(|e| e.into_inner());
                    loop {
                        match q.pop_front() {
                            Some(it) => break it,
                            None => q = cv.wait(q).unwrap_or_else(|e| e.into_inner()),
                        }
                    }
                };
                let sp = it.sp_track().unwrap_or("").to_string();
                let vid = spotify::best_match(&api, &it);
                if tx.send(Ev::Matched { sp, vid }).is_err() {
                    break;
                }
            });
        }
    }

    /// Les pone el video_id a las ya conocidas y manda a buscar las demás.
    fn resolve_spotify(&mut self, items: &mut [Item]) {
        let mut todo = Vec::new();
        for it in items.iter_mut() {
            let Some(sp) = it.sp_track() else { continue };
            match self.sp_match.get(sp) {
                Some(v) => it.video_id = v.clone(),
                None => todo.push(it.clone()),
            }
        }
        let (q, cv) = &*self.matcher;
        let mut q = q.lock().unwrap_or_else(|e| e.into_inner());
        for it in todo {
            if self.matching.insert(it.sp_track().unwrap_or("").to_string()) {
                q.push_back(it);
            }
        }
        cv.notify_all();
    }

    /// Esta antes que ninguna (es la que toca sonar).
    fn match_now(&mut self, it: Item) {
        let sp = it.sp_track().unwrap_or("").to_string();
        let (q, cv) = &*self.matcher;
        let mut q = q.lock().unwrap_or_else(|e| e.into_inner());
        q.retain(|x| x.sp_track() != Some(&sp));
        q.push_front(it);
        self.matching.insert(sp);
        cv.notify_all();
    }

    fn matched(&mut self, sp: String, vid: Option<String>) {
        self.matching.remove(&sp);
        self.sp_match.insert(sp.clone(), vid.clone());
        self.sp_dirty = true;
        for v in self.views.iter_mut() {
            for it in v.items.iter_mut().filter(|x| x.sp_track() == Some(&sp)) {
                it.video_id = vid.clone();
            }
        }
        for it in self.queue.iter_mut().filter(|x| x.sp_track() == Some(&sp)) {
            it.video_id = vid.clone();
        }
        if self.sp_wait.as_ref() == Some(&sp) {
            if let Some(q) = self.qi {
                self.play_index(q);
            }
        }
    }

    pub fn open_spotify(&mut self) {
        let input = self.spotify.as_ref().map(|s| s.client_id()).unwrap_or_default();
        self.popup = Some(Popup::Spotify { input, busy: false, err: None });
    }

    fn spotify_key(&mut self, k: KeyEvent) {
        let Some(Popup::Spotify { input, busy, err }) = &mut self.popup else { return };
        if *busy {
            if k.code == KeyCode::Esc {
                self.popup = None; // el navegador puede seguir; da igual
            }
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.popup = None,
            KeyCode::Enter if !input.trim().is_empty() => {
                *busy = true;
                *err = None;
                let (id, tx, file) = (input.trim().to_string(), self.tx.clone(), data_dir().join("spotify.json"));
                std::thread::spawn(move || {
                    let res = spotify::Spotify::login(&id, &file).map(|s| s.user());
                    let _ = tx.send(Ev::SpotifyIn(res));
                });
            }
            // con sesión: Ctrl+D la cierra
            KeyCode::Char('d') if ctrl && self.spotify.is_some() => {
                let _ = std::fs::remove_file(data_dir().join("spotify.json"));
                self.spotify = None;
                self.sp_playlists.clear();
                self.popup = None;
                self.fill_side();
                self.notify(self.s().sp_disconnected, false);
            }
            KeyCode::Char('u') if ctrl => input.clear(),
            KeyCode::Backspace => drop(input.pop()),
            KeyCode::Char(c) if !ctrl => input.push(c),
            _ => {}
        }
    }

    // ── portada ──
    fn fetch_cover(&mut self) {
        let Some(it) = self.current() else { return };
        let (Some(vid), Some(url)) = (it.video_id.clone(), it.cover_url()) else { return };
        // también sin verla, si sus colores tiñen la interfaz
        if !(self.cover_on || self.cfg.cover_colors) || self.cover.as_ref().is_some_and(|(v, _)| *v == vid) {
            return;
        }
        self.cover = Some((vid.clone(), None));
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let Ok(img) = api.get_bytes(&url).and_then(|b| image::load_from_memory(&b).map_err(|e| e.to_string())) else {
                return;
            };
            let img = api::square(img);
            let pal = palette::of(&img);
            let _ = tx.send(Ev::Cover { vid, img, pal });
        });
    }

    /// Lo que se ve en la cola, o lo que está sonando si no se mira la cola.
    /// Hay algo moviéndose en pantalla: redibujar más a menudo.
    fn animating(&self) -> bool {
        self.cfg.motion
            && ((self.loaded && !self.paused) || self.splash.is_some_and(|s| s.elapsed() < SPLASH) || self.tint_at.elapsed() < TINT)
    }

    // ── colores de la portada ──
    /// Empieza el paso a los colores de esta portada (o de vuelta a los del look).
    fn set_tint(&mut self, pal: Option<(palette::Hsl, palette::Hsl)>) {
        self.tint_from = (self.theme.primary, self.theme.accent);
        self.tint_at = Instant::now();
        self.cover_pal = pal;
    }

    /// El tema que toca pintar: el del look con el principal y el acento de la
    /// portada (si se quiere y el look va a color), pasando suave de unos a otros.
    pub fn apply_tint(&mut self) {
        let base = self.theme_base;
        let dark = palette::dark(base.fg);
        let target = match (self.cover_pal, self.cfg.cover_colors, self.cfg.tint) {
            (Some((a, b)), true, config::Tint::Color) => (palette::adapt(a, dark), palette::adapt(b, dark)),
            // fósforo: todo en un color, el de la portada
            (Some((a, _)), true, config::Tint::Phosphor) => (palette::adapt(a, dark), palette::adapt(a, dark)),
            _ => (base.primary, base.accent),
        };
        let k = if self.cfg.motion { self.tint_at.elapsed().as_secs_f64() / TINT.as_secs_f64() } else { 1.0 };
        let k = k.clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k);
        let blend = |a: Color, b: Color| match (a, b) {
            (Color::Rgb(..), Color::Rgb(..)) if k < 1.0 => ui::mix(a, b, k),
            _ => b,
        };
        self.theme = theme::Theme { primary: blend(self.tint_from.0, target.0), accent: blend(self.tint_from.1, target.1), ..base };
    }

    pub fn queue_view(&self) -> bool {
        self.focus == Focus::Main && self.views.last().is_some_and(|v| v.queue)
    }

    fn queue_remove(&mut self) {
        let Some((i, it)) = self.selected() else { return };
        self.queue.remove(i);
        match self.qi {
            Some(q) if i < q => self.qi = Some(q - 1),
            Some(q) if i == q => {
                self.resume = None;
                if i >= self.queue.len() {
                    self.qi = None;
                    self.stop();
                } else if self.loaded && !self.paused {
                    self.play_index(i);
                } else {
                    self.stop();
                    self.qi = Some(i);
                    self.dur = parse_duration(&self.queue[i].duration);
                }
            }
            Some(q) if i <= q + self.added => self.added -= 1,
            _ => {}
        }
        let n = self.queue.len();
        if let Some(v) = self.views.last_mut() {
            v.state.select(Some(i.min(n.saturating_sub(1))));
        }
        self.notify(fill(self.s().removed, &[&it.title]), false);
    }

    fn queue_move(&mut self, d: isize) {
        let Some((i, _)) = self.selected() else { return };
        let j = i as isize + d;
        if j < 0 || j as usize >= self.queue.len() {
            return;
        }
        let j = j as usize;
        self.queue.swap(i, j);
        if self.qi == Some(i) {
            self.qi = Some(j);
            self.added = 0;
        } else if self.qi == Some(j) {
            self.qi = Some(i);
            self.added = 0;
        }
        if let Some(v) = self.views.last_mut() {
            v.state.select(Some(j));
        }
    }

    fn stop(&mut self) {
        self.log_play();
        if self.loaded {
            self.mpv.cmd(json!(["stop"]));
        }
        self.loaded = false;
        self.waiting = None;
        self.pos = None;
        self.dur = None;
    }

    // ── letra ──
    fn fetch_lyrics(&mut self) {
        let Some(it) = self.current().cloned() else { return };
        let Some(vid) = it.video_id.clone() else { return };
        if self.lyrics.as_ref().is_some_and(|(v, _)| *v == vid) {
            return;
        }
        self.lyrics = Some((vid.clone(), Lyrics::Loading));
        self.lyrics_scroll = 0;
        self.lyrics_hand = None;
        self.lyrics_auto.set(0);
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || {
            // con tiempos si LRCLIB la tiene; si no, la de YouTube Music
            let lyrics = match lrclib::synced(&it) {
                Some(l) => Lyrics::Synced(l, "LRCLIB".into()),
                None => match api.lyrics(&vid) {
                    Ok(Some((text, source))) => Lyrics::Text(text, source),
                    Ok(None) => Lyrics::Missing,
                    Err(e) => Lyrics::Err(e),
                },
            };
            let _ = tx.send(Ev::Lyrics { vid, lyrics });
        });
    }

    fn lyrics_scroll(&mut self, d: isize) {
        let n = match &self.lyrics {
            Some((_, Lyrics::Text(t, _))) => t.lines().count(),
            Some((_, Lyrics::Synced(..))) => self.lyrics_total.get() as usize,
            _ => 0,
        };
        // desde donde iba siguiendo a la canción
        if self.following_lyrics() {
            self.lyrics_scroll = self.lyrics_auto.get();
        }
        self.lyrics_hand = Some(Instant::now());
        let s = (self.lyrics_scroll as isize + d).clamp(0, n.saturating_sub(1) as isize);
        self.lyrics_scroll = s as u16;
    }

    /// La letra sincronizada va sola con la canción, salvo un rato después
    /// de moverla a mano.
    pub fn following_lyrics(&self) -> bool {
        matches!(self.lyrics, Some((_, Lyrics::Synced(..)))) && self.lyrics_hand.is_none_or(|t| t.elapsed() > LYRICS_HAND)
    }

    // ── añadir a playlist ──
    /// Las de la biblioteca menos las automáticas (me gusta, episodios).
    pub fn own_playlists(&self) -> Vec<Item> {
        self.playlists
            .iter()
            .filter(|p| !matches!(p.browse_id.as_deref(), Some("VLLM" | "VLSE") | None))
            .cloned()
            .collect()
    }

    fn open_add_to(&mut self) {
        let Some(item) = self.target() else { return };
        if !self.api.authed() {
            return self.notify(self.s().need_account, true);
        }
        if self.own_playlists().is_empty() {
            return self.notify(self.s().no_playlists, true);
        }
        self.popup = Some(Popup::AddTo { item, state: ListState::default().with_selected(Some(0)) });
    }

    fn popup_key(&mut self, k: KeyEvent) {
        let n = self.own_playlists().len();
        let Some(popup) = &mut self.popup else { return };
        let Popup::AddTo { item, state } = popup else {
            if matches!(popup, Popup::Settings { .. }) {
                return self.settings_key(k);
            }
            if matches!(popup, Popup::Login { .. }) {
                return self.login_key(k);
            }
            if matches!(popup, Popup::Spotify { .. }) {
                return self.spotify_key(k);
            }
            if matches!(popup, Popup::Wrapped { .. }) {
                return self.wrapped_key(k);
            }
            if matches!(popup, Popup::Scrobble { .. }) {
                return self.scrobble_key(k);
            }
            if matches!(popup, Popup::Party) {
                return self.party_popup_key(k);
            }
            if matches!(popup, Popup::Discord { .. }) {
                return self.discord_key(k);
            }
            if matches!(popup, Popup::Info { .. }) {
                return self.info_key(k);
            }
            self.popup = None; // la ayuda se cierra con cualquier tecla
            return;
        };
        let i = state.selected().unwrap_or(0);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.popup = None,
            KeyCode::Char('j') | KeyCode::Down => state.select(Some((i + 1).min(n.saturating_sub(1)))),
            KeyCode::Char('k') | KeyCode::Up => state.select(Some(i.saturating_sub(1))),
            KeyCode::Char('g') | KeyCode::Home => state.select(Some(0)),
            KeyCode::Char('G') | KeyCode::End => state.select(Some(n.saturating_sub(1))),
            KeyCode::Enter => {
                let item = item.clone();
                self.popup = None;
                let Some(pl) = self.own_playlists().into_iter().nth(i) else { return };
                let (api, tx) = (self.api.clone(), self.tx.clone());
                std::thread::spawn(move || {
                    let res = api.add_to_playlist(pl.browse_id.as_deref().unwrap_or(""), item.video_id.as_deref().unwrap_or(""));
                    let ev = match res {
                        Ok(()) => Ev::Notify(format!("{} → {}", item.title, pl.title), false),
                        Err(e) => Ev::Notify(format!("{}: {e}", pl.title), true),
                    };
                    let _ = tx.send(ev);
                });
            }
            _ => {}
        }
    }

    fn advance(&mut self, auto: bool) {
        let Some(qi) = self.qi else { return };
        // «al acabar esta»: se queda lista la siguiente, sin sonar
        if auto && self.sleep == Sleep::EndOfSong {
            self.log_play();
            self.sleep = Sleep::Off;
            self.notify(self.s().sleep_done, false);
            if qi + 1 < self.queue.len() {
                self.qi = Some(qi + 1);
            }
            return self.stop();
        }
        if auto && self.repeat == Repeat::One {
            return self.play_index(qi);
        }
        if qi + 1 < self.queue.len() {
            return self.play_index(qi + 1);
        }
        if self.repeat == Repeat::All {
            return self.play_index(0);
        }
        if self.autoplay {
            match &mut self.radio_pending {
                Some((_, play)) => *play = true, // ya viene: que suene al llegar
                None => {
                    if let Some(seed) = self.current().cloned() {
                        self.radio(&seed, true);
                    }
                }
            }
        }
    }

    /// La canción y detrás su radio, que se va alargando sola.
    fn start_radio(&mut self, it: Item) {
        self.queue = vec![it.clone()];
        self.play_index(0);
        self.radio(&it, false);
        self.notify(fill(self.s().radio_of, &[&it.title]), false);
    }

    fn radio(&mut self, seed: &Item, play: bool) {
        let Some(vid) = seed.video_id.clone() else { return };
        self.radio_pending = Some((vid.clone(), play));
        self.radio_seed = Some(vid.clone());
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let res = api.radio(&vid);
            let _ = tx.send(Ev::Radio { seed: vid, res });
        });
    }

    /// Con autoplay, pide más radio cuando quedan un par de canciones, para
    /// que no se corte ni haya que esperar al acabar la cola.
    fn top_up_radio(&mut self) {
        let Some(q) = self.qi else { return };
        if !self.autoplay || self.repeat != Repeat::Off || self.radio_pending.is_some() || q + 3 < self.queue.len() {
            return;
        }
        // si con esa semilla ya no salió nada nuevo, no insistir
        if let Some(seed) = self.queue.last().cloned().filter(|x| x.video_id != self.radio_seed) {
            self.radio(&seed, false);
        }
    }

    // ── eventos ──
    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Key(k) => self.key(k),
            Ev::Mouse(m) => self.mouse(m),
            Ev::Media(m) => self.media(m),
            Ev::Resize => {}
            Ev::Prop(name, data) => match name.as_str() {
                // sin nada cargado mpv dice null y se perdería la posición restaurada
                "time-pos" if self.loaded => {
                    // lo oído: avanzar poco a poco cuenta, un salto no
                    if let (Some(old), Some(new)) = (self.pos, data.as_f64()) {
                        if (0.0..2.5).contains(&(new - old)) {
                            self.listened += new - old;
                        }
                    }
                    self.pos = data.as_f64();
                    self.maybe_crossfade();
                }
                "duration" if self.loaded => self.dur = data.as_f64(),
                "pause" => {
                    self.paused = data.as_bool().unwrap_or(true);
                    // en pausa, tampoco el final de la anterior
                    if self.paused {
                        self.kill_ghost();
                    }
                }
                // la bajada del temporizador no es tu volumen
                "volume" if !self.fading() => {
                    if let Some(v) = data.as_f64() {
                        self.volume = v.round() as i64;
                    }
                }
                _ => {}
            },
            Ev::End(error) => {
                // el enlace directo pudo caducar o dar 403: uno nuevo y, si
                // vuelve a fallar, que lo resuelva mpv
                // lo de mpv falló pero el enlace bueno viene de camino: esperarlo
                if error && !self.direct {
                    if let Some(vid) = self.upgrade.take().filter(|v| self.current().and_then(|x| x.video_id.as_ref()) == Some(v)) {
                        let start = self.pos;
                        return match self.streams.get(&vid).cloned() {
                            Some(l) => self.start_link(vid, l, start),
                            None => {
                                self.waiting = Some((vid.clone(), start));
                                self.prefetch(vid);
                            }
                        };
                    }
                }
                if error && self.local.is_some() {
                    if let (Some(q), Some(vid)) = (self.qi, self.current().and_then(|x| x.video_id.clone())) {
                        if let Some(p) = self.local.take() {
                            let _ = std::fs::remove_file(p);
                        }
                        self.cached.remove(&vid);
                        return self.play_index(q);
                    }
                }
                if error && self.direct {
                    if let (Some(q), Some(vid)) = (self.qi, self.current().and_then(|x| x.video_id.clone())) {
                        self.streams.remove(&vid);
                        if self.retried.as_ref() != Some(&vid) {
                            self.retried = Some(vid);
                            return self.play_index(q);
                        }
                        return self.start_fallback(&vid, None);
                    }
                }
                if error {
                    self.errors += 1;
                    let t = self.current().map(|x| x.title.clone()).unwrap_or_default();
                    self.notify(fill(self.s().play_failed, &[&t]), true);
                    if self.errors >= 3 {
                        self.errors = 0;
                        return self.notify(self.s().many_errors, true);
                    }
                } else {
                    self.errors = 0;
                }
                self.advance(true);
            }
            Ev::Loaded { seq, title, reset, radio, res } => {
                if seq != self.seq {
                    return;
                }
                self.loading = None;
                match res {
                    Ok(items) => {
                        if reset {
                            self.views.clear();
                        }
                        let mut items = items;
                        self.resolve_spotify(&mut items);
                        let state = TableState::default().with_selected(Some(0));
                        self.push(View { title, items, queue: false, radio, state });
                    }
                    Err(e) => {
                        // al arrancar, sin nada en pantalla: un tropiezo de YouTube
                        // no debería dejarlo vacío, se reintenta una vez
                        if reset && self.views.is_empty() && !self.home_retried {
                            self.home_retried = true;
                            let tx = self.tx.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_secs(2));
                                let _ = tx.send(Ev::RetryHome);
                            });
                        }
                        self.notify(format!("{title}: {e}"), true)
                    }
                }
            }
            Ev::Info { vid, res } => {
                if let Some(Popup::Info { item, info, err, .. }) = &mut self.popup {
                    if item.video_id.as_deref() == Some(vid.as_str()) {
                        match res {
                            Ok(i) => *info = Some(Box::new(i)),
                            Err(e) => *err = Some(e),
                        }
                    }
                }
            }
            Ev::Cached { vid, gone } => {
                self.cached.insert(vid);
                for g in gone {
                    self.cached.remove(&g);
                }
            }
            Ev::RetryHome => {
                if self.views.is_empty() && self.loading.is_none() {
                    self.open(self.s().home, Load::Home, true);
                }
            }
            Ev::Playlists(p) => {
                self.playlists = p;
                self.fill_side();
            }
            Ev::SpPlaylists(p) => {
                self.sp_playlists = p;
                self.fill_side();
            }
            Ev::SpotifyIn(res) => match res {
                Ok(user) => {
                    self.spotify = spotify::Spotify::load(&data_dir().join("spotify.json"));
                    self.popup = None;
                    self.notify(fill(self.s().sp_connected, &[&user]), false);
                    self.load_spotify();
                }
                Err(e) => {
                    if let Some(Popup::Spotify { busy, err, .. }) = &mut self.popup {
                        *busy = false;
                        *err = Some(e);
                    }
                }
            },
            Ev::ScrobbleIn(res) => match res {
                Ok((token, user)) => {
                    if let Err(e) = scrobble::save(&data_dir().join("listenbrainz.json"), &token, &user) {
                        self.notify(e, true);
                    }
                    self.notify(fill(self.s().lb_connected, &[&user]), false);
                    self.lb = Some((user, scrobble::start(token)));
                    self.popup = None;
                }
                Err(e) => {
                    if let Some(Popup::Scrobble { busy, err, .. }) = &mut self.popup {
                        *busy = false;
                        *err = Some(e);
                    }
                }
            },
            Ev::Party(req) => self.party_req(req),
            Ev::Matched { sp, vid } => self.matched(sp, vid),
            Ev::Cover { vid, img, pal } => {
                if let Some((v, c)) = &mut self.cover {
                    if *v == vid {
                        *c = Some(self.picker.new_resize_protocol(img));
                    }
                }
                if self.current().and_then(|x| x.video_id.as_ref()) == Some(&vid) {
                    self.set_tint(pal);
                }
            }
            Ev::Lyrics { vid, lyrics } => {
                if let Some((v, l)) = &mut self.lyrics {
                    if *v == vid {
                        *l = lyrics;
                    }
                }
            }
            Ev::Radio { seed, res } => {
                // de una cola que ya no está (se puso otra cosa mientras)
                let Some((_, play)) = self.radio_pending.take_if(|(s, _)| *s == seed) else { return };
                let items = match res {
                    Ok(items) => items,
                    Err(e) => return self.notify(format!("radio: {e}"), true),
                };
                let start = self.queue.len();
                // otra versión de algo que ya está (su videoclip, por ejemplo) tampoco
                for x in items {
                    if !self.queue.iter().any(|q| q.video_id == x.video_id || q.same_song(&x)) {
                        self.queue.push(x);
                    }
                }
                if play && self.queue.len() > start {
                    self.play_index(start);
                }
            }
            Ev::Notify(m, err) => self.notify(m, err),
            Ev::Stream { vid, format, link } => {
                self.fetching.remove(&vid);
                let link = link.filter(|_| format == self.cfg.quality.ytdl_format());
                if let Some(l) = &link {
                    self.streams.insert(vid.clone(), l.clone());
                }
                if let Some((w, start)) = self.waiting.take_if(|(w, _)| *w == vid) {
                    match link {
                        Some(l) => self.start_link(w, l, start),
                        // yt-dlp falló aquí: que lo intente mpv por su cuenta
                        None => self.start_fallback(&w, start),
                    }
                } else if let (Some(l), true) = (&link, self.upgrade.as_ref() == Some(&vid)) {
                    self.upgrade_when_ready(vid, l);
                }
            }
            Ev::LoggedIn(res) => match res {
                Ok(name) => {
                    self.api.reload();
                    self.popup = None;
                    self.login_skipped = false;
                    self.notify(fill(self.s().login_done, &[&name]), false);
                    self.load_account();
                }
                Err(e) => {
                    if let Some(Popup::Login { busy, err, .. }) = &mut self.popup {
                        *busy = None;
                        *err = Some(e);
                    }
                }
            },
            Ev::Ready(vid) if self.upgrade.as_ref() == Some(&vid) => self.do_upgrade(&vid),
            Ev::Ready(vid) => {
                if let Some((w, start)) = self.waiting.take_if(|(w, _)| *w == vid) {
                    match self.streams.get(&w).cloned() {
                        Some(l) => self.start_link(w, l, start),
                        None => self.start_fallback(&w, start),
                    }
                }
            }
            Ev::Look => {
                self.rounded = hypr::rounding().is_none_or(|r| r > 0);
                self.check_files();
            }
        }
    }

    fn key(&mut self, k: KeyEvent) {
        if k.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.popup.is_some() {
            return self.popup_key(k);
        }
        if self.focus == Focus::Search {
            match k.code {
                KeyCode::Enter => {
                    let q = self.input.trim().to_string();
                    if !q.is_empty() {
                        self.open(&format!("«{q}»"), Load::Search(q), true);
                        self.focus = Focus::Main;
                    }
                }
                KeyCode::Esc | KeyCode::Tab => self.focus = Focus::Main,
                KeyCode::Backspace => drop(self.input.pop()),
                KeyCode::Char('u') if ctrl => self.input.clear(),
                KeyCode::Char('w') if ctrl => {
                    let t = self.input.trim_end().len();
                    let cut = self.input[..t].rfind(' ').map(|i| i + 1).unwrap_or(0);
                    self.input.truncate(cut);
                }
                KeyCode::Char(c) if !ctrl => self.input.push(c),
                _ => {}
            }
            return;
        }

        let page = 10;
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('/') => self.focus = Focus::Search,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Side => Focus::Main,
                    Focus::Main if self.lyrics_on => Focus::Lyrics,
                    _ => Focus::Side,
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = if self.focus == Focus::Lyrics { Focus::Main } else { Focus::Side }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.focus = if self.focus == Focus::Main && self.lyrics_on { Focus::Lyrics } else { Focus::Main }
            }
            KeyCode::Char('?') => self.popup = Some(Popup::Help),
            KeyCode::Char('o') | KeyCode::Char(',') => self.popup = Some(Popup::Settings { sel: 0 }),
            KeyCode::Char('y') => {
                self.lyrics_on = !self.lyrics_on;
                if self.lyrics_on {
                    self.fetch_lyrics();
                } else if self.focus == Focus::Lyrics {
                    self.focus = Focus::Main;
                }
            }
            KeyCode::Char('P') => self.open_add_to(),
            KeyCode::Char('c') => {
                self.cover_on = !self.cover_on;
                self.fetch_cover();
            }
            KeyCode::Char('d') | KeyCode::Delete if !ctrl && self.queue_view() => self.queue_remove(),
            KeyCode::Char('J') if self.queue_view() => self.queue_move(1),
            KeyCode::Char('K') if self.queue_view() => self.queue_move(-1),
            KeyCode::Char('j') | KeyCode::Down => self.nav(1),
            KeyCode::Char('k') | KeyCode::Up => self.nav(-1),
            KeyCode::PageDown => self.nav(page),
            KeyCode::PageUp => self.nav(-page),
            KeyCode::Char('d') if ctrl => self.nav(page),
            KeyCode::Char('u') if ctrl => self.nav(-page),
            KeyCode::Char('g') | KeyCode::Home => self.nav(-100_000),
            KeyCode::Char('G') | KeyCode::End => self.nav(100_000),
            KeyCode::Enter => match self.focus {
                Focus::Side => self.side_activate(),
                Focus::Main => self.activate(),
                _ => {}
            },
            KeyCode::Backspace | KeyCode::Esc => {
                if self.views.len() > 1 {
                    self.views.pop();
                    self.seq += 1;
                    self.loading = None;
                }
            }
            KeyCode::Char(' ') => {
                if self.qi.is_none() {
                    self.activate();
                } else {
                    self.play_pause();
                }
            }
            KeyCode::Char('n') => self.advance(false),
            KeyCode::Char('p') => self.previous(),
            KeyCode::Char('<') => self.mpv.cmd(json!(["seek", -self.cfg.seek, "relative"])),
            KeyCode::Char('>') => self.mpv.cmd(json!(["seek", self.cfg.seek, "relative"])),
            KeyCode::Char('+') | KeyCode::Char('=') => self.set_volume(self.cfg.vol_step),
            KeyCode::Char('-') => self.set_volume(-self.cfg.vol_step),
            KeyCode::Char('s') => self.set_shuffle(!self.shuffle),
            KeyCode::Char('r') => {
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                }
            }
            KeyCode::Char('a') => self.toggle_autoplay(),
            KeyCode::Char('z') => self.enqueue(),
            KeyCode::Char('R') => {
                if let Some((_, it)) = self.selected().filter(|(_, it)| it.playable()) {
                    self.start_radio(it);
                }
            }
            KeyCode::Char('L') => self.like(),
            KeyCode::Char('t') => self.cycle_sleep(),
            KeyCode::Char('e') => self.cycle_sound(),
            KeyCode::Char('Y') => self.copy_link(),
            KeyCode::Char('W') => self.open_wrapped(),
            KeyCode::Char('i') => self.open_info(),
            KeyCode::Char('F') => self.party_key(),
            _ => {}
        }
    }

    /// Pausa o sigue; con la sesión restaurada sin cargar, la arranca.
    fn play_pause(&mut self) {
        match self.qi {
            None => {}
            Some(q) if !self.loaded => self.play_index(q),
            Some(_) => self.mpv.cmd(json!(["cycle", "pause"])),
        }
    }

    fn set_paused(&mut self, p: bool) {
        if self.loaded {
            self.mpv.cmd(json!(["set_property", "pause", p]));
        } else if !p {
            self.play_pause();
        }
    }

    /// Al principio de la canción si ya lleva un rato; si no, la anterior.
    fn previous(&mut self) {
        let qi = self.qi.unwrap_or(0);
        if !self.loaded {
            self.resume = None;
        }
        if (self.pos.unwrap_or(0.0) > 3.0 || qi == 0) && self.loaded {
            self.mpv.cmd(json!(["seek", 0, "absolute"]));
        } else if self.pos.unwrap_or(0.0) > 3.0 || qi == 0 {
            self.play_index(qi);
        } else {
            self.play_index(qi - 1);
        }
    }

    fn set_shuffle(&mut self, on: bool) {
        self.shuffle = on;
        if let (true, Some(qi)) = (self.shuffle, self.qi) {
            let mut rest = self.queue.split_off(qi + 1);
            let cur = self.queue.get(qi).cloned();
            self.shuffle_vec(&mut rest, cur.as_ref());
            self.queue.extend(rest);
            self.added = 0;
        }
    }

    /// El aviso del escritorio de la canción que empieza (si se quiere).
    fn announce(&mut self, i: usize) {
        if self.cfg.desktop == config::Desktop::Off {
            return;
        }
        let Some(it) = self.queue.get(i) else { return };
        if self.desktop.is_none() {
            self.desktop = notif::start(self.api.clone());
        }
        let song = notif::Song {
            title: it.title.clone(),
            artist: [it.artist.as_str(), it.album.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
            cover: it.cover_url(),
            away_only: self.cfg.desktop == config::Desktop::Away,
        };
        if let Some(tx) = &self.desktop {
            let _ = tx.send(song);
        }
    }

    // ── lo escuchado ──
    /// Apunta la actual si se oyó lo bastante, y empieza la cuenta de nuevo.
    /// Lo ya emparejado de Spotify con YouTube Music, para no volver a buscarlo.
    fn save_matches(&mut self) {
        if !self.sp_dirty {
            return;
        }
        let file = data_dir().join("spotify-match.json");
        let tmp = file.with_extension("json.tmp");
        // por un temporal: un corte a medias no deja el archivo roto
        if std::fs::write(&tmp, serde_json::to_string(&self.sp_match).unwrap_or_default()).and_then(|_| std::fs::rename(&tmp, &file)).is_ok() {
            self.sp_dirty = false;
        }
        self.sp_saved = Instant::now();
    }

    fn log_play(&mut self) {
        let secs = std::mem::take(&mut self.listened);
        let Some(it) = self.current() else { return };
        if secs >= stats::MIN_SECS {
            let play = stats::Play { t: unix_now(), title: it.title.clone(), artist: it.artist.clone(), secs };
            stats::log(&data_dir().join("history.jsonl"), &play, it.video_id.as_deref().unwrap_or(""));
            if let Some(vid) = it.video_id.clone() {
                let n = self.plays.entry(vid.clone()).or_default();
                *n += 1;
                if *n >= cache::PLAYS {
                    self.keep_offline(vid);
                }
            }
        }
        self.scrobble_end(secs);
    }

    fn scrobble_end(&mut self, secs: f64) {
        let Some(it) = self.current() else { return };
        if let Some((_, tx)) = &self.lb {
            if scrobble::counts(secs, self.dur) {
                let _ = tx.send(scrobble::Job::Listen(self.track_of(it), self.song_unix));
            }
        }
    }

    fn track_of(&self, it: &Item) -> scrobble::Track {
        scrobble::Track {
            artist: it.artist.clone(),
            title: it.title.clone(),
            album: it.album.clone(),
            secs: self.dur.or_else(|| parse_duration(&it.duration)),
            url: it.link(),
        }
    }

    fn scrobble_key(&mut self, k: KeyEvent) {
        let Some(Popup::Scrobble { input, busy, err }) = &mut self.popup else { return };
        if *busy {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.popup = None,
            KeyCode::Enter if !input.trim().is_empty() => {
                *busy = true;
                *err = None;
                let (token, tx) = (input.trim().to_string(), self.tx.clone());
                std::thread::spawn(move || {
                    let res = scrobble::validate(&token).map(|user| (token, user));
                    let _ = tx.send(Ev::ScrobbleIn(res));
                });
            }
            KeyCode::Char('d') if ctrl && self.lb.is_some() => {
                let _ = std::fs::remove_file(data_dir().join("listenbrainz.json"));
                self.lb = None;
                self.popup = None;
                self.notify(self.s().lb_disconnected, false);
            }
            KeyCode::Char('u') if ctrl => input.clear(),
            KeyCode::Backspace => drop(input.pop()),
            KeyCode::Char(c) if !ctrl => input.push(c),
            _ => {}
        }
    }

    fn open_wrapped(&mut self) {
        let plays = stats::load(&data_dir().join("history.jsonl"));
        self.popup = Some(Popup::Wrapped { period: 0, plays });
    }

    fn wrapped_key(&mut self, k: KeyEvent) {
        let Some(Popup::Wrapped { period, .. }) = &mut self.popup else { return };
        match k.code {
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => *period = (*period + 1) % 3,
            KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => *period = (*period + 2) % 3,
            _ => self.popup = None,
        }
    }

    // ── ficha de la canción ──
    fn open_info(&mut self) {
        let Some(item) = self.target() else { return };
        let Some(vid) = item.video_id.clone() else { return };
        self.popup = Some(Popup::Info { item, info: None, err: None, state: ListState::default().with_selected(Some(0)) });
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let res = api.song_info(&vid);
            let _ = tx.send(Ev::Info { vid, res });
        });
    }

    /// Lo que se puede elegir en la ficha, en orden: 5 del artista, 5
    /// parecidas, 5 artistas.
    pub fn info_rows(info: &api::SongInfo) -> Vec<&Item> {
        info.more.iter().take(5).chain(info.also.iter().take(5)).chain(info.similar.iter().take(5)).collect()
    }

    fn info_key(&mut self, k: KeyEvent) {
        let Some(Popup::Info { info, state, .. }) = &mut self.popup else { return };
        let rows: Vec<Item> = info.as_ref().map(|i| App::info_rows(i).into_iter().cloned().collect()).unwrap_or_default();
        let i = state.selected().unwrap_or(0);
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => state.select(Some((i + 1).min(rows.len().saturating_sub(1)))),
            KeyCode::Char('k') | KeyCode::Up => state.select(Some(i.saturating_sub(1))),
            KeyCode::Enter => {
                let Some(it) = rows.get(i).cloned() else { return };
                self.popup = None;
                match (it.kind, it.browse_id.clone()) {
                    (Kind::Artist, Some(id)) => self.open(&it.title, Load::Artist(id), false),
                    _ => self.start_radio(it),
                }
            }
            KeyCode::Char('z') => {
                let Some(it) = rows.get(i).filter(|x| x.playable()).cloned() else { return };
                match self.qi {
                    None => {
                        self.queue = vec![it];
                        self.play_index(0);
                    }
                    Some(qi) => {
                        self.added += 1;
                        let at = (qi + self.added).min(self.queue.len());
                        self.notify(fill(self.s().queued, &[&it.title]), false);
                        self.queue.insert(at, it);
                    }
                }
            }
            _ => self.popup = None,
        }
    }

    // ── sin conexión ──
    /// A la cola de descargas, si se guardan y aún no está.
    fn keep_offline(&mut self, vid: String) {
        if self.cfg.offline_mb == 0 || self.cached.contains(&vid) {
            return;
        }
        let tx = self.dl.get_or_insert_with(|| cache::start(self.tx.clone()));
        let _ = tx.send((vid, self.cfg.quality.ytdl_format().to_string(), self.cfg.offline_mb));
    }

    /// Cambió el tope: lo que sobre, fuera (con 0, todo); con sitio, las más
    /// puestas a la cola de descargas.
    pub fn offline_changed(&mut self) {
        for vid in cache::evict(self.cfg.offline_mb) {
            self.cached.remove(&vid);
        }
        let mut top: Vec<(String, usize)> = self.plays.iter().filter(|(_, n)| **n >= cache::PLAYS).map(|(v, n)| (v.clone(), *n)).collect();
        top.sort_by_key(|x| std::cmp::Reverse(x.1));
        for (vid, _) in top.into_iter().take(300) {
            self.keep_offline(vid);
        }
    }

    // ── barra (waybar) ──
    /// Lo que suena, para `rola --waybar` (solo si cambió; la posición, al segundo).
    fn bar_sync(&mut self) {
        let Some(it) = self.current() else {
            if !self.bar_last.is_null() {
                bar::clear();
                self.bar_last = Value::Null;
            }
            return;
        };
        let lyric = match &self.lyrics {
            Some((_, Lyrics::Synced(lines, _))) => {
                let pos = self.pos.unwrap_or(0.0) + 0.2;
                lines.iter().rev().find(|(at, _)| *at <= pos).map(|(_, l)| l.clone())
            }
            _ => None,
        };
        let sleep = match (self.sleep, self.sleep_left()) {
            (Sleep::Off, _) => None,
            (_, Some(m)) => Some(format!("{m} min")),
            _ => Some(self.s().sleep_end.to_string()),
        };
        let color = match self.theme.primary {
            Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
            _ => None,
        };
        let v = json!({
            "pid": std::process::id(),
            "title": it.title,
            "artist": it.artist,
            "album": it.album,
            "playing": self.playing(),
            "pos": self.pos.map(f64::floor),
            "dur": self.dur.map(f64::floor),
            "lyric": lyric,
            "sleep": sleep,
            "party": self.party.is_some(),
            "color": color,
        });
        if v != self.bar_last {
            bar::write(&v);
            self.bar_last = v;
        }
    }

    // ── Discord ──
    /// Lo que debe verse en Discord, si ha cambiado (canción, pausa, salto).
    fn discord_sync(&mut self) {
        if self.cfg.discord.is_empty() {
            if self.rpc.take().is_some() {
                self.rpc_last = (0, None);
            }
            return;
        }
        let tx = self.rpc.get_or_insert_with(|| discord::start(self.cfg.discord.clone())).clone();
        let want: discord::Presence = match self.current() {
            Some(it) if self.playing() => {
                let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
                let start = now - (self.pos.unwrap_or(0.0) * 1000.0) as i64;
                let s = self.s();
                Some(discord::Song {
                    title: it.title.clone(),
                    artist: it.artist.clone(),
                    album: it.album.clone(),
                    cover: it.cover_url(),
                    link: it.link().map(|u| (if it.is_spotify() { s.dc_sp } else { s.dc_yt }.to_string(), u)),
                    start_ms: start,
                    end_ms: self.dur.map(|d| start + (d * 1000.0) as i64),
                })
            }
            _ => None,
        };
        // el reloj corre solo: solo cuenta si saltó (más de 3 s) o cambió lo demás
        let same = match (&self.rpc_last.1, &want) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                self.rpc_last.0 == self.track_n
                    && (a.start_ms - b.start_ms).abs() < 3000
                    && a.end_ms.is_some() == b.end_ms.is_some()
                    && a.title == b.title
            }
            _ => false,
        };
        if !same {
            let _ = tx.send(want.clone());
            self.rpc_last = (self.track_n, want);
        }
    }

    fn discord_key(&mut self, k: KeyEvent) {
        let bad = self.s().dc_bad;
        let Some(Popup::Discord { input, err }) = &mut self.popup else { return };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.popup = None,
            KeyCode::Enter => {
                let id = input.trim().to_string();
                if !(17..=21).contains(&id.len()) || !id.chars().all(|c| c.is_ascii_digit()) {
                    *err = Some(bad.into());
                    return;
                }
                self.cfg.discord = id;
                self.rpc = None; // con el ID nuevo
                self.rpc_last = (0, None);
                self.popup = None;
                let _ = self.cfg.save();
                self.cfg_mtimes = config::mtimes();
                self.notify(self.s().dc_on, false);
            }
            KeyCode::Char('d') if ctrl && !self.cfg.discord.is_empty() => {
                self.cfg.discord.clear();
                let _ = self.cfg.save();
                self.cfg_mtimes = config::mtimes();
                self.popup = None;
                self.notify(self.s().dc_off, false);
            }
            KeyCode::Char('u') if ctrl => input.clear(),
            KeyCode::Backspace => drop(input.pop()),
            KeyCode::Char(c) if !ctrl => input.push(c),
            _ => {}
        }
    }

    // ── modo fiesta ──
    /// F: la enciende (y enseña el QR); encendida, enseña el QR otra vez.
    fn party_key(&mut self) {
        if self.party.is_none() {
            match party::start(self.api.clone(), self.tx.clone()) {
                Ok(p) => self.party = Some(p),
                Err(e) => return self.notify(e, true),
            }
        }
        self.popup = Some(Popup::Party);
    }

    fn party_popup_key(&mut self, k: KeyEvent) {
        self.popup = None;
        if k.code == KeyCode::Char('F') {
            self.party = None; // al soltarla, el servidor se para
            self.notify(self.s().party_off, false);
        }
    }

    fn party_req(&mut self, req: party::Req) {
        match req {
            party::Req::State(reply) => {
                let hex = |c: Color| match c {
                    Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
                    _ => None,
                };
                let song = |x: &Item| json!({ "title": x.title, "artist": x.artist, "thumb": x.cover_url() });
                let next: Vec<Value> = self.qi.map_or(Vec::new(), |q| self.queue.iter().skip(q + 1).take(10).map(song).collect());
                let _ = reply.send(json!({
                    "now": self.current().map(song),
                    "playing": self.playing(),
                    "next": next,
                    "colors": hex(self.theme.primary).zip(hex(self.theme.accent)).map(|(p, a)| json!({ "primary": p, "accent": a })),
                }));
            }
            party::Req::Add { item, by } => {
                let who = if by.trim().is_empty() { self.s().party_someone.to_string() } else { by };
                self.notify(fill(self.s().party_added, &[&who, &item.title]), false);
                // como z: detrás de la actual y de las ya añadidas
                match self.qi {
                    None => {
                        self.queue = vec![item];
                        self.play_index(0);
                    }
                    Some(qi) => {
                        self.added += 1;
                        let at = (qi + self.added).min(self.queue.len());
                        self.queue.insert(at, item);
                    }
                }
            }
        }
    }

    // ── fundido entre canciones ──
    /// A `crossfade` s del final, si la siguiente ya tiene su enlace listo:
    /// un mpv aparte toca lo que queda de esta apagándose y el de siempre
    /// pone la siguiente subiendo. Sin enlace listo, como siempre (sin hueco
    /// de todas formas, porque arranca al momento).
    fn maybe_crossfade(&mut self) {
        let x = self.cfg.crossfade as f64;
        if x == 0.0 || !self.playing() || !self.direct || self.xfaded == self.track_n {
            return;
        }
        // repetir una o «al acabar esta»: ahí no se pasa a otra
        if self.repeat == Repeat::One || self.sleep == Sleep::EndOfSong {
            return;
        }
        let (Some(pos), Some(dur), Some(qi)) = (self.pos, self.dur, self.qi) else { return };
        if dur - pos > x || dur < x * 3.0 {
            return;
        }
        let Some(next) = self.queue.get(qi + 1).and_then(|n| n.video_id.clone()) else { return };
        if !self.cached.contains(&next) && !self.streams.get(&next).is_some_and(|l| l.fresh() && l.wait().is_zero()) {
            return;
        }
        let Some(cur) = self.current().and_then(|c| c.video_id.clone()) else { return };
        // lo que suena ahora: del disco o su enlace
        let url = match &self.local {
            Some(p) => p.clone(),
            None => match self.streams.get(&cur).filter(|l| l.fresh()) {
                Some(l) => l.url.clone(),
                None => return,
            },
        };
        self.xfaded = self.track_n;
        let left = dur - pos;
        self.kill_ghost();
        self.ghost = mpv::ghost(&url, pos, left, self.volume, self.cfg.sound.filter());
        self.fade_in = Some(left);
        self.play_index(qi + 1);
    }

    fn kill_ghost(&mut self) {
        if let Some(mut g) = self.ghost.take() {
            let _ = g.kill();
            let _ = g.wait();
        }
    }

    // ── sonido ──
    /// El filtro de sonido elegido, en mpv (con etiqueta, sin tocar el fundido).
    pub fn apply_sound(&mut self) {
        self.mpv.cmd(json!(["af", "remove", "@eq"]));
        if let Some(g) = self.cfg.sound.filter() {
            self.mpv.cmd(json!(["af", "add", format!("@eq:lavfi=[{g}]")]));
        }
    }

    /// e: el siguiente sonido.
    fn cycle_sound(&mut self) {
        self.cfg.sound = self.cfg.sound.step(1);
        self.apply_sound();
        let _ = self.cfg.save();
        self.cfg_mtimes = config::mtimes();
        let name = self.s().sounds[config::Sound::ALL.iter().position(|x| *x == self.cfg.sound).unwrap_or(0)];
        self.notify(fill(self.s().sound_now, &[name]), false);
    }

    // ── temporizador ──
    /// t: no → al acabar esta → 15 → 30 → 45 → 60 → 90 min → no.
    fn cycle_sleep(&mut self) {
        const MINS: [u64; 5] = [15, 30, 45, 60, 90];
        let at = |m: u64| Sleep::At(Instant::now() + Duration::from_secs(m * 60));
        self.sleep = match self.sleep {
            Sleep::Off => Sleep::EndOfSong,
            Sleep::EndOfSong => at(MINS[0]),
            // desde lo que queda: pulsar otra vez da el siguiente escalón
            Sleep::At(_) => match MINS.iter().find(|&&m| m > self.sleep_left().unwrap_or(0) + 1) {
                Some(&m) => at(m),
                None => Sleep::Off,
            },
        };
        // si estaba bajando el volumen, vuelve a como estaba
        self.mpv.cmd(json!(["set_property", "volume", self.volume]));
        let s = self.s();
        let msg = match self.sleep {
            Sleep::Off => s.sleep_off.to_string(),
            Sleep::EndOfSong => s.sleep_eos.to_string(),
            Sleep::At(_) => fill(s.sleep_in, &[&self.sleep_left().unwrap_or(0).to_string()]),
        };
        self.notify(msg, false);
    }

    /// Minutos que quedan (redondeando hacia arriba).
    pub fn sleep_left(&self) -> Option<u64> {
        match self.sleep {
            Sleep::At(t) => Some(t.saturating_duration_since(Instant::now()).as_secs().div_ceil(60)),
            _ => None,
        }
    }

    fn fading(&self) -> bool {
        matches!(self.sleep, Sleep::At(t) if t.saturating_duration_since(Instant::now()).as_secs_f64() < SLEEP_FADE)
    }

    /// Los últimos segundos baja el volumen poco a poco, y luego pausa.
    fn sleep_tick(&mut self) {
        let Sleep::At(t) = self.sleep else { return };
        let left = t.saturating_duration_since(Instant::now()).as_secs_f64();
        if left <= 0.0 {
            self.sleep = Sleep::Off;
            self.set_paused(true);
            self.mpv.cmd(json!(["set_property", "volume", self.volume]));
            self.notify(self.s().sleep_done, false);
        } else if left < SLEEP_FADE && self.playing() {
            self.mpv.cmd(json!(["set_property", "volume", self.volume as f64 * left / SLEEP_FADE]));
        }
    }

    // ── MPRIS ──
    fn media(&mut self, m: mpris::Media) {
        use mpris::Media as M;
        match m {
            M::Next => self.advance(false),
            M::Previous => self.previous(),
            M::PlayPause => self.play_pause(),
            M::Play => self.set_paused(false),
            M::Pause | M::Stop => self.set_paused(true),
            M::Seek(us) if self.loaded => self.mpv.cmd(json!(["seek", us as f64 / 1e6, "relative"])),
            M::SetPos(us) if self.loaded => self.mpv.cmd(json!(["seek", us as f64 / 1e6, "absolute"])),
            M::Seek(_) | M::SetPos(_) => {}
            M::Volume(v) => self.set_volume((v * 100.0).round() as i64 - self.volume),
            M::Shuffle(on) => self.set_shuffle(on),
            M::Loop(l) => {
                self.repeat = match l.as_str() {
                    "Track" => Repeat::One,
                    "Playlist" => Repeat::All,
                    _ => Repeat::Off,
                }
            }
            M::Quit => self.quit = true,
        }
    }

    /// Le pasa a MPRIS cómo está todo (solo se avisa de lo que cambió).
    fn mpris_sync(&mut self) {
        let Some(m) = &mut self.mpris else { return };
        let it = self.qi.and_then(|i| self.queue.get(i));
        let n = self.queue.len();
        let st = mpris::State {
            status: match it {
                None => "Stopped",
                Some(_) if self.loaded && !self.paused => "Playing",
                Some(_) => "Paused",
            },
            track: self.track_n,
            title: it.map(|x| x.title.clone()).unwrap_or_default(),
            artist: it.map(|x| x.artist.clone()).unwrap_or_default(),
            album: it.map(|x| x.album.clone()).unwrap_or_default(),
            art: it.and_then(|x| x.cover_url()).unwrap_or_default(),
            length: self.dur.map(|d| (d * 1e6) as i64).unwrap_or(0),
            volume: self.volume as f64 / 100.0,
            shuffle: self.shuffle,
            loop_status: match self.repeat {
                Repeat::Off => "None",
                Repeat::All => "Playlist",
                Repeat::One => "Track",
            },
            can_next: self.qi.is_some_and(|q| q + 1 < n) || self.repeat == Repeat::All || self.autoplay,
            can_prev: self.qi.is_some(),
        };
        m.update(st, self.pos);
    }

    fn nav(&mut self, d: isize) {
        match self.focus {
            Focus::Side => {
                for _ in 0..d.unsigned_abs().min(self.side.len()) {
                    self.side_move(d);
                }
            }
            Focus::Lyrics => self.lyrics_scroll(d),
            _ => self.table_move(d),
        }
    }

    fn set_volume(&mut self, d: i64) {
        self.volume = (self.volume + d).clamp(0, 130);
        self.mpv.cmd(json!(["set_property", "volume", self.volume]));
    }

    fn enqueue(&mut self) {
        let Some((_, it)) = self.selected().filter(|(_, it)| it.playable()) else { return };
        let Some(qi) = self.qi else {
            self.queue = vec![it];
            return self.play_index(0);
        };
        self.added += 1;
        let at = (qi + self.added).min(self.queue.len());
        self.notify(fill(self.s().queued, &[&it.title]), false);
        self.queue.insert(at, it);
    }

    /// Canción seleccionada en la tabla, o la que suena.
    fn target(&self) -> Option<Item> {
        let sel = if self.focus == Focus::Main { self.selected().map(|x| x.1) } else { None };
        sel.filter(Item::playable).or_else(|| self.current().cloned())
    }

    /// Y: el enlace de lo elegido (o de la que suena) al portapapeles.
    fn copy_link(&mut self) {
        let sel = if self.focus == Focus::Main { self.selected().map(|x| x.1) } else { None };
        let Some(it) = sel.or_else(|| self.current().cloned()) else { return };
        match it.link() {
            Some(url) => {
                clipboard(&url);
                self.notify(fill(self.s().copied, &[&it.title]), false);
            }
            None => self.notify(self.s().no_link, true),
        }
    }

    fn like(&mut self) {
        let Some(it) = self.target() else { return };
        if !self.api.authed() {
            return self.notify(self.s().need_account, true);
        }
        let (api, tx, s) = (self.api.clone(), self.tx.clone(), self.s());
        std::thread::spawn(move || {
            let ev = match api.like(it.video_id.as_deref().unwrap_or("")) {
                Ok(()) => Ev::Notify(format!("\u{f02d1}  {}", it.title), false),
                Err(e) => Ev::Notify(fill(s.like, &[&e]), true),
            };
            let _ = tx.send(ev);
        });
    }

    fn toggle_autoplay(&mut self) {
        self.autoplay = !self.autoplay;
        let s = self.s();
        self.notify(if self.autoplay { s.autoplay_on } else { s.autoplay_off }, false);
    }

    fn reload_theme(&mut self) {
        // de los colores de ahora a los nuevos, sin salto (cambio de look incluido)
        self.tint_from = (self.theme.primary, self.theme.accent);
        self.tint_at = Instant::now();
        self.theme = theme::load(&self.cfg);
        self.theme_base = self.theme;
        self.fetch_cover();
        self.theme_mtime = theme::mtime(theme::source(&self.cfg).as_deref());
    }

    /// Cambia a otra config (otro look, o editada a mano) aplicando lo que haga falta.
    fn apply_config(&mut self, new: Config) {
        let old = self.cfg.lang;
        if new.quality != self.cfg.quality {
            self.mpv.cmd(json!(["set_property", "ytdl-format", new.quality.ytdl_format()]));
            self.streams.clear();
        }
        let sound = new.sound != self.cfg.sound;
        self.cfg = new;
        if self.cfg.lang != old {
            self.retitle(old);
        }
        if sound {
            self.apply_sound();
        }
        self.reload_theme();
    }

    /// Tras cambiar de idioma: barra lateral y vistas abiertas desde ella.
    fn retitle(&mut self, old: Lang) {
        let (old, new) = (old.s(), self.s());
        let pairs = [
            (old.home, new.home),
            (old.liked, new.liked),
            (old.history, new.history),
            (old.albums, new.albums),
            (old.artists, new.artists),
            (old.queue, new.queue),
        ];
        for v in &mut self.views {
            if let Some(&(_, n)) = pairs.iter().find(|(o, _)| v.title == *o) {
                v.title = n.into();
            }
        }
        self.fill_side();
    }

    /// Recarga config y tema si sus archivos cambiaron; true si algo cambió.
    fn check_files(&mut self) -> bool {
        let mut changed = false;
        let m = config::mtimes();
        // con los ajustes abiertos manda lo que se está editando
        if m != self.cfg_mtimes && !matches!(self.popup, Some(Popup::Settings { .. })) {
            self.cfg_mtimes = m;
            self.apply_config(Config::load());
            changed = true;
        }
        if theme::mtime(theme::source(&self.cfg).as_deref()) != self.theme_mtime {
            self.reload_theme();
            changed = true;
        }
        changed
    }

    // ── enlaces directos por adelantado ──
    fn prefetch(&mut self, vid: String) {
        if self.streams.get(&vid).is_some_and(|l| l.fresh()) || !self.fetching.insert(vid.clone()) {
            return;
        }
        let (tx, format) = (self.tx.clone(), self.cfg.quality.ytdl_format());
        std::thread::spawn(move || {
            let link = stream::resolve(&vid, format);
            let _ = tx.send(Ev::Stream { vid, format, link });
        });
    }

    /// La siguiente de la cola (o la restaurada, si aún no suena) y la canción
    /// en la que se para el cursor, que es la que seguramente vas a poner.
    fn prefetch_ahead(&mut self) {
        let n = self.queue.len();
        let next = match self.qi {
            Some(q) if !self.loaded => Some(q),
            Some(q) if self.repeat == Repeat::One => Some(q),
            Some(q) if q + 1 < n => Some(q + 1),
            Some(_) if self.repeat == Repeat::All => Some(0),
            _ => None,
        };
        if let Some(vid) = next.and_then(|i| self.queue.get(i)).and_then(|x| x.video_id.clone()) {
            self.prefetch(vid);
        }
        let sel = if self.focus == Focus::Main { self.selected() } else { None };
        let sel = sel.filter(|(_, it)| it.playable()).and_then(|(_, it)| it.video_id);
        if sel.is_some() && sel == self.hover && self.fetching.len() < 3 {
            self.prefetch(sel.clone().unwrap_or_default());
        }
        self.hover = sel;
    }

    /// Cada medio segundo; true si hay que redibujar.
    fn tick(&mut self) -> bool {
        let mut changed = self.check_files();
        self.top_up_radio();
        self.prefetch_ahead();
        self.sleep_tick();
        // a disco poco a poco: si rola muere a la mala, se pierde poco
        if self.sp_dirty && (self.matching.is_empty() || self.sp_saved.elapsed() > Duration::from_secs(5)) {
            self.save_matches();
        }
        if self.saved_at.elapsed() > Duration::from_secs(60) {
            self.save_state();
            self.save_matches();
            self.saved_at = Instant::now();
        }
        // el final de la anterior que ya acabó solo
        if self.ghost.as_mut().is_some_and(|g| g.try_wait().is_ok_and(|e| e.is_some())) {
            self.ghost = None;
        }
        if self.note.as_ref().is_some_and(|n| n.2.elapsed() > Duration::from_secs(self.cfg.notify_secs)) {
            self.note = None;
            changed = true;
        }
        changed
    }
}

/// Al portapapeles: wl-copy, xclip o xsel si están; si no, se le pide a la
/// terminal (OSC 52: kitty, foot, wezterm… también por SSH).
fn clipboard(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let tools: [(&str, &[&str]); 3] = [("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["-ib"])];
    for (cmd, args) in tools {
        let Ok(mut child) = Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().is_ok_and(|s| s.success()) {
            return;
        }
    }
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
}

fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = c.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// El artista principal (el primero de «A, B & C»), para comparar.
fn main_artist(it: &Item) -> String {
    it.artist.split([',', '&']).next().unwrap_or("").trim().to_lowercase()
}

/// Separa las del mismo artista: si una sigue a otra suya, se cambia por la
/// más cercana de otro (que tampoco choque con la de después). Si no hay
/// manera (todo es del mismo), se queda como está.
fn spread(v: &mut [Item], prev: Option<&Item>) {
    for i in 0..v.len() {
        let before = if i == 0 { prev.map(main_artist) } else { Some(main_artist(&v[i - 1])) };
        let Some(before) = before.filter(|b| !b.is_empty()) else { continue };
        if main_artist(&v[i]) != before {
            continue;
        }
        let next = v.get(i + 1).map(main_artist);
        let fits = |x: &Item| {
            let a = main_artist(x);
            a != before && Some(&a) != next.as_ref()
        };
        let pick = (i + 1..v.len()).find(|&j| fits(&v[j])).or_else(|| (i + 1..v.len()).find(|&j| main_artist(&v[j]) != before));
        if let Some(j) = pick {
            v.swap(i, j);
        }
    }
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Cerrar la ventana de la terminal (SIGHUP) o un kill (SIGTERM, SIGINT):
/// que rola salga como con q, guardando sesión y emparejamientos.
static QUIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    QUIT.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn catch_signals() {
    for sig in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT] {
        unsafe { libc::signal(sig, on_signal as *const () as libc::sighandler_t) };
    }
}

/// "3:45" / "1:02:03" → segundos.
fn parse_duration(s: &str) -> Option<f64> {
    s.split(':').try_fold(0.0, |acc, p| p.parse::<f64>().ok().map(|n| acc * 60.0 + n))
}

fn run(rx: Receiver<Ev>, mut app: App, mut term: ratatui::DefaultTerminal) -> std::io::Result<()> {
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let mut last_tick = Instant::now();
    let res = loop {
        let mut urgent = false;
        match rx.recv_timeout(Duration::from_millis(60)) {
            Ok(ev) => {
                urgent |= !matches!(ev, Ev::Prop(..));
                app.handle(ev);
                while let Ok(ev) = rx.try_recv() {
                    urgent |= !matches!(ev, Ev::Prop(..));
                    app.handle(ev);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break Ok(()),
        }
        if app.quit || QUIT.load(std::sync::atomic::Ordering::Relaxed) {
            break Ok(());
        }
        app.mpris_sync();
        app.discord_sync();
        app.bar_sync();
        if last_tick.elapsed() >= Duration::from_millis(500) {
            urgent |= app.tick();
            last_tick = Instant::now();
        }
        // time-pos llega muchas veces por segundo; la barra no necesita tanto
        let every = if app.animating() { 120 } else { 250 };
        if urgent || last_draw.elapsed() >= Duration::from_millis(every) {
            if let Err(e) = term.draw(|f| app.draw(f)) {
                break Err(e);
            }
            last_draw = Instant::now();
        }
    };
    mouse_capture(false);
    ratatui::restore();
    app.log_play();
    app.save_state();
    bar::clear();
    app.save_matches();
    res
}

fn login() {
    let s = Config::load().lang.s();
    let file = data_dir().join("auth.json");
    for b in login::browsers() {
        println!("{}", fill(s.login_cli_try, &[b.name]));
        match login::import(&b.arg).and_then(|v| login::save(&file, &v)) {
            Ok(()) => return login_test(&file),
            Err(e) if e.is_empty() => println!("  {}", fill(s.login_no_session, &[b.name])),
            Err(e) => println!("  {e}"),
        }
    }
    println!("{}", s.login_cli_paste);
    println!("{}", s.login_intro);
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let Some(auth) = api::parse_headers(&raw) else {
        eprintln!("{}", s.login_no_cookie);
        std::process::exit(1);
    };
    if let Err(e) = login::save(&file, &auth) {
        eprintln!("{}", fill(s.login_save_failed, &[&file.display().to_string(), &e]));
        std::process::exit(1);
    }
    login_test(&file);
}

/// Prueba la sesión recién guardada pidiendo las playlists.
fn login_test(file: &std::path::Path) {
    let s = Config::load().lang.s();
    match Api::new(file).library_playlists() {
        Ok(p) => println!("{}", fill(s.login_ok, &[&p.len().to_string(), &file.display().to_string()])),
        Err(e) => println!("{}", fill(s.login_test_failed, &[&file.display().to_string(), &e])),
    }
}

/// Antes se llamaba ytm: la primera vez, sus carpetas pasan a las de rola.
fn migrate() {
    let state = state_file();
    for new in [config::dir(), data_dir(), state.parent().map(PathBuf::from).unwrap_or_default()] {
        let old = new.with_file_name("ytm");
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
}

/// Lo que tiene que estar instalado: sin mpv no suena nada, y sin yt-dlp ni
/// se encuentran los streams ni se leen las cookies del navegador.
fn missing_deps() -> Vec<&'static str> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    ["mpv", "yt-dlp"].into_iter().filter(|p| !std::env::split_paths(&path).any(|d| d.join(p).is_file())).collect()
}

fn main() {
    migrate();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--login") {
        return login();
    }
    if args.iter().any(|a| a == "--waybar") {
        return bar::waybar(&args);
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", Config::load().lang.s().cli_help);
        return;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("rola {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let missing = missing_deps();
    if !missing.is_empty() {
        let msg = Config::load().lang.s().missing_deps;
        let (list, cmd) = (missing.join(", "), missing.join(" "));
        eprintln!("{}", msg.replacen("{}", &list, 1).replace("{}", &cmd));
        std::process::exit(1);
    }
    let (tx, rx) = mpsc::channel();
    let term = ratatui::init();
    // pregunta a la terminal qué protocolo de imágenes habla (kitty, sixel…);
    // tiene que ir antes de que el hilo de teclas empiece a leer stdin
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize((8, 16)));
    let app = match App::new(tx.clone(), picker) {
        Ok(a) => a,
        Err(e) => {
            ratatui::restore();
            eprintln!("rola: {e}");
            std::process::exit(1);
        }
    };
    mouse_capture(app.cfg.mouse);
    hypr::listen(tx.clone());
    std::thread::spawn(move || loop {
        let ev = match event::read() {
            Ok(Event::Key(k)) => Ev::Key(k),
            // moverlo sin más no hace nada; mandarlo solo llenaría la cola
            Ok(Event::Mouse(m)) if m.kind != MouseEventKind::Moved => Ev::Mouse(m),
            Ok(Event::Resize(..)) => Ev::Resize,
            Ok(_) => continue,
            Err(_) => break,
        };
        if tx.send(ev).is_err() {
            break;
        }
    });
    catch_signals();
    if let Err(e) = run(rx, app, term) {
        eprintln!("rola: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_artists() {
        let it = |a: &str| Item { kind: Kind::Song, title: String::new(), artist: a.into(), album: String::new(), duration: String::new(), video_id: None, browse_id: None, thumb: String::new() };
        let mut v: Vec<Item> = ["A", "A, X", "A", "B", "C", "B", "D"].iter().map(|a| it(a)).collect();
        spread(&mut v, Some(&it("A")));
        let names: Vec<String> = v.iter().map(main_artist).collect();
        assert!(names[0] != "a", "{names:?}");
        assert!(names.windows(2).all(|w| w[0] != w[1]), "{names:?}");
        assert_eq!(names.len(), 7);
        // todo del mismo: se queda tal cual
        let mut same: Vec<Item> = ["A", "A"].iter().map(|a| it(a)).collect();
        spread(&mut same, None);
        assert_eq!(same.len(), 2);
    }

    #[test]
    fn base64() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"https://music.youtube.com/watch?v=abc"), "aHR0cHM6Ly9tdXNpYy55b3V0dWJlLmNvbS93YXRjaD92PWFiYw==");
    }
}
