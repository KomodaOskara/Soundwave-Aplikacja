use std::time::Duration;

use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: String,
    pub thumbnail: String,
}

impl Track {
    pub fn duration_secs(&self) -> Option<f64> {
        parse_duration(&self.duration)
    }

    pub fn is_local(&self) -> bool {
        self.id.starts_with("local_")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub track_count: usize,
    #[allow(dead_code)]
    pub role: String,
    #[allow(dead_code)]
    pub owner_name: String,
}

#[derive(Clone, Debug)]
pub struct LoginResponse {
    pub token: String,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LyricsData {
    pub track_id: String,
    pub synced: Vec<(f64, String)>,
    pub plain: Vec<String>,
    pub found: bool,
    pub loading: bool,
}

#[derive(Clone)]
pub struct ApiClient {
    pub server_url: String,
    pub token: String,
    agent: ureq::Agent,
}

impl ApiClient {
    pub fn new(server_url: impl Into<String>, token: impl Into<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(8))
            .timeout_read(Duration::from_secs(20))
            .timeout_write(Duration::from_secs(10))
            .user_agent("SoundWave-CLI/0.1")
            .build();
        Self {
            server_url: server_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            agent,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.server_url, path)
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    fn handle_error(err: ureq::Error) -> String {
        match err {
            ureq::Error::Status(code, resp) => {
                if let Ok(v) = resp.into_json::<Value>()
                    && let Some(detail) = v.get("detail").and_then(Value::as_str)
                {
                    return format!("{detail} (HTTP {code})");
                }
                format!("Błąd serwera (HTTP {code})")
            }
            ureq::Error::Transport(t) => format!("Błąd połączenia: {t}"),
        }
    }

    pub fn login(&self, username: &str, password: &str) -> Result<LoginResponse, String> {
        let resp = self
            .agent
            .post(&self.url("/api/auth/login"))
            .send_json(json!({ "username": username, "password": password }))
            .map_err(Self::handle_error)?;
        let v: Value = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        let token = v
            .get("token")
            .and_then(Value::as_str)
            .ok_or_else(|| "Serwer nie zwrócił tokenu".to_string())?
            .to_string();
        let uname = v
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or(username)
            .to_string();
        let is_admin = v.get("is_admin").and_then(Value::as_bool).unwrap_or(false);
        Ok(LoginResponse {
            token,
            username: uname,
            is_admin,
        })
    }

    pub fn logout(&self) {
        if self.token.is_empty() {
            return;
        }
        let _ = self
            .agent
            .post(&self.url("/api/auth/logout"))
            .set("Authorization", &self.auth_header())
            .call();
    }

