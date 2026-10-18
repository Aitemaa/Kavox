use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use super::limiter::Limiter;
use super::meters::{Meter, Meters};
use super::mic::MicInput;
use super::smoothed::Smoothed;
use super::{MAX_BLOCK_FRAMES, MAX_VOICES, VoiceId};
use crate::clip::ClipData;

/// Fading voices may briefly exceed `MAX_VOICES`; these extra slots keep that allocation-free.
const VOICE_SLOTS: usize = MAX_VOICES + 8;
const STOP_FADE_MS: u32 = 10;
const STEAL_FADE_MS: u32 = 5;
const GAIN_RAMP_MS: u32 = 5;
const PROGRESS_HZ: u32 = 15;

pub enum Command {
    Play {
        voice: VoiceId,
        clip: Arc<ClipData>,
        gain: f32,
        looping: bool,
    },
    Stop {
        voice: VoiceId,
    },
    StopAll,
    SetVoiceGain {
        voice: VoiceId,
        gain: f32,
    },
    SetSoundsGain(f32),
    SetMicGain(f32),
    SetMicMuted(bool),
    /// Must already be prepared for this renderer's rate.
    SetMic(Option<Box<MicInput>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineEvent {
    Started(VoiceId),
    Progress { voice: VoiceId, frame: usize },
    Ended(VoiceId),
}

/// Things the audio thread hands back so it never frees memory itself.
pub enum Garbage {
    Clip(Arc<ClipData>),
    Mic(Box<MicInput>),
}

/// Control-thread side of a renderer.
pub struct RendererHandle {
    pub commands: Producer<Command>,
    pub events: Consumer<EngineEvent>,
    pub garbage: Consumer<Garbage>,
}

struct Voice {
    id: VoiceId,
    clip: Arc<ClipData>,
    pos: usize,
    gain: Smoothed,
    looping: bool,
    /// (frames left, fade length)
    fade: Option<(u32, u32)>,
}

pub struct Renderer {
    rate: u32,
    voices: Vec<Voice>,
    commands: Consumer<Command>,
    events: Producer<EngineEvent>,
    garbage: Producer<Garbage>,
    mic: Option<Box<MicInput>>,
    mic_buf: Vec<f32>,
    sounds_gain: Smoothed,
    mic_gain: Smoothed,
    mic_level: f32,
    mic_muted: bool,
    limiter: Limiter,
    meters: Arc<Meters>,
    meter: Meter,
    since_progress: usize,
}

pub fn renderer(meter: Meter, meters: Arc<Meters>) -> (Renderer, RendererHandle) {
    let (commands_tx, commands) = RingBuffer::new(256);
    let (events, events_rx) = RingBuffer::new(4096);
    let (garbage, garbage_rx) = RingBuffer::new(256);
    let renderer = Renderer {
        rate: 48_000,
        voices: Vec::with_capacity(VOICE_SLOTS),
        commands,
        events,
        garbage,
        mic: None,
        mic_buf: vec![0.0; MAX_BLOCK_FRAMES],
        sounds_gain: Smoothed::new(1.0),
        mic_gain: Smoothed::new(1.0),
        mic_level: 1.0,
        mic_muted: false,
        limiter: Limiter::new(48_000),
        meters,
        meter,
        since_progress: 0,
    };
    let handle = RendererHandle {
        commands: commands_tx,
        events: events_rx,
        garbage: garbage_rx,
    };
    (renderer, handle)
}

impl Renderer {
    /// Initial gains and mic, applied before the stream starts.
    pub fn configure(
        &mut self,
        sounds_gain: f32,
        mic_gain: f32,
        mic_muted: bool,
        mic: Option<Box<MicInput>>,
    ) {
        self.sounds_gain = Smoothed::new(sounds_gain);
        self.mic_level = mic_gain;
        self.mic_muted = mic_muted;
        self.mic_gain = Smoothed::new(if mic_muted { 0.0 } else { mic_gain });
        self.mic = mic;
    }

    pub fn has_mic(&self) -> bool {
        self.mic.is_some()
    }

    /// Called by the backend once the device rate is known, before the first `render`.
    pub fn prepare(&mut self, rate: u32) {
        self.rate = rate.max(1);
        self.limiter = Limiter::new(self.rate);
        if let Some(mic) = &mut self.mic {
            mic.prepare(self.rate);
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    fn ms_to_frames(&self, ms: u32) -> u32 {
        (self.rate * ms / 1000).max(1)
    }

    /// Fills `out` (interleaved stereo) with the mix.
    pub fn render(&mut self, out: &mut [f32]) {
        if out.len() > MAX_BLOCK_FRAMES * 2 {
            for chunk in out.chunks_mut(MAX_BLOCK_FRAMES * 2) {
                self.render(chunk);
            }
            return;
        }
        self.process_commands();
        out.fill(0.0);

        let mut i = 0;
        while i < self.voices.len() {
            if mix_voice(&mut self.voices[i], out) {
                i += 1;
            } else {
                let voice = self.voices.remove(i);
                self.finish(voice);
            }
        }
        for frame in out.as_chunks_mut::<2>().0 {
            let g = self.sounds_gain.next();
            frame[0] *= g;
            frame[1] *= g;
        }

        if let Some(mic) = &mut self.mic {
            let mono = &mut self.mic_buf[..out.len() / 2];
            mic.read(mono);
            for (frame, s) in out.as_chunks_mut::<2>().0.iter_mut().zip(mono.iter()) {
                let v = s * self.mic_gain.next();
                frame[0] += v;
                frame[1] += v;
            }
        }

        let peak = self.limiter.process(out);
        self.meters.record(self.meter, peak);

        self.since_progress += out.len() / 2;
        if self.since_progress >= (self.rate / PROGRESS_HZ) as usize {
            self.since_progress = 0;
            for voice in &self.voices {
                let _ = self.events.push(EngineEvent::Progress {
                    voice: voice.id,
                    frame: voice.pos,
                });
            }
        }
    }

    fn process_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            match command {
                Command::Play {
                    voice,
                    clip,
                    gain,
                    looping,
                } => self.start(voice, clip, gain, looping),
                Command::Stop { voice } => {
                    let fade = self.ms_to_frames(STOP_FADE_MS);
                    if let Some(v) = self
                        .voices
                        .iter_mut()
                        .find(|v| v.id == voice && v.fade.is_none())
                    {
                        v.fade = Some((fade, fade));
                    }
                }
                Command::StopAll => {
                    let fade = self.ms_to_frames(STOP_FADE_MS);
                    for v in self.voices.iter_mut().filter(|v| v.fade.is_none()) {
                        v.fade = Some((fade, fade));
                    }
                }
                Command::SetVoiceGain { voice, gain } => {
                    let ramp = self.ms_to_frames(GAIN_RAMP_MS);
                    if let Some(v) = self.voices.iter_mut().find(|v| v.id == voice) {
                        v.gain.set(gain, ramp);
                    }
                }
                Command::SetSoundsGain(gain) => {
                    let ramp = self.ms_to_frames(GAIN_RAMP_MS);
                    self.sounds_gain.set(gain, ramp);
                }
                Command::SetMicGain(gain) => {
                    self.mic_level = gain;
                    self.update_mic_gain();
                }
                Command::SetMicMuted(muted) => {
                    self.mic_muted = muted;
                    self.update_mic_gain();
                }
                Command::SetMic(mic) => {
                    if let Some(old) = std::mem::replace(&mut self.mic, mic) {
                        self.trash(Garbage::Mic(old));
                    }
                }
            }
        }
    }

    fn update_mic_gain(&mut self) {
        let ramp = self.ms_to_frames(GAIN_RAMP_MS);
        let target = if self.mic_muted { 0.0 } else { self.mic_level };
        self.mic_gain.set(target, ramp);
    }

    fn start(&mut self, voice: VoiceId, clip: Arc<ClipData>, gain: f32, looping: bool) {
        // A clip for another rate can arrive while switching devices; treat it as already finished.
        if clip.rate != self.rate || clip.frames == 0 {
            self.trash(Garbage::Clip(clip));
            let _ = self.events.push(EngineEvent::Ended(voice));
            return;
        }
        if self.voices.iter().filter(|v| v.fade.is_none()).count() >= MAX_VOICES {
            let fade = self.ms_to_frames(STEAL_FADE_MS);
            if let Some(oldest) = self.voices.iter_mut().find(|v| v.fade.is_none()) {
                oldest.fade = Some((fade, fade));
            }
        }
        if self.voices.len() >= VOICE_SLOTS {
            let oldest = self.voices.remove(0);
            self.finish(oldest);
        }
        self.voices.push(Voice {
            id: voice,
            clip,
            pos: 0,
            gain: Smoothed::new(gain),
            looping,
            fade: None,
        });
        let _ = self.events.push(EngineEvent::Started(voice));
    }

    fn finish(&mut self, voice: Voice) {
        let _ = self.events.push(EngineEvent::Ended(voice.id));
        self.trash(Garbage::Clip(voice.clip));
    }

    fn trash(&mut self, garbage: Garbage) {
        // Only fails if the control thread stopped draining; dropping here is the rare fallback.
        let _ = self.garbage.push(garbage);
    }
}

/// Adds one voice into `out`; returns false once it has finished.
fn mix_voice(v: &mut Voice, out: &mut [f32]) -> bool {
    let frames = v.clip.frames;
    let samples = &v.clip.samples;
    for frame in out.as_chunks_mut::<2>().0 {
        if v.pos >= frames {
            if v.looping {
                v.pos = 0;
            } else {
                return false;
            }
        }
        let mut g = v.gain.next();
        if let Some((left, total)) = &mut v.fade {
            if *left == 0 {
                return false;
            }
            g *= *left as f32 / *total as f32;
            *left -= 1;
        }
        let i = v.pos * 2;
        frame[0] += samples[i] * g;
        frame[1] += samples[i + 1] * g;
        v.pos += 1;
    }
    let ended = v.pos >= frames && !v.looping;
    let faded = matches!(v.fade, Some((0, _)));
    !(ended || faded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::mic_channel;

    const RATE: u32 = 48_000;

    fn setup() -> (Renderer, RendererHandle) {
        let (mut r, h) = renderer(Meter::Virtual, Arc::new(Meters::default()));
        r.prepare(RATE);
        (r, h)
    }

    fn clip(frames: usize, value: f32) -> Arc<ClipData> {
        Arc::new(ClipData::from_stereo(RATE, vec![value; frames * 2]))
    }

    fn events(h: &mut RendererHandle) -> Vec<EngineEvent> {
        std::iter::from_fn(|| h.events.pop().ok())
            .filter(|e| !matches!(e, EngineEvent::Progress { .. }))
            .collect()
    }

    fn play(h: &mut RendererHandle, voice: VoiceId, clip: Arc<ClipData>) {
        assert!(
            h.commands
                .push(Command::Play {
                    voice,
                    clip,
                    gain: 1.0,
                    looping: false
                })
                .is_ok()
        );
    }

    #[test]
    fn plays_clip_to_the_end_and_reports() {
        let (mut r, mut h) = setup();
        play(&mut h, 1, clip(700, 0.25));
        let mut out = vec![0f32; 960];
        r.render(&mut out);
        assert!(out.iter().all(|&s| (s - 0.25).abs() < 1e-6));
        assert_eq!(events(&mut h), [EngineEvent::Started(1)]);
        r.render(&mut out);
        assert!((out[2 * 219] - 0.25).abs() < 1e-6 && out[2 * 220] == 0.0);
        assert_eq!(events(&mut h), [EngineEvent::Ended(1)]);
        assert!(matches!(h.garbage.pop(), Ok(Garbage::Clip(_))));
    }

    #[test]
    fn overlapping_voices_sum() {
        let (mut r, mut h) = setup();
        play(&mut h, 1, clip(1000, 0.2));
        play(&mut h, 2, clip(1000, 0.3));
        let mut out = vec![0f32; 200];
        r.render(&mut out);
        assert!((out[10] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn stop_fades_out_without_a_click() {
        let (mut r, mut h) = setup();
        play(&mut h, 7, clip(48_000, 0.5));
        let mut out = vec![0f32; 960];
        r.render(&mut out);
        assert!(h.commands.push(Command::Stop { voice: 7 }).is_ok());
        r.render(&mut out);
        let fade = (RATE * STOP_FADE_MS / 1000) as usize;
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert!(
            left[..fade]
                .windows(2)
                .all(|w| w[1] <= w[0] && w[0] - w[1] < 0.01)
        );
        assert!(left[fade..].iter().all(|&s| s == 0.0));
        assert_eq!(
            events(&mut h),
            [EngineEvent::Started(7), EngineEvent::Ended(7)]
        );
    }

    #[test]
    fn looping_wraps_until_stopped() {
        let (mut r, mut h) = setup();
        assert!(
            h.commands
                .push(Command::Play {
                    voice: 1,
                    clip: clip(100, 0.4),
                    gain: 1.0,
                    looping: true
                })
                .is_ok()
        );
        let mut out = vec![0f32; 2000];
        for _ in 0..5 {
            r.render(&mut out);
            assert!(out.iter().all(|&s| (s - 0.4).abs() < 1e-6));
        }
        assert_eq!(events(&mut h), [EngineEvent::Started(1)]);
    }

    #[test]
    fn steals_oldest_voice_when_full() {
        let (mut r, mut h) = setup();
        for v in 0..MAX_VOICES as u64 + 1 {
            play(&mut h, v, clip(48_000, 0.001));
        }
        let mut out = vec![0f32; 960];
        r.render(&mut out);
        let ended: Vec<_> = events(&mut h)
            .into_iter()
            .filter(|e| matches!(e, EngineEvent::Ended(_)))
            .collect();
        assert_eq!(ended, [EngineEvent::Ended(0)]);
    }

    #[test]
    fn wrong_rate_clip_ends_immediately() {
        let (mut r, mut h) = setup();
        play(
            &mut h,
            3,
            Arc::new(ClipData::from_stereo(44_100, vec![0.5; 200])),
        );
        let mut out = vec![0f32; 100];
        r.render(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
        assert_eq!(events(&mut h), [EngineEvent::Ended(3)]);
    }

    #[test]
    fn bus_gain_ramps_and_mute_keeps_sounds() {
        let meters = Arc::new(Meters::default());
        let (mut sink, mut mic) = mic_channel(meters.clone());
        mic.set_input_rate(RATE);
        let (mut r, mut h) = renderer(Meter::Virtual, meters);
        r.configure(0.5, 1.0, false, Some(Box::new(mic)));
        r.prepare(RATE);
        sink.push_interleaved(&vec![0.1f32; 4000], 1, |s| s);
        play(&mut h, 1, clip(48_000, 0.2));
        let mut out = vec![0f32; 960];
        r.render(&mut out);
        assert!((out[100] - (0.2 * 0.5 + 0.1)).abs() < 1e-5);

        assert!(h.commands.push(Command::SetMicMuted(true)).is_ok());
        r.render(&mut out);
        r.render(&mut out);
        assert!(
            (out[100] - 0.1).abs() < 1e-5,
            "sounds should keep playing while the mic is muted"
        );
    }

    #[test]
    fn splits_oversized_blocks() {
        let (mut r, mut h) = setup();
        play(&mut h, 1, clip(20_000, 0.3));
        let mut out = vec![0f32; (MAX_BLOCK_FRAMES * 3) * 2];
        r.render(&mut out);
        assert!(out.iter().all(|&s| (s - 0.3).abs() < 1e-6));
    }
}
