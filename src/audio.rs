//! Alert sounds on a small thread. Built-ins are synthesised (no bundled files); user MP3/WAV files
//! play through rodio. The output device is opened only while something plays and closed after 10 s.

use crate::config::Sound;
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::num::NonZero;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub const BUILTINS: [&str; 4] = ["Siren", "Meow", "Beep beep", "Air horn"];
const RATE: u32 = 44_100;

pub enum Cmd {
    /// Short soft chime for warnings.
    Warn { volume: f32 },
    Alarm { sound: Sound, volume: f32, looping: bool },
    Stop,
    /// Soft sine note (calibration beats); `volume` 0 just opens the device ahead of time.
    Note { freq: f32, volume: f32 },
}

fn tone(out: &mut Vec<f32>, secs: f32, f: impl Fn(f32) -> f32, amp: impl Fn(f32) -> f32) {
    let n = (secs * RATE as f32) as usize;
    let mut phase = 0f32;
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        phase += std::f32::consts::TAU * f(t) / RATE as f32;
        out.push(phase.sin() * amp(t / secs));
    }
}

/// Attack/release envelope so tones don't click.
fn env(x: f32) -> f32 {
    (x / 0.05).min(1.0) * ((1.0 - x) / 0.1).min(1.0)
}

fn synth(name: &str) -> Vec<f32> {
    let mut v = Vec::new();
    match name {
        "Meow" => {
            for _ in 0..2 {
                // Rising then falling glide with vibrato and a nasal second harmonic.
                let n0 = v.len();
                tone(&mut v, 0.55, |t| 520.0 + 380.0 * (t / 0.55 * std::f32::consts::PI).sin() + 12.0 * (t * 38.0).sin(), env);
                let mut h = Vec::new();
                tone(&mut h, 0.55, |t| 2.0 * (520.0 + 380.0 * (t / 0.55 * std::f32::consts::PI).sin()), env);
                for (a, b) in v[n0..].iter_mut().zip(h) {
                    *a = 0.7 * *a + 0.3 * b;
                }
                tone(&mut v, 0.25, |_| 0.0, |_| 0.0);
            }
        }
        "Beep beep" => {
            for _ in 0..3 {
                tone(&mut v, 0.14, |_| 1760.0, env);
                tone(&mut v, 0.09, |_| 0.0, |_| 0.0);
            }
            tone(&mut v, 0.3, |_| 0.0, |_| 0.0);
        }
        "Air horn" => {
            for len in [0.25, 0.25, 0.7] {
                let n0 = v.len();
                tone(&mut v, len, |_| 466.0, env);
                let mut h = Vec::new();
                tone(&mut h, len, |_| 699.0, env);
                for (a, b) in v[n0..].iter_mut().zip(h) {
                    // Square-ish saturation makes it brassy.
                    *a = ((*a + b) * 1.8).clamp(-0.9, 0.9);
                }
                tone(&mut v, 0.06, |_| 0.0, |_| 0.0);
            }
        }
        "Chime" => {
            tone(&mut v, 0.18, |_| 988.0, |x| (1.0 - x) * (x / 0.05).min(1.0) * 0.6);
            tone(&mut v, 0.4, |_| 1319.0, |x| (1.0 - x) * (x / 0.05).min(1.0) * 0.6);
        }
        // "Siren" and unknown names.
        _ => {
            for _ in 0..2 {
                tone(&mut v, 0.45, |t| 650.0 + 600.0 * t / 0.45, env);
                tone(&mut v, 0.45, |t| 1250.0 - 600.0 * t / 0.45, env);
            }
        }
    }
    v
}

fn buffer(name: &str, volume: f32) -> SamplesBuffer {
    let data: Vec<f32> = synth(name).into_iter().map(|s| s * volume).collect();
    SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(RATE).unwrap(), data)
}

struct Out {
    /// Keeps the output device open while `player` is alive.
    _sink: MixerDeviceSink,
    player: Player,
}

fn open() -> Option<Out> {
    let mut sink = DeviceSinkBuilder::open_default_sink().ok()?;
    sink.log_on_drop(false);
    let player = Player::connect_new(sink.mixer());
    Some(Out { _sink: sink, player })
}

fn queue(out: &Out, sound: &Sound, volume: f32) {
    match sound {
        Sound::Builtin(name) => out.player.append(buffer(name, volume)),
        Sound::File(path) => {
            match std::fs::File::open(path).ok().and_then(|f| Decoder::new(std::io::BufReader::new(f)).ok()) {
                Some(d) => out.player.append(d.amplify(volume)),
                // Missing/broken file: never stay silent on an alarm.
                None => out.player.append(buffer("Siren", volume)),
            }
        }
    }
}

pub fn spawn(rx: Receiver<Cmd>) {
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let mut out: Option<Out> = None;
            let mut looping: Option<(Sound, f32)> = None;
            let mut idle_since = Instant::now();
            loop {
                let wait = if out.is_some() { Duration::from_millis(200) } else { Duration::from_secs(3600) };
                match rx.recv_timeout(wait) {
                    Ok(Cmd::Warn { volume }) => {
                        if looping.is_none() {
                            if out.is_none() {
                                out = open();
                            }
                            if let Some(o) = &out {
                                o.player.append(buffer("Chime", volume));
                            }
                        }
                    }
                    Ok(Cmd::Alarm { sound, volume, looping: l }) => {
                        if out.is_none() {
                            out = open();
                        }
                        if let Some(o) = &out {
                            o.player.clear();
                            o.player.play();
                            queue(o, &sound, volume);
                        }
                        looping = l.then_some((sound, volume));
                    }
                    Ok(Cmd::Stop) => {
                        looping = None;
                        if let Some(o) = &out {
                            o.player.clear();
                        }
                    }
                    Ok(Cmd::Note { freq, volume }) => {
                        idle_since = Instant::now();
                        if out.is_none() {
                            out = open();
                        }
                        if let Some(o) = &out {
                            if looping.is_none() && volume > 0.0 {
                                let mut data = Vec::new();
                                // Like the dashboard's WebAudio tone: quick attack, ~0.24 s decay.
                                tone(&mut data, 0.27, |_| freq, |x| (x / 0.045).min(1.0) * (1.0 - x).powi(3) * 0.12 * volume);
                                o.player.append(SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(RATE).unwrap(), data));
                            }
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                if let Some(o) = &out {
                    if o.player.empty() {
                        if let Some((sound, volume)) = &looping {
                            // Short gap between repeats.
                            o.player.append(SamplesBuffer::new(
                                NonZero::new(1).unwrap(),
                                NonZero::new(RATE).unwrap(),
                                vec![0f32; RATE as usize / 2],
                            ));
                            queue(o, sound, *volume);
                        } else if idle_since.elapsed() > Duration::from_secs(10) {
                            out = None;
                        }
                    } else {
                        idle_since = Instant::now();
                    }
                }
            }
        })
        .expect("spawn audio thread");
}

pub type Tx = Sender<Cmd>;
