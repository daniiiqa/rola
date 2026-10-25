//! rola por MPRIS (D-Bus): así playerctl, las teclas multimedia, waybar y los
//! botones de los auriculares bluetooth manejan la cola de rola, no solo mpv
//! (que no sabe qué va después). Por eso a mpv no se le carga mpv-mpris.

use crate::Ev;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use zbus::blocking::Connection;
use zbus::interface;
use zbus::zvariant::{ObjectPath, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// Lo que piden desde fuera.
pub enum Media {
    Next,
    Previous,
    Play,
    Pause,
    PlayPause,
    Stop,
    Seek(i64),   // µs, relativo
    SetPos(i64), // µs, absoluto
    Volume(f64), // 0..1
    Shuffle(bool),
    Loop(String),
    Quit,
}

/// Lo que se cuenta hacia fuera.
#[derive(Clone, Default, PartialEq)]
pub struct State {
    pub status: &'static str, // Playing / Paused / Stopped
    pub track: u64,           // cambia con cada canción
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: String,
    pub length: i64, // µs
    pub volume: f64,
    pub shuffle: bool,
    pub loop_status: &'static str, // None / Playlist / Track
    pub can_next: bool,
    pub can_prev: bool,
}

impl State {
    fn track_path(&self) -> ObjectPath<'static> {
        ObjectPath::try_from(format!("/org/rola/track/{}", self.track))
            .unwrap_or_else(|_| ObjectPath::from_static_str_unchecked("/org/mpris/MediaPlayer2/TrackList/NoTrack"))
    }

    fn metadata(&self) -> HashMap<String, Value<'static>> {
        let mut m = HashMap::new();
        if self.status == "Stopped" {
            return m;
        }
        m.insert("mpris:trackid".into(), Value::from(self.track_path()));
        m.insert("xesam:title".into(), Value::from(self.title.clone()));
        if !self.artist.is_empty() {
            m.insert("xesam:artist".into(), Value::from(vec![self.artist.clone()]));
        }
        if !self.album.is_empty() {
            m.insert("xesam:album".into(), Value::from(self.album.clone()));
        }
        if !self.art.is_empty() {
            m.insert("mpris:artUrl".into(), Value::from(self.art.clone()));
        }
        if self.length > 0 {
            m.insert("mpris:length".into(), Value::from(self.length));
        }
        m
    }
}

#[derive(Default)]
struct Shared {
    state: State,
    pos: i64, // µs; se pregunta, no se avisa
}

struct Root {
    tx: Sender<Ev>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}
    fn quit(&self) {
        let _ = self.tx.send(Ev::Media(Media::Quit));
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> String {
        "rola".into()
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    tx: Sender<Ev>,
    shared: Arc<Mutex<Shared>>,
}

impl Player {
    fn send(&self, m: Media) {
        let _ = self.tx.send(Ev::Media(m));
    }
    fn state(&self) -> State {
        self.shared.lock().map(|s| s.state.clone()).unwrap_or_default()
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.send(Media::Next)
    }
    fn previous(&self) {
        self.send(Media::Previous)
    }
    fn pause(&self) {
        self.send(Media::Pause)
    }
    fn play_pause(&self) {
        self.send(Media::PlayPause)
    }
    fn stop(&self) {
        self.send(Media::Stop)
    }
    fn play(&self) {
        self.send(Media::Play)
    }
    fn seek(&self, offset: i64) {
        self.send(Media::Seek(offset))
    }
    fn set_position(&self, _track_id: ObjectPath<'_>, position: i64) {
        self.send(Media::SetPos(position))
    }
    fn open_uri(&self, _uri: String) {}

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.state().status.into()
    }
    #[zbus(property)]
    fn loop_status(&self) -> String {
        self.state().loop_status.into()
    }
    #[zbus(property)]
    fn set_loop_status(&mut self, v: String) {
        self.send(Media::Loop(v))
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn set_rate(&mut self, _v: f64) {}
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.state().shuffle
    }
    #[zbus(property)]
    fn set_shuffle(&mut self, v: bool) {
        self.send(Media::Shuffle(v))
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, Value<'static>> {
        self.state().metadata()
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.state().volume
    }
    #[zbus(property)]
    fn set_volume(&mut self, v: f64) {
        self.send(Media::Volume(v))
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.shared.lock().map(|s| s.pos).unwrap_or(0)
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.state().can_next
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.state().can_prev
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.state().status != "Stopped"
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.state().status != "Stopped"
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.state().status != "Stopped"
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

pub struct Mpris {
    conn: Connection,
    shared: Arc<Mutex<Shared>>,
    last: State,
    // para notar saltos de posición (Seeked): dónde estaba y cuándo
    mark: Option<(i64, Instant, bool)>,
}

impl Mpris {
    /// None sin bus de sesión (por SSH, en una tty…): rola funciona igual.
    pub fn start(tx: Sender<Ev>) -> Option<Mpris> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let conn = zbus::blocking::connection::Builder::session()
            .ok()?
            .serve_at(PATH, Root { tx: tx.clone() })
            .ok()?
            .serve_at(PATH, Player { tx, shared: shared.clone() })
            .ok()?
            .build()
            .ok()?;
        // con otro rola abierto, el nombre corto ya es suyo
        let name = "org.mpris.MediaPlayer2.rola";
        if conn.request_name(name).is_err() {
            conn.request_name(format!("{name}.instance{}", std::process::id())).ok()?;
        }
        Some(Mpris { conn, shared, last: State::default(), mark: None })
    }

    /// Cuenta lo que haya cambiado desde la última vez.
    pub fn update(&mut self, st: State, pos: Option<f64>) {
        let pos_us = pos.map(|p| (p * 1e6) as i64).unwrap_or(0);
        if let Ok(mut s) = self.shared.lock() {
            s.state = st.clone();
            s.pos = pos_us;
        }
        let playing = st.status == "Playing";
        // un salto de más de 2 s respecto a lo esperado es un seek
        if let Some((p0, t0, was)) = self.mark {
            let expect = p0 + if was { t0.elapsed().as_micros() as i64 } else { 0 };
            if st.track == self.last.track && (pos_us - expect).abs() > 2_000_000 {
                let _ = self.conn.emit_signal(None::<&str>, PATH, PLAYER, "Seeked", &(pos_us,));
            }
        }
        self.mark = Some((pos_us, Instant::now(), playing));
        if st == self.last {
            return;
        }
        let (a, b) = (&self.last, &st);
        let mut changed: HashMap<&str, Value> = HashMap::new();
        if a.status != b.status {
            changed.insert("PlaybackStatus", Value::from(b.status));
            for k in ["CanPlay", "CanPause", "CanSeek"] {
                changed.insert(k, Value::from(b.status != "Stopped"));
            }
        }
        if a.metadata() != b.metadata() {
            changed.insert("Metadata", Value::from(b.metadata()));
        }
        if a.volume != b.volume {
            changed.insert("Volume", Value::from(b.volume));
        }
        if a.shuffle != b.shuffle {
            changed.insert("Shuffle", Value::from(b.shuffle));
        }
        if a.loop_status != b.loop_status {
            changed.insert("LoopStatus", Value::from(b.loop_status));
        }
        if a.can_next != b.can_next {
            changed.insert("CanGoNext", Value::from(b.can_next));
        }
        if a.can_prev != b.can_prev {
            changed.insert("CanGoPrevious", Value::from(b.can_prev));
        }
        self.last = st;
        if !changed.is_empty() {
            let body = (PLAYER, changed, Vec::<&str>::new());
            let _ = self.conn.emit_signal(None::<&str>, PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged", &body);
        }
    }
}
