//! Sound effects (§9.4). The three sounds are made in code when the thread
//! starts, so nothing is decoded or shipped. Without an output device the
//! game goes on silently.
use crate::app::Sound;
use rodio::{DeviceSinkBuilder, Float, Source, buffer::SamplesBuffer};
use std::{
    f32::consts::TAU,
    num::NonZero,
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::Duration,
};

const RATE: u32 = 44_100;

pub struct AudioService {
    commands: mpsc::Sender<(Sound, f32)>,
}

impl AudioService {
    pub fn spawn() -> Self {
        let (commands, receiver) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("audio".into())
            .spawn(move || run(receiver));
        if let Err(error) = spawned {
            eprintln!("audio thread: {error}");
        }
        Self { commands }
    }

    /// Returns at once; `volume` is 0 to 1.
    pub fn play(&self, sound: Sound, volume: f32) {
        let _ = self.commands.send((sound, volume));
    }
}

/// An open output mixes silence and costs CPU, so it closes this long
/// after the last sound and opens again for the next.
const IDLE_CLOSE: Duration = Duration::from_secs(3);

fn run(commands: mpsc::Receiver<(Sound, f32)>) {
    let place = place();
    let pass = pass();
    let game_over = game_over();
    let channels = NonZero::new(1).expect("one channel");
    let rate = NonZero::new(RATE).expect("a rate");
    let mut sink = None;
    let mut reported = false;
    loop {
        let command = if sink.is_some() {
            match commands.recv_timeout(IDLE_CLOSE) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => {
                    sink = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match commands.recv() {
                Ok(command) => command,
                Err(_) => return,
            }
        };
        let (sound, volume) = command;
        if sink.is_none() {
            sink = match DeviceSinkBuilder::open_default_sink() {
                Ok(mut opened) => {
                    opened.log_on_drop(false);
                    Some(opened)
                }
                Err(error) => {
                    // Said once; later sounds try again quietly.
                    if !std::mem::replace(&mut reported, true) {
                        eprintln!("[audio] no output: {error}; sounds are off");
                    }
                    None
                }
            };
        }
        let Some(sink) = &sink else {
            continue;
        };
        let samples = match sound {
            Sound::Place => &place,
            Sound::Pass => &pass,
            Sound::GameOver => &game_over,
        };
        let source = SamplesBuffer::new(channels, rate, samples.clone());
        sink.mixer().add(source.amplify(volume.clamp(0., 1.)));
    }
}

fn seconds(s: f32) -> usize {
    (s * RATE as f32) as usize
}

/// A tone that dies away: `decay` is the time to fall to about a third.
fn tone(frequency: f32, length: f32, decay: f32, amplitude: f32) -> Vec<Float> {
    (0..seconds(length))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            // A 2 ms rise avoids a click at the start.
            let rise = (t / 0.002).min(1.);
            amplitude * rise * (-t / decay).exp() * (TAU * frequency * t).sin()
        })
        .collect()
}

fn mix_into(out: &mut Vec<Float>, at: f32, sound: &[Float]) {
    let start = seconds(at);
    if out.len() < start + sound.len() {
        out.resize(start + sound.len(), 0.);
    }
    for (o, s) in out[start..].iter_mut().zip(sound) {
        *o += s;
    }
}

/// A burst of noise around `center` Hz that dies away within `decay`:
/// the dry snap of a hard edge meeting the board. The noise is the same
/// every time, so the sound is too.
fn snap(center: f32, q: f32, length: f32, decay: f32) -> Vec<Float> {
    let mut seed: u32 = 0x9e37_79b9;
    let mut noise = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.
    };
    // A band-pass biquad, so the snap has a pitch of its own.
    let w = TAU * center / RATE as f32;
    let alpha = w.sin() / (2. * q);
    let a0 = 1. + alpha;
    let (b0, b2) = (alpha / a0, -alpha / a0);
    let (a1, a2) = (-2. * w.cos() / a0, (1. - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0., 0., 0., 0.);
    (0..seconds(length))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let rise = (t / 0.0005).min(1.);
            let x = noise() * rise * (-t / decay).exp();
            let y = b0 * x + b2 * x2 - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, x, y1, y);
            y
        })
        .collect()
}

/// Scales a sound so its loudest sample is `peak`.
fn normalized(mut sound: Vec<Float>, peak: f32) -> Vec<Float> {
    let loudest = sound.iter().fold(0f32, |m, s| m.max(s.abs()));
    if loudest > 0. {
        for s in &mut sound {
            *s *= peak / loudest;
        }
    }
    sound
}

/// "Pata": a flat disc slapped down. A dry snap with a short hollow body,
/// and a softer tap a moment later as the disc settles.
fn place() -> Vec<Float> {
    let mut out = snap(2400., 1.2, 0.05, 0.006);
    mix_into(&mut out, 0., &tone(320., 0.05, 0.010, 0.35));
    mix_into(&mut out, 0., &tone(1100., 0.02, 0.003, 0.15));
    let settle: Vec<Float> = snap(3000., 1.5, 0.03, 0.004)
        .into_iter()
        .map(|s| s * 0.35)
        .collect();
    mix_into(&mut out, 0.022, &settle);
    normalized(out, 0.8)
}

/// Two soft falling notes.
fn pass() -> Vec<Float> {
    let mut out = tone(660., 0.16, 0.06, 0.18);
    mix_into(&mut out, 0.11, &tone(495., 0.2, 0.07, 0.18));
    out
}

/// A rising three-note chime.
fn game_over() -> Vec<Float> {
    let mut out = Vec::new();
    for (i, frequency) in [523.25, 659.25, 783.99].into_iter().enumerate() {
        mix_into(&mut out, i as f32 * 0.12, &tone(frequency, 0.5, 0.18, 0.16));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_are_short_and_never_clip() {
        for sound in [place(), pass(), game_over()] {
            assert!(!sound.is_empty());
            assert!(sound.len() <= seconds(0.8));
            assert!(sound.iter().all(|s| s.abs() <= 1.));
        }
        // Playing with no device, or after the thread ended, does nothing.
        AudioService::spawn().play(Sound::Place, 0.0);
    }
}
