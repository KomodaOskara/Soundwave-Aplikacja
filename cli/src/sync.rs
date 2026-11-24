use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::api::{parse_track_value, Track};

#[derive(Clone, Debug)]
pub enum SyncInbound {
    Connected(bool),
    DeviceCount(usize),
    CmdPlay,
    CmdPause,
    CmdNext,
    CmdPrev,
    CmdSeek(f64),
    CmdVolume(u8),
    CmdPlayTrack {
        track: Track,
        queue: Vec<Track>,
        idx: usize,
    },
    CmdTransferTo {
        target_device_id: String,
    },
    TakeOver {
        track: Track,
        position: f64,
        queue: Vec<Track>,
        q_idx: usize,
        is_playing: bool,
        from_name: String,
    },
}

#[derive(Clone, Debug)]
pub enum SyncOutbound {
    PlaybackState {
        track: Option<Track>,
        is_playing: bool,
        position: f64,
        duration: f64,
        queue: Vec<Track>,
        q_idx: usize,
    },
    Transfer {
        target_device_id: String,
        track: Option<Track>,
        position: f64,
        queue: Vec<Track>,
        q_idx: usize,
        is_playing: bool,
    },
    Shutdown,
}

pub struct SyncClient {
    out_tx: Sender<SyncOutbound>,
    pub in_rx: Receiver<SyncInbound>,
    stop: Arc<AtomicBool>,
}

impl SyncClient {
    pub fn start(server_url: String, token: String, device_id: String, device_name: String) -> Self {
        let (out_tx, out_rx) = mpsc::channel();
        let (in_tx, in_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        thread::spawn(move || {
            sync_worker(
                server_url,
                token,
                device_id,
                device_name,
                out_rx,
                in_tx,
                stop_clone,
            );
        });

        Self {
            out_tx,
            in_rx,
            stop,
        }
    }

    pub fn send(&self, msg: SyncOutbound) {
        let _ = self.out_tx.send(msg);
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.out_tx.send(SyncOutbound::Shutdown);
    }
}

impl Drop for SyncClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn ws_url(server_url: &str, token: &str) -> String {
    let base = server_url.trim_end_matches('/');
    let ws_base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("wss://{base}")
    };
    format!("{ws_base}/ws/sync?t={token}")
}

fn configure_read_timeout(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    let timeout = Some(Duration::from_millis(40));
    match ws.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(timeout);
        }
        MaybeTlsStream::Rustls(tls) => {
            let _ = tls.get_mut().set_read_timeout(timeout);
        }
        _ => {}
    }
}

fn track_to_json(t: &Track) -> Value {
    json!({
        "id": t.id,
        "track_id": t.id,
        "title": t.title,
        "artist": t.artist,
        "duration": t.duration,
        "thumbnail": t.thumbnail,
    })
}

