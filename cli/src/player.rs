use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(not(windows))]
use std::io::Write;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::api::Track;

const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u32 = 2;
const BYTES_PER_FRAME: usize = (CHANNELS as usize) * 2; // s16le stereo = 4 bytes/frame
const CHUNK_FRAMES: usize = 960; // ~20 ms at 48 kHz
const CHUNK_BYTES: usize = CHUNK_FRAMES * BYTES_PER_FRAME;
pub const SPECTRUM_BANDS: usize = 16;

const BAND_FREQS: [f32; SPECTRUM_BANDS] = [
    65.0, 100.0, 150.0, 220.0, 320.0, 460.0, 650.0, 920.0,
    1300.0, 1850.0, 2600.0, 3600.0, 5000.0, 7000.0, 9500.0, 12500.0,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Buffering,
    Playing,
    Paused,
}

#[derive(Clone, Debug)]
pub struct PlayerSnapshot {
    pub status: PlaybackStatus,
    pub track: Option<Track>,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub volume: u8,
    pub level: f32,
    pub bands: [f32; SPECTRUM_BANDS],
    pub finished_seq: u64,
    pub last_error: Option<String>,
}

impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            track: None,
            position_secs: 0.0,
            duration_secs: 0.0,
            volume: 85,
            level: 0.0,
            bands: [0.0; SPECTRUM_BANDS],
            finished_seq: 0,
            last_error: None,
        }
    }
}

enum PlayerCmd {
    Play {
        track: Track,
        url: String,
        start_secs: f64,
    },
    TogglePause,
    Seek(f64),
    Stop,
    Shutdown,
}

pub struct AudioPlayer {
    tx: Sender<PlayerCmd>,
    state: Arc<Mutex<PlayerSnapshot>>,
    volume: Arc<AtomicU8>,
    paused: Arc<AtomicBool>,
    interrupt: Arc<AtomicBool>,
}

impl AudioPlayer {
    pub fn new(initial_volume: u8) -> Self {
        let vol = initial_volume.clamp(0, 100);
        let state = Arc::new(Mutex::new(PlayerSnapshot {
            volume: vol,
            ..Default::default()
        }));
        let volume = Arc::new(AtomicU8::new(vol));
        let paused = Arc::new(AtomicBool::new(false));
        let interrupt = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let st_clone = Arc::clone(&state);
        let vol_clone = Arc::clone(&volume);
        let pause_clone = Arc::clone(&paused);
        let intr_clone = Arc::clone(&interrupt);

        thread::spawn(move || worker_loop(rx, st_clone, vol_clone, pause_clone, intr_clone));

        Self {
            tx,
            state,
            volume,
            paused,
            interrupt,
        }
    }

    pub fn play_at(&self, track: Track, url: String, start_secs: f64, start_paused: bool) {
        self.paused.store(start_paused, Ordering::SeqCst);
        self.interrupt.store(true, Ordering::SeqCst);
        let _ = self.tx.send(PlayerCmd::Play {
            track,
            url,
            start_secs: start_secs.max(0.0),
        });
    }

    pub fn toggle_pause(&self) -> bool {
        let now_paused = !self.paused.load(Ordering::SeqCst);
        self.set_paused(now_paused);
        now_paused
    }

    pub fn set_paused(&self, pause: bool) {
        self.paused.store(pause, Ordering::SeqCst);
        let _ = self.tx.send(PlayerCmd::TogglePause);
    }

    pub fn seek_to(&self, target_secs: f64) {
        let target = {
            let s = self.state.lock().map(|g| g.clone()).unwrap_or_default();
            if s.track.is_none() {
                return;
            }
            let max_d = if s.duration_secs > 1.0 {
                s.duration_secs - 1.0
            } else {
                36000.0
            };
            target_secs.clamp(0.0, max_d)
        };
        self.interrupt.store(true, Ordering::SeqCst);
        let _ = self.tx.send(PlayerCmd::Seek(target));
    }

    pub fn seek_relative(&self, delta_secs: f64) {
        let pos = self.state.lock().map(|g| g.position_secs).unwrap_or(0.0);
        self.seek_to(pos + delta_secs);
    }

    pub fn set_volume(&self, vol: u8) {
        let clamped = vol.clamp(0, 100);
        self.volume.store(clamped, Ordering::Relaxed);
        if let Ok(mut s) = self.state.lock() {
            s.volume = clamped;
        }
    }

    pub fn volume(&self) -> u8 {
        self.volume.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.interrupt.store(true, Ordering::SeqCst);
        let _ = self.tx.send(PlayerCmd::Stop);
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.interrupt.store(true, Ordering::SeqCst);
        let _ = self.tx.send(PlayerCmd::Shutdown);
    }
}

