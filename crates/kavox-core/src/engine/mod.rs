//! Real-time mixer. `Renderer::render` runs inside audio callbacks and never allocates, locks or blocks.

mod limiter;
mod meters;
mod mic;
mod renderer;
mod smoothed;

pub use meters::{Meter, Meters};
pub use mic::{MicInput, MicSink, mic_channel};
pub use renderer::{Command, EngineEvent, Garbage, Renderer, RendererHandle, renderer};

pub type VoiceId = u64;

/// Voices that can play at once per renderer; extra plays steal the oldest.
pub const MAX_VOICES: usize = 32;

/// Largest block a backend should pass to `Renderer::render`; bigger buffers are split.
pub const MAX_BLOCK_FRAMES: usize = 4096;
