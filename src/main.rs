//! ytm — YouTube Music en la terminal, al estilo spotify-tui.
//!
//!     ytm            abre la app
//!     ytm --login    conecta tu cuenta (biblioteca, playlists, me gusta)

mod api;
mod mpv;
mod theme;
mod ui;

use api::{Api, Item, Kind};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::{ListState, TableState};
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
    theme: theme::Theme,
    theme_mtime: Option<SystemTime>,
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
    fn new(tx: Sender<Ev>) -> Result<App, String> {
        let st: Value = std::fs::read_to_string(state_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({}));
        let volume = st["volume"].as_i64().unwrap_or(70);
        let mpv = mpv::Mpv::start(volume, tx.clone())?;
        let api = Api::new(&data_dir().join("auth.json"));
        let mut app = App {
            api,
            tx,
            mpv,
            theme: theme::load(),
            theme_mtime: theme::mtime(),
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
        if app.api.authed() {
            let (api, tx) = (app.api.clone(), app.tx.clone());
            std::thread::spawn(move || match api.library_playlists() {
                Ok(p) => drop(tx.send(Ev::Playlists(p))),
                Err(e) => drop(tx.send(Ev::Notify(format!("playlists: {e}"), true))),
            });
        }
        app.open("Inicio", Load::Home, true);
        Ok(app)
    }

    fn save_state(&self) {
        let st = json!({
            "volume": self.volume,
            "shuffle": self.shuffle,
            "repeat": match self.repeat { Repeat::Off => "off", Repeat::All => "all", Repeat::One => "one" },
            "autoplay": self.autoplay,
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
        let mut s = vec![("\u{f02dc}  Inicio".to_string(), Side::Home)];
        if self.api.authed() {
            s.push(("\u{f02d1}  Me gusta".into(), Side::Liked));
            s.push(("\u{f02da}  Historial".into(), Side::History));
            s.push(("\u{f0025}  Álbumes".into(), Side::Albums));
            s.push(("\u{f0004}  Artistas".into(), Side::Artists));
        }
        s.push(("\u{f0cb8}  Cola".into(), Side::Queue));
        if !playlists.is_empty() {
            s.push((String::new(), Side::Sep));
            for p in playlists {
                if let Some(id) = &p.browse_id {
                    s.push((p.title.clone(), Side::Playlist(id.clone())));
                }
            }
        } else if !self.api.authed() {
            s.push((String::new(), Side::Sep));
            s.push(("sin cuenta: ytm --login".into(), Side::Hint));
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
        let Some((label, id)) = self.side_state.selected().and_then(|i| self.side.get(i)).cloned() else {
            return;
        };
        let title = label.split_once("  ").map(|x| x.1.to_string()).unwrap_or(label);
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
        self.qi = Some(i);
        self.added = 0;
        self.pos = None;
        self.dur = None;
        self.mpv.load(&vid);
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
                "time-pos" => self.pos = data.as_f64(),
                "duration" => self.dur = data.as_f64(),
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
                    self.notify(format!("no se pudo reproducir: {t}"), true);
                    if self.errors >= 3 {
                        self.errors = 0;
                        return self.notify("varios fallos seguidos; ¿yt-dlp desactualizado?", true);
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
            Ev::Playlists(p) => self.fill_side(&p),
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
            KeyCode::Tab => self.focus = if self.focus == Focus::Side { Focus::Main } else { Focus::Side },
            KeyCode::Char('h') | KeyCode::Left => self.focus = Focus::Side,
            KeyCode::Char('l') | KeyCode::Right => self.focus = Focus::Main,
            KeyCode::Char('j') | KeyCode::Down => self.nav(1),
            KeyCode::Char('k') | KeyCode::Up => self.nav(-1),
            KeyCode::PageDown => self.nav(page),
            KeyCode::PageUp => self.nav(-page),
            KeyCode::Char('d') if ctrl => self.nav(page),
            KeyCode::Char('u') if ctrl => self.nav(-page),
            KeyCode::Char('g') | KeyCode::Home => self.nav(-100_000),
            KeyCode::Char('G') | KeyCode::End => self.nav(100_000),
            KeyCode::Enter => {
                if self.focus == Focus::Side {
                    self.side_activate()
                } else {
                    self.activate()
                }
            }
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
                    self.mpv.cmd(json!(["cycle", "pause"]));
                }
            }
            KeyCode::Char('n') => self.advance(false),
            KeyCode::Char('p') => {
                let qi = self.qi.unwrap_or(0);
                if self.pos.unwrap_or(0.0) > 3.0 || qi == 0 {
                    self.mpv.cmd(json!(["seek", 0, "absolute"]));
                } else {
                    self.play_index(qi - 1);
                }
            }
            KeyCode::Char('<') => self.mpv.cmd(json!(["seek", -5, "relative"])),
            KeyCode::Char('>') => self.mpv.cmd(json!(["seek", 5, "relative"])),
            KeyCode::Char('+') | KeyCode::Char('=') => self.set_volume(5),
            KeyCode::Char('-') => self.set_volume(-5),
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
            KeyCode::Char('a') => {
                self.autoplay = !self.autoplay;
                let m = if self.autoplay { "activado" } else { "apagado" };
                self.notify(format!("autoplay (radio al acabar la cola) {m}"), false);
            }
            KeyCode::Char('z') => self.enqueue(),
            KeyCode::Char('R') => {
                if let Some((_, it)) = self.selected().filter(|(_, it)| it.playable()) {
                    self.queue = vec![it.clone()];
                    self.play_index(0);
                    self.radio(&it, false);
                    self.notify(format!("radio de {}", it.title), false);
                }
            }
            KeyCode::Char('L') => self.like(),
            _ => {}
        }
    }

    fn nav(&mut self, d: isize) {
        if self.focus == Focus::Side {
            for _ in 0..d.unsigned_abs().min(self.side.len()) {
                self.side_move(d);
            }
        } else {
            self.table_move(d);
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
        self.notify(format!("a la cola: {}", it.title), false);
        self.queue.insert(at, it);
    }

    fn like(&mut self) {
        let sel = if self.focus == Focus::Main { self.selected().map(|x| x.1) } else { None };
        let Some(it) = sel.filter(Item::playable).or_else(|| self.current().cloned()) else { return };
        if !self.api.authed() {
            return self.notify("necesitas cuenta: ytm --login", true);
        }
        let (api, tx) = (self.api.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let ev = match api.like(it.video_id.as_deref().unwrap_or("")) {
                Ok(()) => Ev::Notify(format!("\u{f02d1}  {}", it.title), false),
                Err(e) => Ev::Notify(format!("me gusta: {e}"), true),
            };
            let _ = tx.send(ev);
        });
    }

    fn tick(&mut self) {
        let m = theme::mtime();
        if m != self.theme_mtime {
            self.theme_mtime = m;
            self.theme = theme::load();
        }
        if self.note.as_ref().is_some_and(|n| n.2.elapsed() > Duration::from_secs(4)) {
            self.note = None;
        }
    }
}

fn run(rx: Receiver<Ev>, mut app: App) -> std::io::Result<()> {
    let mut term = ratatui::init();
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
    println!(
        "Conectar tu cuenta de YouTube Music:

  1. Abre https://music.youtube.com con tu sesión iniciada.
  2. F12 → pestaña Red (Network) → filtra por  browse
  3. Haz scroll o cambia de página para que aparezca una petición POST \"browse\".
  4. Clic derecho en ella → Copiar → Copiar encabezados de la petición
     (Firefox/Zen: \"Copiar encabezados de solicitud\").
  5. Pégalos aquí y termina con Enter y luego Ctrl+D.
"
    );
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let Some(auth) = api::parse_headers(&raw) else {
        eprintln!("\nno encontré la cookie con SAPISID en lo pegado; ¿copiaste los headers de la petición?");
        std::process::exit(1);
    };
    let dir = data_dir();
    let file = dir.join("auth.json");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, auth.to_string())) {
        eprintln!("no se pudo guardar {}: {e}", file.display());
        std::process::exit(1);
    }
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
    match Api::new(&file).library_playlists() {
        Ok(p) => println!("\nlisto: {} playlists en tu biblioteca. Guardado en {}", p.len(), file.display()),
        Err(e) => println!("\nguardado en {}, pero la prueba falló: {e}", file.display()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--login") {
        return login();
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("ytm — YouTube Music en la terminal\n\n  ytm            abre la app\n  ytm --login    conecta tu cuenta");
        return;
    }
    let (tx, rx) = mpsc::channel();
    let app = match App::new(tx.clone()) {
        Ok(a) => a,
        Err(e) => {
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
    if let Err(e) = run(rx, app) {
        eprintln!("ytm: {e}");
    }
}
