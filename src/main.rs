//! ytm — YouTube Music en la terminal, al estilo spotify-tui.
//!
//!     ytm            abre la app
//!     ytm --login    conecta tu cuenta (biblioteca, playlists, me gusta)

mod api;
mod config;
mod hypr;
mod i18n;
mod mpv;
mod settings;
mod theme;
mod ui;

use api::{Api, Item, Kind};
use config::Config;
use i18n::{fill, Strings};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
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
    Resize,
    Prop(String, Value),
    End(bool),
    Loaded { seq: u64, title: String, reset: bool, res: Result<Vec<Item>, String> },
    Playlists(Vec<Item>),
    Radio { items: Vec<Item>, play: bool },
    Lyrics { vid: String, res: Result<Option<(String, String)>, String> },
    Cover { vid: String, img: image::DynamicImage },
    Notify(String, bool),
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
    Playlist(String),
}

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
    Err(String),
}

pub enum Popup {
    Help,
    Settings { sel: usize },
    AddTo { item: Item, state: ListState },
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
}

struct View {
    title: String,
    items: Vec<Item>,
    queue: bool, // la cola en vivo (items vacío)
    state: TableState,
}

struct App {
    api: Api,
    tx: Sender<Ev>,
    mpv: mpv::Mpv,
    cfg: Config,
    theme: theme::Theme,
    theme_mtime: Option<SystemTime>,
    rounded: bool, // Hyprland redondea las ventanas (para los bordes «como Hyprland»)
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
    resume: Option<(usize, f64)>, // dónde se quedó la sesión anterior
    playlists: Vec<Item>,
    lyrics_on: bool,
    lyrics: Option<(String, Lyrics)>, // video_id → letra
    lyrics_scroll: u16,
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
    errors: u32, // fallos seguidos; con 3 deja de saltar
    note: Option<(String, bool, Instant)>,
    rng: u64,
    quit: bool,
}

fn data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"))
        .join("ytm")
}