fn sync_worker(
    server_url: String,
    token: String,
    device_id: String,
    device_name: String,
    out_rx: Receiver<SyncOutbound>,
    in_tx: Sender<SyncInbound>,
    stop: Arc<AtomicBool>,
) {
    let url = ws_url(&server_url, &token);
    let mut last_state: Option<SyncOutbound> = None;

    while !stop.load(Ordering::Relaxed) {
        let conn = tungstenite::connect(&url);
        let Ok((mut ws, _)) = conn else {
            let _ = in_tx.send(SyncInbound::Connected(false));
            for _ in 0..40 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                while let Ok(msg) = out_rx.try_recv() {
                    if matches!(msg, SyncOutbound::Shutdown) {
                        return;
                    }
                    if matches!(msg, SyncOutbound::PlaybackState { .. }) {
                        last_state = Some(msg);
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
            continue;
        };

        // Rejestracja urządzenia w /ws/sync (dokładnie tak jak robi frontend/index.html)
        let reg = json!({
            "type": "register",
            "device_id": device_id,
            "name": device_name,
            "device_type": "desktop",
        });
        if ws.send(Message::Text(reg.to_string().into())).is_err() {
            thread::sleep(Duration::from_secs(3));
            continue;
        }

        // Wyślij od razu bieżący stan po (re)connect, żeby inne urządzenia widziały co gra
        if let Some(ref st) = last_state {
            let _ = send_outbound(&mut ws, st, &device_name);
        }

        configure_read_timeout(&mut ws);
        let _ = in_tx.send(SyncInbound::Connected(true));
        let mut last_ping = Instant::now();

        loop {
            if stop.load(Ordering::Relaxed) {
                let _ = ws.close(None);
                return;
            }

            // 1. Obsłuż wiadomości wychodzące z aplikacji
            let mut disconnected = false;
            while let Ok(msg) = out_rx.try_recv() {
                if matches!(msg, SyncOutbound::Shutdown) {
                    let _ = ws.close(None);
                    return;
                }
                if matches!(msg, SyncOutbound::PlaybackState { .. }) {
                    last_state = Some(msg.clone());
                }
                if send_outbound(&mut ws, &msg, &device_name).is_err() {
                    disconnected = true;
                    break;
                }
            }
            if disconnected {
                break;
            }

            // 2. Keepalive ping co 25 s
            if last_ping.elapsed() >= Duration::from_secs(25) {
                last_ping = Instant::now();
                if ws
                    .send(Message::Text(json!({"type": "ping"}).to_string().into()))
                    .is_err()
                {
                    break;
                }
            }

            // 3. Odbierz wiadomości przychodzące z serwera (non-blocking)
            loop {
                match ws.read() {
                    Ok(Message::Text(txt)) => {
                        if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                            handle_ws_json(v, &in_tx);
                        }
                    }
                    Ok(Message::Ping(p)) => {
                        let _ = ws.send(Message::Pong(p));
                    }
                    Ok(Message::Close(_)) => {
                        disconnected = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(ref e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        break;
                    }
                    Err(_) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                break;
            }
        }

        let _ = in_tx.send(SyncInbound::Connected(false));
        for _ in 0..30 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn send_outbound(
    ws: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    msg: &SyncOutbound,
    device_name: &str,
) -> Result<(), tungstenite::Error> {
    let payload = match msg {
        SyncOutbound::PlaybackState {
            track,
            is_playing,
            position,
            duration,
            queue,
            q_idx,
        } => {
            let q_json: Vec<Value> = queue.iter().take(50).map(track_to_json).collect();
            json!({
                "type": "playback_state",
                "track": track.as_ref().map(track_to_json),
                "is_playing": is_playing,
                "position": position,
                "duration": duration,
                "queue": q_json,
                "qIdx": q_idx,
            })
        }
        SyncOutbound::Transfer {
            target_device_id,
            track,
            position,
            queue,
            q_idx,
            is_playing,
        } => {
            let q_json: Vec<Value> = queue.iter().take(50).map(track_to_json).collect();
            json!({
                "type": "transfer",
                "target_device_id": target_device_id,
                "track": track.as_ref().map(track_to_json),
                "position": position,
                "queue": q_json,
                "qIdx": q_idx,
                "is_playing": is_playing,
                "from_name": device_name,
            })
        }
        SyncOutbound::Shutdown => return Ok(()),
    };
    ws.send(Message::Text(payload.to_string().into()))
}

fn handle_ws_json(v: Value, in_tx: &Sender<SyncInbound>) {
    let Some(msg_type) = v.get("type").and_then(Value::as_str) else {
        return;
    };

    match msg_type {
        "device_list" => {
            let count = v
                .get("devices")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            let _ = in_tx.send(SyncInbound::DeviceCount(count));
        }
        "take_over" => {
            let Some(track) = v.get("track").cloned().and_then(parse_track_value) else {
                return;
            };
            let position = v.get("position").and_then(Value::as_f64).unwrap_or(0.0);
            let queue: Vec<Track> = v
                .get("queue")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .cloned()
                        .filter_map(parse_track_value)
                        .collect()
                })
                .unwrap_or_default();
            let q_idx = v.get("qIdx").and_then(Value::as_u64).unwrap_or(0) as usize;
            let is_playing = v.get("is_playing").and_then(Value::as_bool).unwrap_or(true);
            let from_name = v
                .get("from_name")
                .and_then(Value::as_str)
                .unwrap_or("innego urządzenia")
                .to_string();
            let _ = in_tx.send(SyncInbound::TakeOver {
                track,
                position,
                queue,
                q_idx,
                is_playing,
                from_name,
            });
        }
        "command" => {
            let Some(cmd) = v.get("cmd").and_then(Value::as_str) else {
                return;
            };
            match cmd {
                "play" => {
                    let _ = in_tx.send(SyncInbound::CmdPlay);
                }
                "pause" => {
                    let _ = in_tx.send(SyncInbound::CmdPause);
                }
                "next" => {
                    let _ = in_tx.send(SyncInbound::CmdNext);
                }
                "prev" => {
                    let _ = in_tx.send(SyncInbound::CmdPrev);
                }
                "seek" => {
                    if let Some(pos) = v.get("position").and_then(Value::as_f64) {
                        let _ = in_tx.send(SyncInbound::CmdSeek(pos.max(0.0)));
                    }
                }
                "volume" => {
                    if let Some(val) = v.get("value").and_then(Value::as_f64) {
                        let pct = (val.clamp(0.0, 1.0) * 100.0).round() as u8;
                        let _ = in_tx.send(SyncInbound::CmdVolume(pct));
                    }
                }
                "play_track" => {
                    if let Some(track) = v.get("track").cloned().and_then(parse_track_value) {
                        let queue: Vec<Track> = v
                            .get("queue")
                            .and_then(Value::as_array)
                            .map(|arr| {
                                arr.iter()
                                    .cloned()
                                    .filter_map(parse_track_value)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let idx = v.get("idx").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let _ = in_tx.send(SyncInbound::CmdPlayTrack { track, queue, idx });
                    }
                }
                "transfer_to" => {
                    if let Some(to) = v.get("to").and_then(Value::as_str) {
                        let _ = in_tx.send(SyncInbound::CmdTransferTo {
                            target_device_id: to.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}
