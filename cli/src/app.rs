use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::api::{ApiClient, LyricsData, Playlist, Track};
use crate::config::{
    default_device_id, device_display_name, normalize_server, save_session, SessionConfig,
    DEFAULT_SERVER,
};
use crate::player::{AudioPlayer, PlaybackStatus, PlayerSnapshot};
use crate::sync::{SyncClient, SyncInbound, SyncOutbound};
use crate::theme::{LayoutMode, Theme, ThemeId};

pub const PLAYLIST_COLORS: [(&str, &str); 6] = [
    ("Mięta", "#6ee7b7"),
    ("Fiolet", "#a78bfa"),
    ("Róż", "#f472b6"),
    ("Bursztyn", "#fb923c"),
    ("Błękit", "#38bdf8"),
    ("Nothing", "#ff3b30"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Login,
    Main,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LoginField {
    Server,
    Username,
    Password,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Tracks,
    SearchInput,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StudioTab {
    Queue,
    Lyrics,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RepeatMode::Off => "OFF",
            RepeatMode::All => "ALL",
            RepeatMode::One => "ONE",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum NavItem {
    Search,
    Liked,
    Local,
    Queue,
    Playlist { id: i64, name: String, count: usize },
    Settings,
}

impl NavItem {
    pub fn label(&self) -> String {
        match self {
            NavItem::Search => "⌕  Szukaj".to_string(),
            NavItem::Liked => "♥  Polubione".to_string(),
            NavItem::Local => "♫  Moja muzyka".to_string(),
            NavItem::Queue => "≡  Kolejka".to_string(),
            NavItem::Playlist { name, count, .. } => format!("♪  {name} ({count})"),
            NavItem::Settings => "⚙  Ustawienia".to_string(),
        }
    }
}

#[derive(Clone)]
pub enum Modal {
    CreatePlaylist {
        name: String,
        color_idx: usize,
        pending_track: Option<Track>,
    },
    AddToPlaylist {
        track: Track,
        selected: usize,
    },
}

struct PlayTracker {
    track: Track,
    started_at_ms: i64,
    played_secs: f64,
    last_tick: Instant,
}

enum ApiJob {
    Login {
        server: String,
        username: String,
        password: String,
        theme: ThemeId,
        layout: LayoutMode,
        volume: u8,
    },
    LoadBootstrap,
    RefreshPlaylists,
    LoadPlaylist(i64, String),
    LoadLiked,
    LoadLocal,
    Search(String),
    ToggleLike {
        track: Track,
        currently_liked: bool,
    },
    CreatePlaylist {
        name: String,
        cover_color: String,
        pending_track: Option<Track>,
    },
    AddToPlaylist {
        playlist_id: i64,
        playlist_name: String,
        track: Track,
    },
    LoadRadio(Track),
    LoadLyrics(Track),
    Presence(Option<Track>, bool),
    RecordPlay {
        track: Track,
        played_s: i64,
        completed: bool,
        session_id: String,
        started_at_ms: i64,
    },
}

enum ApiResult {
    LoginOk(SessionConfig),
    LoginErr(String),
    Bootstrap {
        playlists: Result<Vec<Playlist>, String>,
        liked: Result<Vec<Track>, String>,
    },
    PlaylistsRefreshed(Result<Vec<Playlist>, String>),
    TracksLoaded {
        title: String,
        tracks: Result<Vec<Track>, String>,
    },
    LikeToggled {
        track_id: String,
        liked: bool,
        res: Result<(), String>,
    },
    PlaylistCreated {
        res: Result<Playlist, String>,
        pending_track: Option<Track>,
    },
    TrackAddedToPlaylist {
        playlist_name: String,
        res: Result<bool, String>,
    },
    RadioLoaded(Result<Vec<Track>, String>),
    LyricsLoaded(LyricsData),
}

pub struct App {
    pub screen: Screen,

    // Login state
    pub login_server: String,
    pub login_user: String,
    pub login_pass: String,
    pub login_field: LoginField,
    pub login_busy: bool,
    pub login_error: Option<String>,

    // Session, API & WebSocket Sync
    pub session: Option<SessionConfig>,
    pub api: Option<ApiClient>,
    pub sync: Option<SyncClient>,
    pub sync_connected: bool,
    pub sync_devices: usize,
    pub session_id: String,
    job_tx: Sender<ApiJob>,
    res_rx: Receiver<ApiResult>,
    api_tx_updater: Sender<ApiClient>,

    // Appearance & Layout
    pub theme_id: ThemeId,
    pub layout: LayoutMode,
    pub studio_tab: StudioTab,
    pub settings_index: usize,

    // Modals (Tworzenie playlisty / Dodaj do playlisty)
    pub modal: Option<Modal>,

    // Navigation & library
    pub focus: Focus,
    pub nav_items: Vec<NavItem>,
    pub nav_index: usize,
    pub playlists: Vec<Playlist>,
    pub liked_ids: HashSet<String>,

    // Tracks view
    pub view_title: String,
    pub tracks: Vec<Track>,
    pub track_index: usize,
    pub loading: bool,

    // Search
    pub search_query: String,

    // Playback, Queue & Telemetry
    pub player: AudioPlayer,
    pub queue: Vec<Track>,
    pub queue_index: Option<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub last_finished_seq: u64,
    pub last_status: PlaybackStatus,
    pub last_presence_at: Instant,
    pub last_sync_push_at: Instant,
    play_tracker: Option<PlayTracker>,

    // Lyrics
    pub show_lyrics: bool,
    pub lyrics: LyricsData,

    // Toast / status message
    pub status_msg: Option<(String, Instant)>,
}

impl App {
    pub fn new(initial_session: Option<SessionConfig>, override_server: Option<String>) -> Self {
        let initial_vol = initial_session.as_ref().map(|s| s.volume).unwrap_or(85);
        let initial_theme = initial_session.as_ref().map(|s| s.theme).unwrap_or_default();
        let initial_layout = initial_session.as_ref().map(|s| s.layout).unwrap_or_default();
        let default_server = override_server
            .as_deref()
            .map(normalize_server)
            .or_else(|| initial_session.as_ref().map(|s| s.server_url.clone()))
            .unwrap_or_else(|| DEFAULT_SERVER.to_string());

        let (job_tx, job_rx) = mpsc::channel::<ApiJob>();
        let (res_tx, res_rx) = mpsc::channel::<ApiResult>();
        let (api_tx_updater, api_rx_updater) = mpsc::channel::<ApiClient>();

        let initial_client = initial_session
            .as_ref()
            .map(|s| ApiClient::new(&s.server_url, &s.token));

        let initial_sync = initial_session.as_ref().map(|s| {
            let dev_id = if s.device_id.is_empty() {
                default_device_id()
            } else {
                s.device_id.clone()
            };
            SyncClient::start(
                s.server_url.clone(),
                s.token.clone(),
                dev_id,
                device_display_name(),
            )
        });

        spawn_api_worker(job_rx, res_tx, api_rx_updater, initial_client.clone());

        let now_ms = unix_now_ms();
        let logged_in = initial_session.is_some();
        let app = Self {
            screen: if logged_in { Screen::Main } else { Screen::Login },
            login_server: default_server,
            login_user: String::new(),
            login_pass: String::new(),
            login_field: LoginField::Username,
            login_busy: false,
            login_error: None,
            session: initial_session,
            api: initial_client,
            sync: initial_sync,
            sync_connected: false,
            sync_devices: 0,
            session_id: format!("cli_{now_ms}"),
            job_tx,
            res_rx,
            api_tx_updater,
            theme_id: initial_theme,
            layout: initial_layout,
            studio_tab: StudioTab::Queue,
            settings_index: 0,
            modal: None,
            focus: Focus::Tracks,
            nav_items: vec![
                NavItem::Search,
                NavItem::Liked,
                NavItem::Local,
                NavItem::Queue,
                NavItem::Settings,
            ],
            nav_index: 1,
            playlists: Vec::new(),
            liked_ids: HashSet::new(),
            view_title: "Polubione".to_string(),
            tracks: Vec::new(),
            track_index: 0,
            loading: logged_in,
            search_query: String::new(),
            player: AudioPlayer::new(initial_vol),
            queue: Vec::new(),
            queue_index: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            last_finished_seq: 0,
            last_status: PlaybackStatus::Stopped,
            last_presence_at: Instant::now() - Duration::from_secs(60),
            last_sync_push_at: Instant::now() - Duration::from_secs(10),
            play_tracker: None,
            show_lyrics: false,
            lyrics: LyricsData::default(),
            status_msg: None,
        };

        if logged_in {
            let _ = app.job_tx.send(ApiJob::LoadBootstrap);
        }

        app
    }

    pub fn theme(&self) -> Theme {
        self.theme_id.theme()
    }

    pub fn is_settings_view(&self) -> bool {
        matches!(self.nav_items.get(self.nav_index), Some(NavItem::Settings))
    }

    pub fn settings_count(&self) -> usize {
        ThemeId::ALL.len() + LayoutMode::ALL.len()
    }

    pub fn activate_setting(&mut self) {
        let theme_len = ThemeId::ALL.len();
        if self.settings_index < theme_len {
            let chosen = ThemeId::ALL[self.settings_index];
            self.theme_id = chosen;
            self.persist_prefs();
            self.set_status(format!("Motyw zmieniony na: {}", chosen.name()));
        } else {
            let l_idx = self.settings_index - theme_len;
            if let Some(&chosen) = LayoutMode::ALL.get(l_idx) {
                self.layout = chosen;
                self.persist_prefs();
                self.set_status(format!("Układ zmieniony na: {}", chosen.name()));
            }
        }
    }

    fn persist_prefs(&mut self) {
        if let Some(sess) = self.session.as_mut() {
            sess.theme = self.theme_id;
            sess.layout = self.layout;
            sess.volume = self.player.volume();
            if sess.device_id.is_empty() {
                sess.device_id = default_device_id();
            }
            let _ = save_session(sess);
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = Some((msg.into(), Instant::now()));
    }

    pub fn active_status(&self) -> Option<&str> {
        self.status_msg
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(5))
            .map(|(m, _)| m.as_str())
    }

    pub fn submit_login(&mut self) {
        if self.login_busy {
            return;
        }
        let server = normalize_server(&self.login_server);
        let user = self.login_user.trim().to_string();
        let pass = self.login_pass.clone();
        if user.is_empty() || pass.is_empty() {
            self.login_error = Some("Podaj login i hasło.".to_string());
            return;
        }
        self.login_busy = true;
        self.login_error = None;
        let _ = self.job_tx.send(ApiJob::Login {
            server,
            username: user,
            password: pass,
            theme: self.theme_id,
            layout: self.layout,
            volume: self.player.volume(),
        });
    }

    pub fn rebuild_nav(&mut self) {
        let current_item = self.nav_items.get(self.nav_index).cloned();
        let mut items = vec![
            NavItem::Search,
            NavItem::Liked,
            NavItem::Local,
            NavItem::Queue,
        ];
        for p in &self.playlists {
            items.push(NavItem::Playlist {
                id: p.id,
                name: p.name.clone(),
                count: p.track_count,
            });
        }
        items.push(NavItem::Settings);
        self.nav_items = items;

        if let Some(prev) = current_item
            && let Some(pos) = self.nav_items.iter().position(|x| match (x, &prev) {
                (NavItem::Playlist { id: a, .. }, NavItem::Playlist { id: b, .. }) => a == b,
                _ => x == &prev,
            })
        {
            self.nav_index = pos;
        } else if self.nav_index >= self.nav_items.len() {
            self.nav_index = 0;
        }
    }

    pub fn activate_nav(&mut self) {
        let Some(item) = self.nav_items.get(self.nav_index).cloned() else {
            return;
        };
        match item {
            NavItem::Search => {
                self.view_title = if self.search_query.is_empty() {
                    "Szukaj (wpisz tytuł lub wklej link YouTube)".to_string()
                } else {
                    format!("Wyniki: {}", self.search_query)
                };
                self.focus = Focus::SearchInput;
            }
            NavItem::Liked => {
                self.loading = true;
                self.view_title = "Polubione".to_string();
                self.focus = Focus::Tracks;
                let _ = self.job_tx.send(ApiJob::LoadLiked);
            }
            NavItem::Local => {
                self.loading = true;
                self.view_title = "Moja muzyka".to_string();
                self.focus = Focus::Tracks;
                let _ = self.job_tx.send(ApiJob::LoadLocal);
            }
            NavItem::Queue => {
                self.loading = false;
                self.view_title = format!("Kolejka ({})", self.queue.len());
                self.tracks = self.queue.clone();
                self.track_index = self
                    .queue_index
                    .unwrap_or(0)
                    .min(self.tracks.len().saturating_sub(1));
                self.focus = Focus::Tracks;
            }
            NavItem::Playlist { id, name, .. } => {
                self.loading = true;
                self.view_title = name.clone();
                self.focus = Focus::Tracks;
                let _ = self.job_tx.send(ApiJob::LoadPlaylist(id, name));
            }
            NavItem::Settings => {
                self.loading = false;
                self.view_title = "Ustawienia".to_string();
                self.focus = Focus::Settings;
            }
        }
    }

    pub fn run_search(&mut self) {
        let q = self.search_query.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.loading = true;
        self.view_title = format!("Szukam: {q}...");
        self.focus = Focus::Tracks;
        let _ = self.job_tx.send(ApiJob::Search(q));
    }

    // --- Modals: tworzenie playlisty & dodawanie utworów ---

    pub fn open_create_playlist_modal(&mut self, pending_track: Option<Track>) {
        self.modal = Some(Modal::CreatePlaylist {
            name: String::new(),
            color_idx: 0,
            pending_track,
        });
    }

    pub fn open_add_to_playlist_modal(&mut self) {
        let target = if self.focus == Focus::Tracks && !self.is_settings_view() {
            self.tracks.get(self.track_index).cloned()
        } else {
            None
        }
        .or_else(|| self.player.snapshot().track);

        let Some(track) = target else {
            self.set_status("Wybierz lub włącz utwór, żeby dodać go do playlisty.");
            return;
        };
        self.modal = Some(Modal::AddToPlaylist { track, selected: 0 });
    }

    pub fn submit_create_playlist(&mut self) {
        let Some(Modal::CreatePlaylist {
            name,
            color_idx,
            pending_track,
        }) = self.modal.clone()
        else {
            return;
        };
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            self.set_status("Nazwa playlisty nie może być pusta.");
            return;
        }
        let color = PLAYLIST_COLORS[color_idx % PLAYLIST_COLORS.len()].1.to_string();
        self.modal = None;
        self.set_status(format!("Tworzę playlistę „{trimmed}”..."));
        let _ = self.job_tx.send(ApiJob::CreatePlaylist {
            name: trimmed,
            cover_color: color,
            pending_track,
        });
    }

    pub fn confirm_add_to_playlist(&mut self) {
        let Some(Modal::AddToPlaylist { track, selected }) = self.modal.clone() else {
            return;
        };
        // Ostatnia opcja to "+ Utwórz nową playlistę..."
        if selected >= self.playlists.len() {
            self.open_create_playlist_modal(Some(track));
            return;
        }
        let pl = self.playlists[selected].clone();
        self.modal = None;
        self.set_status(format!("Dodaję do „{}”...", pl.name));
        let _ = self.job_tx.send(ApiJob::AddToPlaylist {
            playlist_id: pl.id,
            playlist_name: pl.name,
            track,
        });
    }

    // --- Odtwarzanie, kolejka, presence, sync ---

    fn flush_play_tracker(&mut self, completed: bool) {
        if let Some(tracker) = self.play_tracker.take() {
            let played_s = tracker.played_secs.round() as i64;
            if played_s >= 3 || completed {
                let _ = self.job_tx.send(ApiJob::RecordPlay {
                    track: tracker.track,
                    played_s: played_s.max(1),
                    completed,
                    session_id: self.session_id.clone(),
                    started_at_ms: tracker.started_at_ms,
                });
            }
        }
    }

    pub fn play_selected_track(&mut self) {
        if self.tracks.is_empty() || self.track_index >= self.tracks.len() {
            return;
        }
        let is_queue_view = matches!(self.nav_items.get(self.nav_index), Some(NavItem::Queue));
        if is_queue_view {
            self.play_queue_index(self.track_index);
        } else {
            self.queue = self.tracks.clone();
            self.play_queue_index(self.track_index);
        }
    }

    pub fn play_queue_index(&mut self, idx: usize) {
        self.play_queue_index_at(idx, 0.0, false);
    }

    fn play_queue_index_at(&mut self, idx: usize, start_secs: f64, start_paused: bool) {
        let Some(track) = self.queue.get(idx).cloned() else {
            return;
        };
        let Some(url) = self.api.as_ref().map(|a| a.stream_url(&track)) else {
            return;
        };

        self.flush_play_tracker(false);

        self.queue_index = Some(idx);
        self.player.play_at(track.clone(), url, start_secs, start_paused);
        self.set_status(format!("▶ {} — {}", track.title, track.artist));

        self.play_tracker = Some(PlayTracker {
            track: track.clone(),
            started_at_ms: unix_now_ms(),
            played_secs: 0.0,
            last_tick: Instant::now(),
        });

        // Natychmiastowe wysłanie obecności + stanu synchronizacji /ws/sync
        self.last_presence_at = Instant::now();
        let _ = self
            .job_tx
            .send(ApiJob::Presence(Some(track.clone()), !start_paused));
        self.push_sync_state(
            Some(track.clone()),
            !start_paused,
            start_secs,
            track.duration_secs().unwrap_or(0.0),
        );

        self.lyrics = LyricsData {
            track_id: track.id.clone(),
            loading: true,
            ..Default::default()
        };
        let _ = self.job_tx.send(ApiJob::LoadLyrics(track));
    }

    pub fn toggle_pause(&mut self) {
        let now_paused = self.player.toggle_pause();
        let snap = self.player.snapshot();
        self.last_presence_at = Instant::now();
        if now_paused {
            let _ = self.job_tx.send(ApiJob::Presence(None, false));
        } else if let Some(tr) = snap.track.clone() {
            let _ = self.job_tx.send(ApiJob::Presence(Some(tr), true));
        }
        self.push_sync_state(
            snap.track,
            !now_paused,
            snap.position_secs,
            snap.duration_secs,
        );
    }

    pub fn seek_relative(&mut self, delta: f64) {
        self.player.seek_relative(delta);
        let snap = self.player.snapshot();
        self.push_sync_state(
            snap.track,
            snap.status == PlaybackStatus::Playing,
            (snap.position_secs + delta).max(0.0),
            snap.duration_secs,
        );
    }

    fn push_sync_state(
        &mut self,
        track: Option<Track>,
        is_playing: bool,
        position: f64,
        duration: f64,
    ) {
        self.last_sync_push_at = Instant::now();
        if let Some(sync) = self.sync.as_ref() {
            sync.send(SyncOutbound::PlaybackState {
                track,
                is_playing,
                position,
                duration,
                queue: self.queue.clone(),
                q_idx: self.queue_index.unwrap_or(0),
            });
        }
    }

    pub fn next_track(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let cur = self.queue_index.unwrap_or(0);
        if self.repeat == RepeatMode::One {
            self.play_queue_index(cur);
            return;
        }
        let next = if self.shuffle && self.queue.len() > 1 {
            let seed = Instant::now().elapsed().subsec_nanos() as usize
                ^ (SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as usize)
                    .unwrap_or(7));
            let mut pick = seed % self.queue.len();
            if pick == cur {
                pick = (pick + 1) % self.queue.len();
            }
            pick
        } else if cur + 1 < self.queue.len() {
            cur + 1
        } else if self.repeat == RepeatMode::All {
            0
        } else {
            self.set_status("Koniec kolejki (wciśnij [r], żeby uruchomić Radio)");
            return;
        };
        self.play_queue_index(next);
    }

    pub fn prev_track(&mut self) {
        let snap = self.player.snapshot();
        if snap.position_secs > 4.0 {
            self.player.seek_to(0.0);
            self.push_sync_state(
                snap.track,
                snap.status == PlaybackStatus::Playing,
                0.0,
                snap.duration_secs,
            );
            return;
        }
        if self.queue.is_empty() {
            return;
        }
        let cur = self.queue_index.unwrap_or(0);
        let prev = if cur > 0 {
            cur - 1
        } else if self.repeat == RepeatMode::All {
            self.queue.len() - 1
        } else {
            0
        };
        self.play_queue_index(prev);
    }

    pub fn enqueue_selected(&mut self) {
        if self.is_settings_view() {
            return;
        }
        let Some(track) = self.tracks.get(self.track_index).cloned() else {
            return;
        };
        self.queue.push(track.clone());
        self.set_status(format!("+ Dodano do kolejki: {}", track.title));
    }

    pub fn toggle_like_selected(&mut self) {
        let target = if self.focus == Focus::Tracks && !self.is_settings_view() {
            self.tracks.get(self.track_index).cloned()
        } else {
            self.player.snapshot().track
        };
        let Some(track) = target else { return };
        let currently_liked = self.liked_ids.contains(&track.id);
        let _ = self.job_tx.send(ApiJob::ToggleLike {
            track,
            currently_liked,
        });
    }

    pub fn start_radio(&mut self) {
        let seed = if self.focus == Focus::Tracks && !self.is_settings_view() {
            self.tracks.get(self.track_index).cloned()
        } else {
            None
        }
        .or_else(|| self.player.snapshot().track);

        let Some(track) = seed else {
            self.set_status("Wybierz lub włącz utwór, aby uruchomić Radio.");
            return;
        };
        self.set_status(format!("📻 Generuję radio dla: {}...", track.title));
        let _ = self.job_tx.send(ApiJob::LoadRadio(track));
    }

    pub fn toggle_lyrics_or_studio_tab(&mut self) {
        match self.layout {
            LayoutMode::Studio => {
                self.studio_tab = match self.studio_tab {
                    StudioTab::Queue => StudioTab::Lyrics,
                    StudioTab::Lyrics => StudioTab::Queue,
                };
                if self.studio_tab == StudioTab::Lyrics {
                    self.ensure_current_lyrics();
                }
            }
            LayoutMode::Default => {
                self.show_lyrics = !self.show_lyrics;
                if self.show_lyrics {
                    self.ensure_current_lyrics();
                }
            }
        }
    }

    fn ensure_current_lyrics(&mut self) {
        if let Some(track) = self.player.snapshot().track
            && self.lyrics.track_id != track.id
        {
            self.lyrics = LyricsData {
                track_id: track.id.clone(),
                loading: true,
                ..Default::default()
            };
            let _ = self.job_tx.send(ApiJob::LoadLyrics(track));
        }
    }

    pub fn set_volume(&mut self, vol: u8) {
        self.player.set_volume(vol.clamp(0, 100));
        self.persist_prefs();
    }

    pub fn adjust_volume(&mut self, delta: i16) {
        let cur = self.player.volume() as i16;
        let next = (cur + delta).clamp(0, 100) as u8;
        self.set_volume(next);
    }

    fn handle_sync_inbound(&mut self, msg: SyncInbound) {
        match msg {
            SyncInbound::Connected(ok) => {
                self.sync_connected = ok;
                if ok {
                    let snap = self.player.snapshot();
                    if snap.track.is_some() {
                        self.push_sync_state(
                            snap.track,
                            snap.status == PlaybackStatus::Playing,
                            snap.position_secs,
                            snap.duration_secs,
                        );
                    }
                }
            }
            SyncInbound::DeviceCount(n) => {
                self.sync_devices = n;
            }
            SyncInbound::CmdPlay => {
                self.player.set_paused(false);
                let snap = self.player.snapshot();
                if let Some(tr) = snap.track.clone() {
                    let _ = self.job_tx.send(ApiJob::Presence(Some(tr), true));
                }
                self.push_sync_state(snap.track, true, snap.position_secs, snap.duration_secs);
            }
            SyncInbound::CmdPause => {
                self.player.set_paused(true);
                let snap = self.player.snapshot();
                let _ = self.job_tx.send(ApiJob::Presence(None, false));
                self.push_sync_state(snap.track, false, snap.position_secs, snap.duration_secs);
            }
            SyncInbound::CmdNext => self.next_track(),
            SyncInbound::CmdPrev => self.prev_track(),
            SyncInbound::CmdSeek(pos) => {
                self.player.seek_to(pos);
                let snap = self.player.snapshot();
                self.push_sync_state(
                    snap.track,
                    snap.status == PlaybackStatus::Playing,
                    pos,
                    snap.duration_secs,
                );
            }
            SyncInbound::CmdVolume(pct) => {
                self.set_volume(pct);
            }
            SyncInbound::CmdPlayTrack { track, queue, idx } => {
                self.queue = if queue.is_empty() {
                    vec![track.clone()]
                } else {
                    queue
                };
                let target_idx = if idx < self.queue.len() {
                    idx
                } else {
                    self.queue
                        .iter()
                        .position(|t| t.id == track.id)
                        .unwrap_or(0)
                };
                self.play_queue_index(target_idx);
                self.set_status(format!("▶ Zdalnie: {}", track.title));
            }
            SyncInbound::CmdTransferTo { target_device_id } => {
                let snap = self.player.snapshot();
                if let Some(sync) = self.sync.as_ref() {
                    sync.send(SyncOutbound::Transfer {
                        target_device_id,
                        track: snap.track.clone(),
                        position: snap.position_secs,
                        queue: self.queue.clone(),
                        q_idx: self.queue_index.unwrap_or(0),
                        is_playing: snap.status == PlaybackStatus::Playing,
                    });
                }
                self.player.set_paused(true);
                let _ = self.job_tx.send(ApiJob::Presence(None, false));
                self.set_status("Przeniesiono odtwarzanie na inne urządzenie");
            }
            SyncInbound::TakeOver {
                track,
                position,
                queue,
                q_idx,
                is_playing,
                from_name,
            } => {
                self.queue = if queue.is_empty() {
                    vec![track.clone()]
                } else {
                    queue
                };
                let target_idx = if q_idx < self.queue.len() {
                    q_idx
                } else {
                    self.queue
                        .iter()
                        .position(|t| t.id == track.id)
                        .unwrap_or(0)
                };
                self.play_queue_index_at(target_idx, position, !is_playing);
                self.set_status(format!("▶ Przeniesiono z {from_name}"));
            }
        }
    }

    pub fn tick(&mut self) -> PlayerSnapshot {
        // 1. Odbierz zdarzenia WebSocket /ws/sync (zdalne sterowanie + transfer)
        let mut sync_msgs = Vec::new();
        if let Some(sync) = self.sync.as_ref() {
            while let Ok(msg) = sync.in_rx.try_recv() {
                sync_msgs.push(msg);
            }
        }
        for msg in sync_msgs {
            self.handle_sync_inbound(msg);
        }

        // 2. Odbierz wyniki z wątku REST API
        while let Ok(res) = self.res_rx.try_recv() {
            match res {
                ApiResult::LoginOk(cfg) => {
                    self.login_busy = false;
                    self.login_error = None;
                    self.login_pass.clear();
                    let _ = save_session(&cfg);
                    let client = ApiClient::new(&cfg.server_url, &cfg.token);
                    let _ = self.api_tx_updater.send(client.clone());
                    self.api = Some(client);
                    let dev_id = if cfg.device_id.is_empty() {
                        default_device_id()
                    } else {
                        cfg.device_id.clone()
                    };
                    self.sync = Some(SyncClient::start(
                        cfg.server_url.clone(),
                        cfg.token.clone(),
                        dev_id,
                        device_display_name(),
                    ));
                    self.session = Some(cfg);
                    self.screen = Screen::Main;
                    self.loading = true;
                    self.set_status("Zalogowano! Sesja zapamiętana.");
                    let _ = self.job_tx.send(ApiJob::LoadBootstrap);
                }
                ApiResult::LoginErr(err) => {
                    self.login_busy = false;
                    self.login_error = Some(err);
                }
                ApiResult::Bootstrap { playlists, liked } => {
                    self.loading = false;
                    match playlists {
                        Ok(pls) => {
                            self.playlists = pls;
                            self.rebuild_nav();
                        }
                        Err(e) => self.set_status(format!("Playlisty: {e}")),
                    }
                    match liked {
                        Ok(tracks) => {
                            self.liked_ids = tracks.iter().map(|t| t.id.clone()).collect();
                            self.tracks = tracks;
                            self.track_index = 0;
                            self.view_title = format!("Polubione ({})", self.tracks.len());
                        }
                        Err(e) => self.set_status(format!("Polubione: {e}")),
                    }
                }
                ApiResult::PlaylistsRefreshed(Ok(pls)) => {
                    self.playlists = pls;
                    self.rebuild_nav();
                }
                ApiResult::PlaylistsRefreshed(Err(_)) => {}
                ApiResult::TracksLoaded { title, tracks } => {
                    self.loading = false;
                    match tracks {
                        Ok(list) => {
                            self.view_title = format!("{title} ({})", list.len());
                            self.tracks = list;
                            self.track_index = 0;
                        }
                        Err(e) => {
                            self.view_title = title;
                            self.set_status(format!("Błąd: {e}"));
                        }
                    }
                }
                ApiResult::LikeToggled {
                    track_id,
                    liked,
                    res,
                } => match res {
                    Ok(()) => {
                        if liked {
                            self.liked_ids.insert(track_id);
                            self.set_status("♥ Dodano do polubionych");
                        } else {
                            self.liked_ids.remove(&track_id);
                            self.set_status("Usunięto z polubionych");
                        }
                    }
                    Err(e) => self.set_status(format!("Błąd polubienia: {e}")),
                },
                ApiResult::PlaylistCreated { res, pending_track } => match res {
                    Ok(pl) => {
                        let name = pl.name.clone();
                        let pid = pl.id;
                        self.playlists.push(pl);
                        self.rebuild_nav();
                        if let Some(tr) = pending_track {
                            self.set_status(format!("Utworzono „{name}”, dodaję utwór..."));
                            let _ = self.job_tx.send(ApiJob::AddToPlaylist {
                                playlist_id: pid,
                                playlist_name: name,
                                track: tr,
                            });
                        } else {
                            self.set_status(format!("✓ Utworzono playlistę „{name}”"));
                        }
                    }
                    Err(e) => self.set_status(format!("Błąd tworzenia playlisty: {e}")),
                },
                ApiResult::TrackAddedToPlaylist { playlist_name, res } => match res {
                    Ok(true) => {
                        self.set_status(format!("Już jest na playliście „{playlist_name}”"));
                    }
                    Ok(false) => {
                        self.set_status(format!("✓ Dodano do „{playlist_name}”"));
                        let _ = self.job_tx.send(ApiJob::RefreshPlaylists);
                    }
                    Err(e) => self.set_status(format!("Błąd dodawania: {e}")),
                },
                ApiResult::RadioLoaded(res) => match res {
                    Ok(list) if !list.is_empty() => {
                        let count = list.len();
                        if let Some(cur) = self.queue_index {
                            let keep = (cur + 1).min(self.queue.len());
                            self.queue.truncate(keep);
                            self.queue.extend(list);
                        } else {
                            self.queue = list;
                            self.play_queue_index(0);
                        }
                        self.set_status(format!("📻 Radio gotowe (+{count} utworów w kolejce)"));
                    }
                    Ok(_) => self.set_status("Radio nie znalazło propozycji."),
                    Err(e) => self.set_status(format!("Błąd radia: {e}")),
                },
                ApiResult::LyricsLoaded(data) => {
                    if data.track_id == self.lyrics.track_id || self.lyrics.track_id.is_empty() {
                        self.lyrics = data;
                    }
                }
            }
        }

        let snap = self.player.snapshot();

        // 3. Zliczaj realny czas odsłuchu (do /api/stats/play)
        if let Some(tracker) = self.play_tracker.as_mut() {
            let dt = tracker.last_tick.elapsed().as_secs_f64();
            tracker.last_tick = Instant::now();
            if snap.status == PlaybackStatus::Playing && dt < 2.0 {
                tracker.played_secs += dt;
            }
        }

        // 4. Zmiana stanu (np. Buffering -> Playing / Paused / Stopped) → od razu presence + sync
        if snap.status != self.last_status {
            self.last_status = snap.status.clone();
            match snap.status {
                PlaybackStatus::Playing => {
                    self.last_presence_at = Instant::now();
                    let _ = self
                        .job_tx
                        .send(ApiJob::Presence(snap.track.clone(), true));
                    self.push_sync_state(
                        snap.track.clone(),
                        true,
                        snap.position_secs,
                        snap.duration_secs,
                    );
                }
                PlaybackStatus::Paused | PlaybackStatus::Stopped => {
                    self.last_presence_at = Instant::now();
                    let _ = self.job_tx.send(ApiJob::Presence(None, false));
                    self.push_sync_state(
                        snap.track.clone(),
                        false,
                        snap.position_secs,
                        snap.duration_secs,
                    );
                }
                PlaybackStatus::Buffering => {}
            }
        }

        // 5. Auto-next na końcu utworu + zapis odsłuchu (completed=true)
        if snap.finished_seq != self.last_finished_seq {
            self.last_finished_seq = snap.finished_seq;
            self.flush_play_tracker(true);
            self.next_track();
        }

        // 6. Heartbeat presence co 40 s (TTL serwera = 150 s) + okresowy sync pozycji co 5 s
        if snap.status == PlaybackStatus::Playing {
            if self.last_presence_at.elapsed() >= Duration::from_secs(40) {
                self.last_presence_at = Instant::now();
                let _ = self
                    .job_tx
                    .send(ApiJob::Presence(snap.track.clone(), true));
            }
            if self.last_sync_push_at.elapsed() >= Duration::from_secs(5) {
                self.push_sync_state(
                    snap.track.clone(),
                    true,
                    snap.position_secs,
                    snap.duration_secs,
                );
            }
        }

        snap
    }

    pub fn shutdown(&mut self) {
        self.player.stop();
        if let Some(sync) = self.sync.take() {
            sync.shutdown();
        }
        if let Some(api) = self.api.as_ref() {
            if let Some(tracker) = self.play_tracker.take() {
                let played_s = tracker.played_secs.round() as i64;
                if played_s >= 3 {
                    api.record_play_event(
                        &tracker.track,
                        played_s,
                        false,
                        &self.session_id,
                        tracker.started_at_ms,
                    );
                }
            }
            api.send_presence(None, false);
        }
    }
}

fn unix_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn spawn_api_worker(
    job_rx: Receiver<ApiJob>,
    res_tx: Sender<ApiResult>,
    api_rx_updater: Receiver<ApiClient>,
    mut client: Option<ApiClient>,
) {
    thread::spawn(move || {
        while let Ok(job) = job_rx.recv() {
            while let Ok(updated) = api_rx_updater.try_recv() {
                client = Some(updated);
            }

            match job {
                ApiJob::Login {
                    server,
                    username,
                    password,
                    theme,
                    layout,
                    volume,
                } => {
                    let temp = ApiClient::new(&server, "");
                    match temp.login(&username, &password) {
                        Ok(lr) => {
                            let cfg = SessionConfig {
                                server_url: server,
                                token: lr.token,
                                username: lr.username,
                                is_admin: lr.is_admin,
                                volume,
                                theme,
                                layout,
                                device_id: default_device_id(),
                            };
                            client = Some(ApiClient::new(&cfg.server_url, &cfg.token));
                            let _ = res_tx.send(ApiResult::LoginOk(cfg));
                        }
                        Err(e) => {
                            let _ = res_tx.send(ApiResult::LoginErr(e));
                        }
                    }
                }
                ApiJob::LoadBootstrap => {
                    if let Some(c) = client.as_ref() {
                        let playlists = c.playlists();
                        let liked = c.liked();
                        let _ = res_tx.send(ApiResult::Bootstrap { playlists, liked });
                    }
                }
                ApiJob::RefreshPlaylists => {
                    if let Some(c) = client.as_ref() {
                        let _ = res_tx.send(ApiResult::PlaylistsRefreshed(c.playlists()));
                    }
                }
                ApiJob::LoadPlaylist(pid, name) => {
                    if let Some(c) = client.as_ref() {
                        let _ = res_tx.send(ApiResult::TracksLoaded {
                            title: name,
                            tracks: c.playlist_tracks(pid),
                        });
                    }
                }
                ApiJob::LoadLiked => {
                    if let Some(c) = client.as_ref() {
                        let _ = res_tx.send(ApiResult::TracksLoaded {
                            title: "Polubione".to_string(),
                            tracks: c.liked(),
                        });
                    }
                }
                ApiJob::LoadLocal => {
                    if let Some(c) = client.as_ref() {
                        let _ = res_tx.send(ApiResult::TracksLoaded {
                            title: "Moja muzyka".to_string(),
                            tracks: c.local_tracks(),
                        });
                    }
                }
                ApiJob::Search(q) => {
                    if let Some(c) = client.as_ref() {
                        let _ = res_tx.send(ApiResult::TracksLoaded {
                            title: format!("Wyniki: {q}"),
                            tracks: c.search(&q),
                        });
                    }
                }
                ApiJob::ToggleLike {
                    track,
                    currently_liked,
                } => {
                    if let Some(c) = client.as_ref() {
                        let res = if currently_liked {
                            c.unlike_track(&track.id)
                        } else {
                            c.like_track(&track)
                        };
                        let _ = res_tx.send(ApiResult::LikeToggled {
                            track_id: track.id,
                            liked: !currently_liked,
                            res,
                        });
                    }
                }
                ApiJob::CreatePlaylist {
                    name,
                    cover_color,
                    pending_track,
                } => {
                    if let Some(c) = client.as_ref() {
                        let _ = res_tx.send(ApiResult::PlaylistCreated {
                            res: c.create_playlist(&name, &cover_color),
                            pending_track,
                        });
                    }
                }
                ApiJob::AddToPlaylist {
                    playlist_id,
                    playlist_name,
                    track,
                } => {
                    if let Some(c) = client.as_ref() {
                        let _ = res_tx.send(ApiResult::TrackAddedToPlaylist {
                            playlist_name,
                            res: c.add_track_to_playlist(playlist_id, &track),
                        });
                    }
                }
                ApiJob::LoadRadio(seed) => {
                    if let Some(c) = client.as_ref() {
                        let _ = res_tx.send(ApiResult::RadioLoaded(c.radio(&seed)));
                    }
                }
                ApiJob::LoadLyrics(track) => {
                    if let Some(c) = client.as_ref() {
                        let data = c.lyrics(&track).unwrap_or(LyricsData {
                            track_id: track.id.clone(),
                            found: false,
                            loading: false,
                            ..Default::default()
                        });
                        let _ = res_tx.send(ApiResult::LyricsLoaded(data));
                    }
                }
                ApiJob::Presence(track, playing) => {
                    if let Some(c) = client.as_ref() {
                        c.send_presence(track.as_ref(), playing);
                    }
                }
                ApiJob::RecordPlay {
                    track,
                    played_s,
                    completed,
                    session_id,
                    started_at_ms,
                } => {
                    if let Some(c) = client.as_ref() {
                        c.record_play_event(&track, played_s, completed, &session_id, started_at_ms);
                    }
                }
            }
        }
    });
}