fn state_file() -> PathBuf {
    std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"))
        .join("ytm/state.json")
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
            tx,
            mpv,
            theme: theme::load(&cfg.theme, cfg.transparent, &cfg.wallust),
            theme_mtime: theme::mtime(theme::source(&cfg.theme, &cfg.wallust).as_deref()),
            rounded: hypr::rounding().is_none_or(|r| r > 0),
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
            errors: 0,
            note: None,
            rng: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1,
            quit: false,
        };
        app.fill_side(&[]);
        if !app.cfg.restore {
            return Ok(app.start());
        }
        app.queue = st["queue"].as_array().into_iter().flatten().filter_map(Item::from_json).collect();
        if let Some(qi) = st["qi"].as_u64().map(|i| i as usize).filter(|&i| i < app.queue.len()) {
            let pos = st["pos"].as_f64().unwrap_or(0.0);
            app.qi = Some(qi);
            app.resume = Some((qi, pos));
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
        if self.api.authed() {
            let (api, tx) = (self.api.clone(), self.tx.clone());
            std::thread::spawn(move || match api.library_playlists() {
                Ok(p) => drop(tx.send(Ev::Playlists(p))),
                Err(e) => drop(tx.send(Ev::Notify(format!("playlists: {e}"), true))),
            });
        }
        self.open(self.s().home, Load::Home, true);
        self
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
            "queue": self.queue.iter().map(Item::to_json).collect::<Vec<_>>(),
            "qi": self.qi,
            "pos": if self.loaded { self.pos } else { self.resume.map(|r| r.1) },
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

    fn shuffle_vec(&mut self, v: &mut [Item]) {
        for i in (1..v.len()).rev() {
            let j = (self.rand() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }

    // ── barra lateral ──
    fn fill_side(&mut self, playlists: &[Item]) {
        let t = self.s();
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
            for p in playlists {
                if let Some(id) = &p.browse_id {
                    s.push((p.title.clone(), Side::Playlist(id.clone())));
                }
            }
        } else if !self.api.authed() {
            s.push((String::new(), Side::Sep));
            s.push((t.no_account_hint.into(), Side::Hint));
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
            if !matches!(self.side[i as usize].1, Side::Sep | Side::Hint) {
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
            Side::Queue => {
                self.seq += 1;
                self.loading = None;
                self.views.clear();
                let sel = self.qi.unwrap_or(0);
                self.push(View { title, items: Vec::new(), queue: true, state: TableState::default().with_selected(Some(sel)) });
            }
            Side::Sep | Side::Hint => return,
        }
        self.focus = Focus::Main;
    }

    // ── vistas ──
    fn open(&mut self, title: &str, load: Load, reset: bool) {
        self.seq += 1;
        let (seq, api, tx, title) = (self.seq, self.api.clone(), self.tx.clone(), title.to_string());
        self.loading = Some(title.clone());
        std::thread::spawn(move || {
            let res = match load {
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
            let _ = tx.send(Ev::Loaded { seq, title, reset, res });
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
        if it.playable() {
            let items: Vec<Item> = self.items().iter().filter(|x| x.playable()).cloned().collect();
            let start = self.items()[..i].iter().filter(|x| x.playable()).count();
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
        if self.shuffle {
            let first = items.remove(start);
            self.shuffle_vec(&mut items);
            items.insert(0, first);
            self.queue = items;
            self.play_index(0);
        } else {
            self.queue = items;
            self.play_index(start);
        }
    }

    fn play_index(&mut self, i: usize) {
        let Some(vid) = self.queue.get(i).and_then(|x| x.video_id.clone()) else { return };
        let start = self.resume.take().filter(|r| r.0 == i).map(|r| r.1);
        self.qi = Some(i);
        self.added = 0;
        self.loaded = true;
        self.pos = None;
        self.dur = None;
        self.mpv.load(&vid, start);
        if self.lyrics_on {
            self.fetch_lyrics();
        }
        self.fetch_cover();
    }

    // ── portada ──
    fn fetch_cover(&mut self) {
        let Some(it) = self.current() else { return };
        let (Some(vid), Some(url)) = (it.video_id.clone(), it.cover_url()) else { return };
        if !self.cover_on || self.cover.as_ref().is_some_and(|(v, _)| *v == vid) {
            return;
        }
        self.cover = Some((vid.clone(), None));
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let Ok(img) = api.get_bytes(&url).and_then(|b| image::load_from_memory(&b).map_err(|e| e.to_string())) else {
                return;
            };
            // hqdefault es 4:3 con franjas negras alrededor del 16:9
            let img = if img.width() * 3 == img.height() * 4 {
                let h = img.width() * 9 / 16;
                img.crop_imm(0, (img.height() - h) / 2, img.width(), h)
            } else {
                img
            };
            // las miniaturas de video son 16:9; YouTube Music recorta el centro
            let side = img.width().min(img.height());
            let img = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
            let _ = tx.send(Ev::Cover { vid, img });
        });
    }

    /// Lo que se ve en la cola, o lo que está sonando si no se mira la cola.
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
        if self.loaded {
            self.mpv.cmd(json!(["stop"]));
        }
        self.loaded = false;
        self.pos = None;
        self.dur = None;
    }

    // ── letra ──
    fn fetch_lyrics(&mut self) {
        let Some(vid) = self.current().and_then(|x| x.video_id.clone()) else { return };
        if self.lyrics.as_ref().is_some_and(|(v, _)| *v == vid) {
            return;
        }
        self.lyrics = Some((vid.clone(), Lyrics::Loading));
        self.lyrics_scroll = 0;
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let res = api.lyrics(&vid);
            let _ = tx.send(Ev::Lyrics { vid, res });
        });
    }

    fn lyrics_scroll(&mut self, d: isize) {
        let n = match &self.lyrics {
            Some((_, Lyrics::Text(t, _))) => t.lines().count(),
            _ => 0,
        };
        let s = (self.lyrics_scroll as isize + d).clamp(0, n.saturating_sub(1) as isize);
        self.lyrics_scroll = s as u16;
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
            if let Some(seed) = self.current().cloned() {
                self.radio(&seed, true);
            }
        }
    }

    fn radio(&mut self, seed: &Item, play: bool) {
        let Some(vid) = seed.video_id.clone() else { return };
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || match api.radio(&vid) {
            Ok(items) => drop(tx.send(Ev::Radio { items, play })),
            Err(e) => drop(tx.send(Ev::Notify(format!("radio: {e}"), true))),
        });
    }

    // ── eventos ──
    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Key(k) => self.key(k),
            Ev::Resize => {}
            Ev::Prop(name, data) => match name.as_str() {
                // sin nada cargado mpv dice null y se perdería la posición restaurada
                "time-pos" if self.loaded => self.pos = data.as_f64(),
                "duration" if self.loaded => self.dur = data.as_f64(),
                "pause" => self.paused = data.as_bool().unwrap_or(true),
                "volume" => {
                    if let Some(v) = data.as_f64() {
                        self.volume = v.round() as i64;
                    }
                }
                _ => {}
            },
            Ev::End(error) => {
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
            Ev::Loaded { seq, title, reset, res } => {
                if seq != self.seq {
                    return;
                }
                self.loading = None;
                match res {
                    Ok(items) => {
                        if reset {
                            self.views.clear();
                        }
                        let state = TableState::default().with_selected(Some(0));
                        self.push(View { title, items, queue: false, state });
                    }
                    Err(e) => self.notify(format!("{title}: {e}"), true),
                }
            }
            Ev::Playlists(p) => {
                self.fill_side(&p);
                self.playlists = p;
            }
            Ev::Cover { vid, img } => {
                if let Some((v, c)) = &mut self.cover {
                    if *v == vid {
                        *c = Some(self.picker.new_resize_protocol(img));
                    }
                }
            }
            Ev::Lyrics { vid, res } => {
                if let Some((v, l)) = &mut self.lyrics {
                    if *v == vid {
                        *l = match res {
                            Ok(Some((text, source))) => Lyrics::Text(text, source),
                            Ok(None) => Lyrics::Missing,
                            Err(e) => Lyrics::Err(e),
                        };
                    }
                }
            }
            Ev::Radio { items, play } => {
                let have: std::collections::HashSet<_> = self.queue.iter().map(|x| x.video_id.clone()).collect();
                let start = self.queue.len();
                self.queue.extend(items.into_iter().filter(|x| !have.contains(&x.video_id)));
                if play && self.queue.len() > start {
                    self.play_index(start);
                }
            }
            Ev::Notify(m, err) => self.notify(m, err),
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
                } else if !self.loaded {
                    self.play_index(self.qi.unwrap_or(0));
                } else {
                    self.mpv.cmd(json!(["cycle", "pause"]));
                }
            }
            KeyCode::Char('n') => self.advance(false),
            KeyCode::Char('p') => {
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
            KeyCode::Char('<') => self.mpv.cmd(json!(["seek", -self.cfg.seek, "relative"])),
            KeyCode::Char('>') => self.mpv.cmd(json!(["seek", self.cfg.seek, "relative"])),
            KeyCode::Char('+') | KeyCode::Char('=') => self.set_volume(self.cfg.vol_step),
            KeyCode::Char('-') => self.set_volume(-self.cfg.vol_step),
            KeyCode::Char('s') => {
                self.shuffle = !self.shuffle;
                if let (true, Some(qi)) = (self.shuffle, self.qi) {
                    let mut rest = self.queue.split_off(qi + 1);
                    self.shuffle_vec(&mut rest);
                    self.queue.extend(rest);
                    self.added = 0;
                }
            }
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
                    self.queue = vec![it.clone()];
                    self.play_index(0);
                    self.radio(&it, false);
                    self.notify(fill(self.s().radio_of, &[&it.title]), false);
                }
            }
            KeyCode::Char('L') => self.like(),
            _ => {}
        }
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
        let c = &self.cfg;
        self.theme = theme::load(&c.theme, c.transparent, &c.wallust);
        self.theme_mtime = theme::mtime(theme::source(&c.theme, &c.wallust).as_deref());
    }

    fn tick(&mut self) {
        let m = theme::mtime(theme::source(&self.cfg.theme, &self.cfg.wallust).as_deref());
        if m != self.theme_mtime {
            self.reload_theme();
        }
        if self.cfg.borders == config::Borders::Auto {
            self.rounded = hypr::rounding().is_none_or(|r| r > 0);
        }
        if self.note.as_ref().is_some_and(|n| n.2.elapsed() > Duration::from_secs(self.cfg.notify_secs)) {
            self.note = None;
        }
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
        match rx.recv_timeout(Duration::from_millis(250)) {
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
        if app.quit {
            break Ok(());
        }
        if last_tick.elapsed() > Duration::from_secs(2) {
            app.tick();
            last_tick = Instant::now();
            urgent = true;
        }
        // time-pos llega muchas veces por segundo; la barra no necesita tanto
        if urgent || last_draw.elapsed() >= Duration::from_millis(250) {
            if let Err(e) = term.draw(|f| app.draw(f)) {
                break Err(e);
            }
            last_draw = Instant::now();
        }
    };
    ratatui::restore();
    app.save_state();
    res
}