    pub fn me(&self) -> Result<(String, bool), String> {
        let resp = self
            .agent
            .get(&self.url("/api/auth/me"))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        let v: Value = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        let uname = v
            .get("username")
            .and_then(Value::as_str)
            .ok_or_else(|| "Nieprawidłowa odpowiedź /api/auth/me".to_string())?
            .to_string();
        let is_admin = v.get("is_admin").and_then(Value::as_bool).unwrap_or(false);
        Ok((uname, is_admin))
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>, String> {
        let resp = self
            .agent
            .get(&self.url("/api/playlists"))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        let arr: Vec<Value> = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        Ok(arr
            .into_iter()
            .filter_map(|v| {
                Some(Playlist {
                    id: v.get("id")?.as_i64()?,
                    name: v.get("name")?.as_str()?.to_string(),
                    track_count: v.get("track_count").and_then(Value::as_u64).unwrap_or(0) as usize,
                    role: v
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("owner")
                        .to_string(),
                    owner_name: v
                        .get("owner_name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                })
            })
            .collect())
    }

    pub fn playlist_tracks(&self, pid: i64) -> Result<Vec<Track>, String> {
        let resp = self
            .agent
            .get(&self.url(&format!("/api/playlists/{pid}/tracks")))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        let arr: Vec<Value> = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        Ok(arr.into_iter().filter_map(parse_track_value).collect())
    }

    pub fn create_playlist(&self, name: &str, cover_color: &str) -> Result<Playlist, String> {
        let resp = self
            .agent
            .post(&self.url("/api/playlists"))
            .set("Authorization", &self.auth_header())
            .send_json(json!({ "name": name, "cover_color": cover_color }))
            .map_err(Self::handle_error)?;
        let v: Value = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        let id = v
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| "Serwer nie zwrócił ID playlisty".to_string())?;
        Ok(Playlist {
            id,
            name: v
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(name)
                .to_string(),
            track_count: 0,
            role: "owner".to_string(),
            owner_name: String::new(),
        })
    }

    pub fn add_track_to_playlist(&self, pid: i64, track: &Track) -> Result<bool, String> {
        let resp = self
            .agent
            .post(&self.url(&format!("/api/playlists/{pid}/tracks")))
            .set("Authorization", &self.auth_header())
            .send_json(json!({
                "track_id": track.id,
                "title": track.title,
                "artist": track.artist,
                "duration": track.duration,
                "thumbnail": track.thumbnail,
            }))
            .map_err(Self::handle_error)?;
        let v: Value = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        Ok(v.get("duplicate").and_then(Value::as_bool).unwrap_or(false))
    }

    pub fn record_play_event(
        &self,
        track: &Track,
        played_s: i64,
        completed: bool,
        session_id: &str,
        started_at_ms: i64,
    ) {
        if self.token.is_empty() || played_s < 1 {
            return;
        }
        let duration_s = track.duration_secs().unwrap_or(0.0).round() as i64;
        let (local_date, local_hour, local_dow) = local_calendar_parts();
        let body = json!({
            "events": [{
                "track_id": track.id,
                "title": track.title,
                "artist": track.artist,
                "duration_s": duration_s,
                "played_s": played_s,
                "completed": completed,
                "source": "cli",
                "source_id": Value::Null,
                "device": "cli",
                "session_id": session_id,
                "local_date": local_date,
                "local_hour": local_hour,
                "local_dow": local_dow,
                "started_at_ms": started_at_ms,
            }]
        });
        let _ = self
            .agent
            .post(&self.url("/api/stats/play"))
            .set("Authorization", &self.auth_header())
            .send_json(body);
    }

    pub fn liked(&self) -> Result<Vec<Track>, String> {
        let resp = self
            .agent
            .get(&self.url("/api/liked"))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        let arr: Vec<Value> = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        Ok(arr.into_iter().filter_map(parse_track_value).collect())
    }

    pub fn like_track(&self, track: &Track) -> Result<(), String> {
        self.agent
            .post(&self.url("/api/liked"))
            .set("Authorization", &self.auth_header())
            .send_json(json!({
                "track_id": track.id,
                "title": track.title,
                "artist": track.artist,
                "duration": track.duration,
                "thumbnail": track.thumbnail,
            }))
            .map_err(Self::handle_error)?;
        Ok(())
    }

    pub fn unlike_track(&self, track_id: &str) -> Result<(), String> {
        self.agent
            .delete(&self.url(&format!("/api/liked/{}", encode_uri(track_id))))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        Ok(())
    }

    pub fn local_tracks(&self) -> Result<Vec<Track>, String> {
        let resp = self
            .agent
            .get(&self.url("/api/local?scope=mine"))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        let arr: Vec<Value> = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        Ok(arr.into_iter().filter_map(parse_track_value).collect())
    }

    pub fn search(&self, query: &str) -> Result<Vec<Track>, String> {
        let resp = self
            .agent
            .get(&self.url(&format!("/api/search?q={}", encode_uri(query))))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        let v: Value = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        let arr = v
            .get("tracks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(arr.into_iter().filter_map(parse_track_value).collect())
    }

    pub fn radio(&self, seed: &Track) -> Result<Vec<Track>, String> {
        let path = format!(
            "/api/radio?title={}&artist={}&seed_id={}&algo=mix",
            encode_uri(&seed.title),
            encode_uri(&seed.artist),
            encode_uri(&seed.id)
        );
        let resp = self
            .agent
            .get(&self.url(&path))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        let v: Value = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        let arr = v
            .get("tracks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(arr.into_iter().filter_map(parse_track_value).collect())
    }

    pub fn lyrics(&self, track: &Track) -> Result<LyricsData, String> {
        let path = format!(
            "/api/lyrics?title={}&artist={}",
            encode_uri(&track.title),
            encode_uri(&track.artist)
        );
        let resp = self
            .agent
            .get(&self.url(&path))
            .set("Authorization", &self.auth_header())
            .call()
            .map_err(Self::handle_error)?;
        let v: Value = resp.into_json().map_err(|e| format!("Błąd JSON: {e}"))?;
        let synced_raw = v.get("syncedLyrics").and_then(Value::as_str).unwrap_or("");
        let plain_raw = v.get("plainLyrics").and_then(Value::as_str).unwrap_or("");

        let synced = parse_lrc(synced_raw);
        let plain: Vec<String> = if plain_raw.trim().is_empty() {
            Vec::new()
        } else {
            plain_raw.lines().map(|l| l.to_string()).collect()
        };
        let found = !synced.is_empty() || !plain.is_empty();

        Ok(LyricsData {
            track_id: track.id.clone(),
            synced,
            plain,
            found,
            loading: false,
        })
    }

    pub fn send_presence(&self, track: Option<&Track>, playing: bool) {
        if self.token.is_empty() {
            return;
        }
        let body = match track {
            Some(t) if playing => json!({
                "title": t.title,
                "artist": t.artist,
                "thumbnail": t.thumbnail,
                "playing": true,
            }),
            _ => json!({ "title": "", "artist": "", "thumbnail": "", "playing": false }),
        };
        let _ = self
            .agent
            .post(&self.url("/api/presence"))
            .set("Authorization", &self.auth_header())
            .send_json(body);
    }

    pub fn stream_url(&self, track: &Track) -> String {
        let enc_id = encode_uri(&track.id);
        let enc_t = encode_uri(&self.token);
        if track.is_local() {
            format!("{}/api/stream/local/{}?t={}", self.server_url, enc_id, enc_t)
        } else {
            format!("{}/api/stream/{}?t={}", self.server_url, enc_id, enc_t)
        }
    }
}

pub fn parse_track_value(v: Value) -> Option<Track> {
    let id = v
        .get("track_id")
        .or_else(|| v.get("id"))
        .and_then(Value::as_str)?
        .to_string();
    if id.is_empty() {
        return None;
    }
    let title = v
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Bez tytułu")
        .to_string();
    let artist = v
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or("Nieznany wykonawca")
        .to_string();
    let duration = v
        .get("duration")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let thumbnail = v
        .get("thumbnail")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(Track {
        id,
        title,
        artist,
        duration,
        thumbnail,
    })
}

pub fn parse_duration(s: &str) -> Option<f64> {
    let parts: Vec<f64> = s
        .trim()
        .split(':')
        .filter_map(|p| p.parse::<f64>().ok())
        .collect();
    match parts.as_slice() {
        [m, sec] => Some(m * 60.0 + sec),
        [h, m, sec] => Some(h * 3600.0 + m * 60.0 + sec),
        _ => None,
    }
}

fn parse_lrc(raw: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let mut rest = line.trim();
        let mut timestamps = Vec::new();
        // Obsługa wielu znaczników czasu w jednej linii np. [00:12.30][01:05.10]Tekst
        while rest.starts_with('[') {
            let Some(close) = rest.find(']') else {
                break;
            };
            let ts = &rest[1..close];
            if let Some((m_str, s_str)) = ts.split_once(':')
                && let (Ok(m), Ok(s)) = (m_str.trim().parse::<f64>(), s_str.trim().parse::<f64>())
            {
                timestamps.push(m * 60.0 + s);
                rest = rest[close + 1..].trim_start();
            } else {
                break;
            }
        }
        if timestamps.is_empty() {
            continue;
        }
        let text = rest.trim().to_string();
        for t in timestamps {
            out.push((t, text.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn encode_uri(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn local_calendar_parts() -> (String, i32, i32) {
    let now = Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let hour = now.hour() as i32;
    let dow = now.weekday().num_days_from_monday() as i32; // 0=pon..6=niedz
    (date, hour, dow)
}
