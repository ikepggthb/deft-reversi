//! 効果音（着手音・パス音・終局音）を合成・再生するバックグラウンドサービス。
//!
//! 3 種類の効果音はスレッド起動時にサイン波からコード生成されるため、外部音声ファイルや
//! デコーダを必要としない。音声出力デバイスが存在しない環境でもエラーにせず無音で続行する。
use crate::app::Sound;
use rodio::{DeviceSinkBuilder, Float, Source, buffer::SamplesBuffer};
use std::{
    f32::consts::TAU,
    num::NonZero,
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::Duration,
};

/// PCM 合成のサンプリングレート（Hz）。
const RATE: u32 = 44_100;

/// 効果音再生スレッドへの送信ハンドル。
pub struct AudioService {
    commands: mpsc::Sender<(Sound, f32)>,
}

impl AudioService {
    /// 専用の `audio` スレッドを起動し、制御ハンドルを返す。
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

    /// 指定した効果音を非同期で再生する（即座に返る）。`volume` は `0.0..=1.0`。
    pub fn play(&self, sound: Sound, volume: f32) {
        let _ = self.commands.send((sound, volume));
    }
}

/// 出力デバイスを開いたままにすると無音をミキシングし続けて CPU を消費するため、
/// 最後の再生からこの時間アイドル状態が続いたらデバイスを閉じ、次回再生時に開き直す。
const IDLE_CLOSE: Duration = Duration::from_secs(3);

/// オーディオ再生スレッドのメインループ。
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
                    // デバイス不在の警告は初回のみ出力し、以後の再生試行では静かにスキップする。
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

/// 秒数をサンプル数に変換する。
fn seconds(s: f32) -> usize {
    (s * RATE as f32) as usize
}

/// 指数減衰するサイン波トーンを生成する。`decay` は振幅が約 1/3 に減衰するまでの秒数。
fn tone(frequency: f32, length: f32, decay: f32, amplitude: f32) -> Vec<Float> {
    (0..seconds(length))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            // 先頭 2 ms のフェードインを入れてクリックノイズを防ぐ。
            let rise = (t / 0.002).min(1.);
            amplitude * rise * (-t / decay).exp() * (TAU * frequency * t).sin()
        })
        .collect()
}

/// 出力バッファの指定時刻 `at`（秒）の位置に音声波形を加算合成する。
fn mix_into(out: &mut Vec<Float>, at: f32, sound: &[Float]) {
    let start = seconds(at);
    if out.len() < start + sound.len() {
        out.resize(start + sound.len(), 0.);
    }
    for (o, s) in out[start..].iter_mut().zip(sound) {
        *o += s;
    }
}

/// 着手音（盤面に石を打つような短い打撃音）を生成する。
fn place() -> Vec<Float> {
    let mut out = tone(180., 0.09, 0.018, 0.55);
    mix_into(&mut out, 0., &tone(540., 0.04, 0.006, 0.25));
    out
}

/// パス音（柔らかく下降する 2 音）を生成する。
fn pass() -> Vec<Float> {
    let mut out = tone(660., 0.16, 0.06, 0.18);
    mix_into(&mut out, 0.11, &tone(495., 0.2, 0.07, 0.18));
    out
}

/// 終局音（上昇する 3 音のチャイム）を生成する。
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
