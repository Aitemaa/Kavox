//! M0 spike (throwaway): proves clips + mic reach voice apps through the virtual device, and measures latency.

mod engine;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;

use std::io::BufRead;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rtrb::RingBuffer;

use engine::{Command, MicIn, Mixer};
use platform::Platform;

const USAGE: &str = "usage:
  spike run <audio files...> [--device NAME] [--no-mic]
  spike probe [--device NAME] [--trials N]";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let mut files = Vec::new();
    let mut device = None;
    let mut mic = true;
    let mut trials = 10;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => device = args.next(),
            "--no-mic" => mic = false,
            "--trials" => trials = args.next().and_then(|n| n.parse().ok()).unwrap_or(trials),
            _ => files.push(arg),
        }
    }
    match mode.as_str() {
        "run" if !files.is_empty() => run(&files, device.as_deref(), mic),
        "probe" => probe(device.as_deref(), trials),
        _ => bail!("{USAGE}"),
    }
}

fn run(files: &[String], device: Option<&str>, mic: bool) -> Result<()> {
    let mut platform = Platform::open(device)?;
    let clips = files
        .iter()
        .map(|f| engine::load_clip(Path::new(f), platform.out_rate).map(Arc::new))
        .collect::<Result<Vec<_>>>()?;

    let (mut commands, command_rx) = RingBuffer::new(64);
    let (mic_tx, mic_rx) = RingBuffer::new(platform.mic_rate as usize / 5);
    let mixer = Mixer::new(
        command_rx,
        Some(MicIn::new(mic_rx, platform.mic_rate, platform.out_rate)),
    );
    platform.start(mixer, Some(mic_tx))?;
    let mut mic_on = mic;
    if !mic_on {
        let _ = commands.push(Command::SetMicEnabled(false));
    }

    println!(
        "\nSelect \"{}\" as the microphone in your voice app.\n",
        platform.virtual_mic_name()
    );
    for (i, clip) in clips.iter().enumerate() {
        let secs = clip.samples.len() as f32 / 2.0 / platform.out_rate as f32;
        println!("  {}  {} ({secs:.1} s)", i + 1, clip.name);
    }
    println!("\nType a number + Enter to play, s = stop all, m = toggle mic, q = quit.");

    for line in std::io::stdin().lock().lines() {
        match line?.trim() {
            "q" => break,
            "s" => {
                let _ = commands.push(Command::StopAll);
            }
            "m" => {
                mic_on = !mic_on;
                let _ = commands.push(Command::SetMicEnabled(mic_on));
                println!("mic {}", if mic_on { "on" } else { "off" });
            }
            n => match n
                .parse::<usize>()
                .ok()
                .and_then(|n| clips.get(n.wrapping_sub(1)))
            {
                Some(clip) => {
                    let _ = commands.push(Command::Play(clip.clone()));
                }
                None => println!("unknown command: {n}"),
            },
        }
    }
    Ok(())
}

fn probe(device: Option<&str>, trials: usize) -> Result<()> {
    let mut platform = Platform::open(device)?;
    let beep = Arc::new(engine::beep(platform.out_rate));
    let (mut commands, command_rx) = RingBuffer::new(64);
    platform.start(Mixer::new(command_rx, None), None)?;

    let armed = Arc::new(AtomicBool::new(false));
    let (onset_tx, onset_rx) = mpsc::channel();
    let detector_armed = armed.clone();
    platform.record_virtual(Box::new(
        move |data: &[f32], channels: usize, rate: u32, received: Instant| {
            if !detector_armed.load(Ordering::Acquire) {
                return;
            }
            if let Some(frame) = engine::find_onset(data, channels, 0.3) {
                detector_armed.store(false, Ordering::Release);
                let after =
                    Duration::from_secs_f64((data.len() / channels - frame) as f64 / rate as f64);
                let _ = onset_tx.send(received.checked_sub(after).unwrap_or(received));
            }
        },
    ))?;
    thread::sleep(Duration::from_millis(500));

    println!("Measuring trigger -> virtual mic latency ({trials} trials)");
    let mut results = Vec::new();
    for trial in 1..=trials {
        armed.store(true, Ordering::Release);
        let sent = Instant::now();
        let _ = commands.push(Command::Play(beep.clone()));
        match onset_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(onset) => {
                let ms = onset.saturating_duration_since(sent).as_secs_f64() * 1000.0;
                println!("  trial {trial}: {ms:.1} ms");
                results.push(ms);
            }
            Err(_) => {
                armed.store(false, Ordering::Release);
                println!("  trial {trial}: no signal detected");
            }
        }
        thread::sleep(Duration::from_millis(300));
    }

    if results.is_empty() {
        bail!("the beep never reached the virtual mic");
    }
    results.sort_by(f64::total_cmp);
    println!(
        "min {:.1} ms, median {:.1} ms, max {:.1} ms (includes capture buffering, so an upper bound)",
        results[0],
        results[results.len() / 2],
        results[results.len() - 1]
    );
    Ok(())
}