fn login() {
    let s = Config::load().lang.s();
    println!("{}", s.login_intro);
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let Some(auth) = api::parse_headers(&raw) else {
        eprintln!("{}", s.login_no_cookie);
        std::process::exit(1);
    };
    let dir = data_dir();
    let file = dir.join("auth.json");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, auth.to_string())) {
        eprintln!("{}", fill(s.login_save_failed, &[&file.display().to_string(), &e.to_string()]));
        std::process::exit(1);
    }
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
    match Api::new(&file).library_playlists() {
        Ok(p) => println!("{}", fill(s.login_ok, &[&p.len().to_string(), &file.display().to_string()])),
        Err(e) => println!("{}", fill(s.login_test_failed, &[&file.display().to_string(), &e])),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--login") {
        return login();
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", Config::load().lang.s().cli_help);
        return;
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
            eprintln!("ytm: {e}");
            std::process::exit(1);
        }
    };
    std::thread::spawn(move || loop {
        let ev = match event::read() {
            Ok(Event::Key(k)) => Ev::Key(k),
            Ok(Event::Resize(..)) => Ev::Resize,
            Ok(_) => continue,
            Err(_) => break,
        };
        if tx.send(ev).is_err() {
            break;
        }
    });
    if let Err(e) = run(rx, app, term) {
        eprintln!("ytm: {e}");
    }
}