// ============================================================================
// Platform-specific PCM output sink:
// - Linux/Unix: paplay (PipeWire / PulseAudio) — zero -devel dependencies
// - Windows:    cpal (native WASAPI) — zero external audio binaries needed
// ============================================================================

#[cfg(not(windows))]
mod sink {
    use super::*;

    pub struct AudioSink {
        child: Child,
    }

    impl AudioSink {
        pub fn preferred_sample_rate() -> u32 {
            DEFAULT_SAMPLE_RATE
        }

        pub fn open(sample_rate: u32) -> Result<Self, String> {
            let child = Command::new("paplay")
                .args([
                    "--raw",
                    &format!("--rate={sample_rate}"),
                    "--channels=2",
                    "--format=s16le",
                    "--client-name=SoundWave-CLI",
                    "--stream-name=SoundWave",
                    "--latency-msec=80",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Nie udało się uruchomić paplay: {e}"))?;
            Ok(Self { child })
        }

        pub fn write_pcm(&mut self, bytes: &[u8], _interrupt: &AtomicBool) -> bool {
            self.child
                .stdin
                .as_mut()
                .map(|stdin| stdin.write_all(bytes).is_ok())
                .unwrap_or(false)
        }

        pub fn finish(mut self) {
            drop(self.child.stdin.take());
            let _ = self.child.wait();
        }

        pub fn kill(mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(windows)]
mod sink {
    use super::*;
    use std::collections::VecDeque;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream, StreamConfig};

    pub struct AudioSink {
        _stream: Stream,
        queue: Arc<Mutex<VecDeque<i16>>>,
        max_buffered_samples: usize,
    }

    impl AudioSink {
        pub fn preferred_sample_rate() -> u32 {
            cpal::default_host()
                .default_output_device()
                .and_then(|d| d.default_output_config().ok())
                .map(|c| c.sample_rate().0)
                .unwrap_or(DEFAULT_SAMPLE_RATE)
        }

        pub fn open(sample_rate: u32) -> Result<Self, String> {
            let host = cpal::default_host();
            let device = host
                .default_output_device()
                .ok_or_else(|| "Brak domyślnego urządzenia audio (WASAPI)".to_string())?;
            let supported = device
                .default_output_config()
                .map_err(|e| format!("Błąd konfiguracji audio: {e}"))?;
            let out_channels = supported.channels().max(1) as usize;
            let fmt = supported.sample_format();
            let config = StreamConfig {
                channels: supported.channels(),
                sample_rate: cpal::SampleRate(sample_rate),
                buffer_size: cpal::BufferSize::Default,
            };

            let queue: Arc<Mutex<VecDeque<i16>>> =
                Arc::new(Mutex::new(VecDeque::with_capacity(sample_rate as usize / 2)));
            let q_cb = Arc::clone(&queue);
            let err_fn = |_err| {};

            let stream = match fmt {
                SampleFormat::F32 => device.build_output_stream(
                    &config,
                    move |data: &mut [f32], _| {
                        let mut q = q_cb.lock().unwrap_or_else(|e| e.into_inner());
                        for frame in data.chunks_mut(out_channels) {
                            let l = q.pop_front().unwrap_or(0) as f32 / 32768.0;
                            let r = q.pop_front().unwrap_or(0) as f32 / 32768.0;
                            if out_channels == 1 {
                                frame[0] = (l + r) * 0.5;
                            } else {
                                frame[0] = l;
                                frame[1] = r;
                                for extra in &mut frame[2..] {
                                    *extra = 0.0;
                                }
                            }
                        }
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::I16 => device.build_output_stream(
                    &config,
                    move |data: &mut [i16], _| {
                        let mut q = q_cb.lock().unwrap_or_else(|e| e.into_inner());
                        for frame in data.chunks_mut(out_channels) {
                            let l = q.pop_front().unwrap_or(0);
                            let r = q.pop_front().unwrap_or(0);
                            if out_channels == 1 {
                                frame[0] = ((l as i32 + r as i32) / 2) as i16;
                            } else {
                                frame[0] = l;
                                frame[1] = r;
                                for extra in &mut frame[2..] {
                                    *extra = 0;
                                }
                            }
                        }
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::U16 => device.build_output_stream(
                    &config,
                    move |data: &mut [u16], _| {
                        let mut q = q_cb.lock().unwrap_or_else(|e| e.into_inner());
                        for frame in data.chunks_mut(out_channels) {
                            let l = (q.pop_front().unwrap_or(0) as i32 + 32768) as u16;
                            let r = (q.pop_front().unwrap_or(0) as i32 + 32768) as u16;
                            if out_channels == 1 {
                                frame[0] = ((l as u32 + r as u32) / 2) as u16;
                            } else {
                                frame[0] = l;
                                frame[1] = r;
                                for extra in &mut frame[2..] {
                                    *extra = 32768;
                                }
                            }
                        }
                    },
                    err_fn,
                    None,
                ),
                other => return Err(format!("Nieobsługiwany format audio: {other:?}")),
            }
            .map_err(|e| format!("Nie udało się otworzyć strumienia WASAPI: {e}"))?;

            stream
                .play()
                .map_err(|e| format!("Nie udało się uruchomić odtwarzania WASAPI: {e}"))?;

            // Bufor ~120 ms stereo (2 próbki na ramkę)
            let max_buffered_samples = (sample_rate as usize * 2 * 120) / 1000;
            Ok(Self {
                _stream: stream,
                queue,
                max_buffered_samples,
            })
        }

        pub fn write_pcm(&mut self, bytes: &[u8], interrupt: &AtomicBool) -> bool {
            while !interrupt.load(Ordering::SeqCst) {
                let len = self.queue.lock().map(|q| q.len()).unwrap_or(0);
                if len < self.max_buffered_samples {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            if interrupt.load(Ordering::SeqCst) {
                return true;
            }
            if let Ok(mut q) = self.queue.lock() {
                for pair in bytes.chunks_exact(2) {
                    q.push_back(i16::from_le_bytes([pair[0], pair[1]]));
                }
                true
            } else {
                false
            }
        }

        pub fn finish(self) {
            // Pozwól dokończyć ostatnie ~120 ms bufora przed zamknięciem strumienia
            for _ in 0..30 {
                let empty = self.queue.lock().map(|q| q.is_empty()).unwrap_or(true);
                if empty {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        pub fn kill(self) {
            if let Ok(mut q) = self.queue.lock() {
                q.clear();
            }
            drop(self._stream);
        }
    }
}

use sink::AudioSink;

struct ActiveStream {
    track: Track,
    url: String,
    ffmpeg: Child,
    audio_sink: Option<AudioSink>,
    sample_rate: u32,
    base_secs: f64,
    frames_played: u64,
}

impl ActiveStream {
    fn kill_all(&mut self) {
        let _ = self.ffmpeg.kill();
        let _ = self.ffmpeg.wait();
        if let Some(s) = self.audio_sink.take() {
            s.kill();
        }
    }
}

impl Drop for ActiveStream {
    fn drop(&mut self) {
        self.kill_all();
    }
}

fn spawn_ffmpeg(url: &str, start_secs: f64, sample_rate: u32) -> Result<Child, String> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-reconnect",
        "1",
        "-reconnect_streamed",
        "1",
        "-reconnect_delay_max",
        "4",
    ]);
    if start_secs > 0.5 {
        cmd.args(["-ss", &format!("{start_secs:.2}")]);
    }
    cmd.args([
        "-i",
        url,
        "-vn",
        "-f",
        "s16le",
        "-ac",
        "2",
        "-ar",
        &sample_rate.to_string(),
        "pipe:1",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW — brak mignięcia czarnego okna konsoli na Windows
        cmd.creation_flags(0x08000000);
    }

    cmd.spawn().map_err(|e| {
        #[cfg(windows)]
        {
            format!("Brak ffmpeg w PATH ({e}) — zainstaluj: winget install Gyan.FFmpeg")
        }
        #[cfg(not(windows))]
        {
            format!("Nie udało się uruchomić ffmpeg: {e}")
        }
    })
}

fn worker_loop(
    rx: Receiver<PlayerCmd>,
    state: Arc<Mutex<PlayerSnapshot>>,
    volume: Arc<AtomicU8>,
    paused: Arc<AtomicBool>,
    interrupt: Arc<AtomicBool>,
) {
    let sample_rate = AudioSink::preferred_sample_rate();
    let mut current: Option<ActiveStream> = None;
    let mut last_track_url: Option<(Track, String)> = None;
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut smooth_bands = [0.0f32; SPECTRUM_BANDS];
    let mut mono = Vec::with_capacity(CHUNK_FRAMES);

    let band_coeffs: [f32; SPECTRUM_BANDS] = std::array::from_fn(|i| {
        let omega = 2.0 * std::f32::consts::PI * BAND_FREQS[i] / (sample_rate as f32);
        2.0 * omega.cos()
    });

    loop {
        let mut pending = Vec::new();
        if current.is_none() {
            match rx.recv() {
                Ok(cmd) => pending.push(cmd),
                Err(_) => break,
            }
        }
        while let Ok(cmd) = rx.try_recv() {
            pending.push(cmd);
        }

        let mut shutdown = false;
        for cmd in pending {
            match cmd {
                PlayerCmd::Shutdown => {
                    shutdown = true;
                    if let Some(mut s) = current.take() {
                        s.kill_all();
                    }
                    break;
                }
                PlayerCmd::Stop => {
                    interrupt.store(false, Ordering::SeqCst);
                    if let Some(mut s) = current.take() {
                        s.kill_all();
                    }
                    smooth_bands = [0.0; SPECTRUM_BANDS];
                    if let Ok(mut st) = state.lock() {
                        st.status = PlaybackStatus::Stopped;
                        st.level = 0.0;
                        st.bands = [0.0; SPECTRUM_BANDS];
                        st.position_secs = 0.0;
                    }
                }
                PlayerCmd::TogglePause => {
                    if let Some(stream) = current.as_mut() {
                        let is_paused = paused.load(Ordering::SeqCst);
                        if is_paused {
                            if let Some(s) = stream.audio_sink.take() {
                                s.kill();
                            }
                            smooth_bands = [0.0; SPECTRUM_BANDS];
                            if let Ok(mut st) = state.lock() {
                                st.status = PlaybackStatus::Paused;
                                st.level = 0.0;
                                st.bands = [0.0; SPECTRUM_BANDS];
                            }
                        } else if let Ok(mut st) = state.lock() {
                            st.status = PlaybackStatus::Playing;
                        }
                    }
                }
                PlayerCmd::Play {
                    track,
                    url,
                    start_secs,
                } => {
                    interrupt.store(false, Ordering::SeqCst);
                    if let Some(mut old) = current.take() {
                        old.kill_all();
                    }
                    let dur = track.duration_secs().unwrap_or(0.0);
                    last_track_url = Some((track.clone(), url.clone()));
                    if let Ok(mut st) = state.lock() {
                        st.status = PlaybackStatus::Buffering;
                        st.track = Some(track.clone());
                        st.position_secs = start_secs;
                        st.duration_secs = dur;
                        st.level = 0.0;
                        st.last_error = None;
                    }
                    match spawn_ffmpeg(&url, start_secs, sample_rate) {
                        Ok(ffmpeg) => {
                            current = Some(ActiveStream {
                                track,
                                url,
                                ffmpeg,
                                audio_sink: None,
                                sample_rate,
                                base_secs: start_secs,
                                frames_played: 0,
                            });
                        }
                        Err(err) => {
                            if let Ok(mut st) = state.lock() {
                                st.status = PlaybackStatus::Stopped;
                                st.last_error = Some(err);
                            }
                        }
                    }
                }
                PlayerCmd::Seek(target_secs) => {
                    interrupt.store(false, Ordering::SeqCst);
                    if let Some((track, url)) = last_track_url.clone() {
                        if let Some(mut old) = current.take() {
                            old.kill_all();
                        }
                        let dur = track.duration_secs().unwrap_or(0.0);
                        if let Ok(mut st) = state.lock() {
                            st.status = PlaybackStatus::Buffering;
                            st.position_secs = target_secs;
                            st.duration_secs = dur;
                            st.level = 0.0;
                        }
                        if let Ok(ffmpeg) = spawn_ffmpeg(&url, target_secs, sample_rate) {
                            current = Some(ActiveStream {
                                track,
                                url,
                                ffmpeg,
                                audio_sink: None,
                                sample_rate,
                                base_secs: target_secs,
                                frames_played: 0,
                            });
                        }
                    }
                }
            }
        }

        if shutdown {
            break;
        }

        if let Some(stream) = current.as_mut() {
            if paused.load(Ordering::SeqCst) {
                if let Some(s) = stream.audio_sink.take() {
                    s.kill();
                }
                thread::sleep(Duration::from_millis(25));
                continue;
            }

            if interrupt.load(Ordering::SeqCst) {
                continue;
            }

            let read_res = match stream.ffmpeg.stdout.as_mut() {
                Some(stdout) => read_chunk(stdout, &mut buf),
                None => Ok(0),
            };

            if interrupt.load(Ordering::SeqCst) {
                continue;
            }

            match read_res {
                Ok(0) => {
                    let _ = stream.track.id.len();
                    let _ = stream.url.len();
                    if let Some(s) = stream.audio_sink.take() {
                        s.finish();
                    }
                    let _ = stream.ffmpeg.wait();
                    current = None;
                    smooth_bands = [0.0; SPECTRUM_BANDS];
                    if let Ok(mut st) = state.lock() {
                        st.status = PlaybackStatus::Stopped;
                        st.level = 0.0;
                        st.bands = [0.0; SPECTRUM_BANDS];
                        st.finished_seq += 1;
                    }
                }
                Ok(n) => {
                    let aligned = n - (n % BYTES_PER_FRAME);
                    if aligned == 0 {
                        continue;
                    }
                    if stream.audio_sink.is_none() {
                        match AudioSink::open(stream.sample_rate) {
                            Ok(s) => stream.audio_sink = Some(s),
                            Err(err) => {
                                if let Ok(mut st) = state.lock() {
                                    st.status = PlaybackStatus::Stopped;
                                    st.last_error = Some(err);
                                }
                                current = None;
                                continue;
                            }
                        }
                    }

                    let vol_pct = volume.load(Ordering::Relaxed) as f32 / 100.0;
                    let gain = vol_pct.powf(1.6);
                    let mut sum_sq: f64 = 0.0;
                    mono.clear();

                    for frame in buf[..aligned].chunks_exact_mut(4) {
                        let l_raw = i16::from_le_bytes([frame[0], frame[1]]) as f32;
                        let r_raw = i16::from_le_bytes([frame[2], frame[3]]) as f32;
                        let m = ((l_raw + r_raw) * 0.5) / 32768.0;
                        mono.push(m);

                        let l_out = (l_raw * gain).clamp(-32768.0, 32767.0) as i16;
                        let r_out = (r_raw * gain).clamp(-32768.0, 32767.0) as i16;
                        let ln = (l_out as f64) / 32768.0;
                        let rn = (r_out as f64) / 32768.0;
                        sum_sq += ln * ln + rn * rn;

                        let lb = l_out.to_le_bytes();
                        let rb = r_out.to_le_bytes();
                        frame[0] = lb[0];
                        frame[1] = lb[1];
                        frame[2] = rb[0];
                        frame[3] = rb[1];
                    }

                    let sample_count = (aligned / 2).max(1);
                    let rms = ((sum_sq / (sample_count as f64)).sqrt() * 2.8).min(1.0) as f32;

                    let n_win = mono.len().min(512);
                    if n_win >= 64 {
                        let n_f = n_win as f32;
                        for b in 0..SPECTRUM_BANDS {
                            let coeff = band_coeffs[b];
                            let mut s1 = 0.0f32;
                            let mut s2 = 0.0f32;
                            for (idx, &sample) in mono[..n_win].iter().enumerate() {
                                let w = 0.5
                                    * (1.0
                                        - (2.0 * std::f32::consts::PI * (idx as f32) / n_f).cos());
                                let s0 = sample * w + coeff * s1 - s2;
                                s2 = s1;
                                s1 = s0;
                            }
                            let mag = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt() / n_f;
                            let tilt = 1.0 + (b as f32) * 0.28;
                            let target = ((mag * 36.0 * tilt).ln_1p() / 1.9).clamp(0.0, 1.0);
                            if target > smooth_bands[b] {
                                smooth_bands[b] = smooth_bands[b] * 0.35 + target * 0.65;
                            } else {
                                smooth_bands[b] = smooth_bands[b] * 0.78 + target * 0.22;
                            }
                        }
                    }

                    let write_ok = stream
                        .audio_sink
                        .as_mut()
                        .map(|s| s.write_pcm(&buf[..aligned], &interrupt))
                        .unwrap_or(false);

                    if !write_ok {
                        stream.audio_sink = None;
                    }

                    stream.frames_played += (aligned / BYTES_PER_FRAME) as u64;
                    let pos =
                        stream.base_secs + (stream.frames_played as f64 / stream.sample_rate as f64);

                    if let Ok(mut st) = state.lock() {
                        st.status = PlaybackStatus::Playing;
                        st.position_secs = pos;
                        if st.duration_secs < pos {
                            st.duration_secs = pos;
                        }
                        st.level = rms;
                        st.bands = smooth_bands;
                    }
                }
                Err(_) => {
                    current = None;
                    if let Ok(mut st) = state.lock() {
                        st.status = PlaybackStatus::Stopped;
                        st.level = 0.0;
                        st.bands = [0.0; SPECTRUM_BANDS];
                        st.last_error = Some("Przerwany strumień audio".to_string());
                    }
                }
            }
        }
    }
}

fn read_chunk(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}
