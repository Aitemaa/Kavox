//! Renders the README screenshots offscreen with `egui_kittest` (wgpu), no window involved.
//!
//! The real `KavoxApp::ui` runs against made-up demo data: a controller with a do-nothing audio
//! backend in a temp dir, an inert hotkey service and no tray. Nothing here reads the user's
//! config, library or audio devices.
//!
//! Regenerate with:
//!
//! ```sh
//! KAVOX_SNAPSHOT_DIR=docs/screenshots cargo test -p kavox render_readme_screenshots
//! ```
//!
//! Without `KAVOX_SNAPSHOT_DIR` the test returns immediately, so a plain `cargo test` stays fast
//! and doesn't need a GPU.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

use eframe::egui;
use egui_kittest::Harness;
use kavox_core::backend::{AudioBackend, StreamErrorFn, StreamHandle};
use kavox_core::engine::{MicSink, Renderer};
use kavox_core::library::PALETTE;
use kavox_core::{
    Board, Controller, DeviceInfo, DeviceList, Error, EventSink, Library, Paths, Result, Retrigger,
    Settings, Snapshot, Sound, StreamInfo, StreamRole, StreamStatus, Theme, UiEvent, VirtualStatus,
};

use super::KavoxApp;
use crate::hotkey::HotkeyService;
use crate::state::{UiState, View, VoiceProgress};
use crate::theme::{self, Palette};

/// Audio backend with one fake mic and speakers that can't open streams.
struct DemoBackend;

impl AudioBackend for DemoBackend {
    fn name(&self) -> &'static str {
        "demo"
    }
    fn list_devices(&mut self) -> Result<DeviceList> {
        Ok(DeviceList::default())
    }
    fn resolve_virtual(
        &mut self,
        _: Option<&str>,
        _: &DeviceList,
    ) -> (Option<String>, VirtualStatus) {
        (None, VirtualStatus::NotInstalled)
    }
    fn open_output(
        &mut self,
        _: StreamRole,
        _: Option<&str>,
        _: Renderer,
        _: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>> {
        Err(Error::Device("demo backend".into()))
    }
    fn open_input(
        &mut self,
        _: Option<&str>,
        _: MicSink,
        _: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>> {
        Err(Error::Device("demo backend".into()))
    }
}

struct NoSink;

impl EventSink for NoSink {
    fn emit(&self, _: UiEvent) {}
}

/// `eframe::App` around the real app shell (what `Hosted` does in `main`).
struct Shot(KavoxApp);

impl eframe::App for Shot {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.0.ui(ui, frame);
    }
}

/// (name, file, duration ms, palette index, hotkey)
const DEMO_SOUNDS: &[(&str, &str, u64, usize, Option<&str>)] = &[
    ("Airhorn", "airhorn.ogg", 2_300, 0, Some("Ctrl+Alt+1")),
    ("Sad trombone", "sad-trombone.mp3", 3_100, 1, None),
    ("Applause", "applause.wav", 6_800, 3, Some("Ctrl+Alt+3")),
    ("Drumroll", "drumroll.ogg", 4_200, 4, None),
    ("Bruh", "bruh.mp3", 1_100, 5, Some("F5")),
    ("Ding", "ding.wav", 800, 2, None),
    ("Crickets", "crickets.ogg", 5_400, 7, None),
    ("Wow", "wow.mp3", 1_400, 1, Some("F6")),
    ("Laugh track", "laugh-track.ogg", 7_600, 3, None),
    ("Rimshot", "rimshot.wav", 1_900, 0, None),
    ("Victory", "victory.mp3", 5_100, 4, Some("Ctrl+Alt+9")),
    ("Boing", "boing.wav", 900, 5, None),
    // The next three live on the "Streaming" board.
    ("Intro jingle", "intro-jingle.ogg", 6_200, 4, None),
    ("Be right back", "brb.mp3", 3_400, 2, None),
    ("Outro", "outro.ogg", 8_800, 1, None),
];

/// How many of `DEMO_SOUNDS` are on the "Main" board.
const MAIN_BOARD_LEN: usize = 12;

fn demo_library() -> Library {
    let mut sounds = std::collections::BTreeMap::new();
    let mut ids = Vec::new();
    for (i, (name, file, duration_ms, color, hotkey)) in DEMO_SOUNDS.iter().enumerate() {
        let id = format!("demo-sound-{i:02}");
        ids.push(id.clone());
        sounds.insert(
            id.clone(),
            Sound {
                id,
                name: (*name).into(),
                file: (*file).into(),
                sha256: format!("{i:064x}"),
                duration_ms: *duration_ms,
                volume: 1.0,
                norm_gain_db: 0.0,
                color: PALETTE[*color].into(),
                hotkey: hotkey.map(Into::into),
                // Show all three trigger-mode icons on the board.
                retrigger: match i {
                    8 => Retrigger::Toggle,
                    6 | 9 => Retrigger::Overlap,
                    _ => Retrigger::Restart,
                },
                looping: i == 6,
            },
        );
    }
    let board = |id: &str, name: &str, sounds: Vec<String>| Board {
        id: id.into(),
        name: name.into(),
        sounds,
    };
    Library {
        schema: kavox_core::library::LIBRARY_SCHEMA,
        active_board: "demo-board-main".into(),
        boards: vec![
            board("demo-board-main", "Main", ids[..MAIN_BOARD_LEN].to_vec()),
            board(
                "demo-board-stream",
                "Streaming",
                ids[MAIN_BOARD_LEN..].to_vec(),
            ),
        ],
        sounds,
    }
}

fn device(id: &str, label: &str, is_default: bool) -> DeviceInfo {
    DeviceInfo {
        id: id.into(),
        label: label.into(),
        is_default,
    }
}

fn demo_devices() -> DeviceList {
    DeviceList {
        inputs: vec![
            device("demo-mic", "Blue Yeti USB Microphone", true),
            device("demo-webcam-mic", "HD Webcam Microphone", false),
        ],
        outputs: vec![
            device("demo-headphones", "Sennheiser HD 560S", true),
            device("demo-speakers", "Studio Monitors", false),
        ],
        virtual_candidates: Vec::new(),
    }
}

fn stream(device: &str, label: &str) -> Option<StreamInfo> {
    Some(StreamInfo {
        device: device.into(),
        label: label.into(),
        rate: 48_000,
        fallback: false,
    })
}

fn demo_snapshot(theme: Theme) -> Snapshot {
    let mut settings = Settings::default();
    settings.app.theme = theme;
    settings.app.setup_complete = true;
    settings.audio.mic_device = Some("demo-mic".into());
    settings.audio.monitor_output = Some("demo-headphones".into());
    settings.audio.mic_gain = 1.0;
    settings.audio.sounds_to_mic_gain = 0.85;
    settings.audio.sounds_to_monitor_gain = 0.55;
    // No "Stop all" hotkey: its keycaps make the strip's end controls wider than the space
    // `mixer_strip` reserves for them, and they would overlap the last fader.
    settings.hotkeys.toggle_mute = Some("Ctrl+Alt+M".into());
    Snapshot {
        settings,
        library: demo_library(),
        devices: demo_devices(),
        virtual_status: VirtualStatus::Ok {
            device: "kavox_mic".into(),
            mic_label: "Kavox Mic".into(),
        },
        streams: StreamStatus {
            mic: stream("demo-mic", "Blue Yeti USB Microphone"),
            virtual_out: stream("kavox_mic", "Kavox Mic"),
            monitor: stream("demo-headphones", "Sennheiser HD 560S"),
        },
        playing: Vec::new(),
        notices: Vec::new(),
        read_only: false,
        platform: "linux",
        backend: "pulse",
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Scene {
    Soundboard,
    Voice,
    PadEditor,
}

impl Scene {
    /// Logical window size. The voice board is cut off above its "Hotkeys" disclosure, whose
    /// triangle glyph isn't in the bundled fonts and would show as a tofu box.
    fn window(self) -> egui::Vec2 {
        match self {
            Scene::Voice => egui::vec2(1000.0, 596.0),
            // Tall enough for the whole editor drawer.
            Scene::PadEditor => egui::vec2(1000.0, 760.0),
            _ => egui::vec2(1000.0, 650.0),
        }
    }
}

fn demo_state(scene: Scene, theme: Theme) -> UiState {
    let mut state = UiState::from_snapshot(
        demo_snapshot(theme),
        env!("CARGO_PKG_VERSION"),
        true,
        PathBuf::from("/home/demo/.local/share/kavox/logs"),
        false,
    );
    state.levels = kavox_core::Levels {
        mic: 0.22,
        virtual_out: 0.36,
        monitor: 0.14,
    };
    match scene {
        Scene::Soundboard => {
            // "Applause" is mid-playback.
            state.playing.insert(
                "demo-sound-02".into(),
                vec![VoiceProgress {
                    voice_id: 1,
                    frame: 134_000,
                    frames: 326_400,
                }],
            );
        }
        Scene::Voice => {
            state.view = View::Voice;
            state.settings.voice.enabled = true;
            state.settings.voice.preset = "chipmunk".into();
            state.settings.voice.hear_myself = true;
            state
                .settings
                .voice
                .hotkeys
                .insert("chipmunk".into(), "Ctrl+Alt+F1".into());
            state
                .settings
                .voice
                .hotkeys
                .insert("giant".into(), "Ctrl+Alt+F2".into());
            state.levels.mic = 0.3;
        }
        Scene::PadEditor => {
            state.start_editing("demo-sound-00".into());
            state.focused_pad = Some("demo-sound-00".into());
        }
    }
    state
}

/// Renders `scene` to a PNG at `out`.
fn render(scene: Scene, theme: Theme, pixels_per_point: f32, out: &std::path::Path) {
    let scratch = std::env::temp_dir().join(format!(
        "kavox-screenshots-{}-{}",
        std::process::id(),
        out.file_stem().and_then(|s| s.to_str()).unwrap_or("shot")
    ));
    let paths = Paths {
        config_dir: scratch.join("config"),
        sounds_dir: scratch.join("sounds"),
        log_dir: scratch.join("logs"),
    };
    let controller = Controller::start(paths, Box::new(DemoBackend), Arc::new(NoSink));
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let app = KavoxApp::new(
        controller.clone(),
        HotkeyService::inert(),
        cmd_tx,
        cmd_rx,
        demo_state(scene, theme),
        None,
    );

    let dark = theme == Theme::Dark;
    let mut harness = Harness::builder()
        .with_size(scene.window())
        .with_pixels_per_point(pixels_per_point)
        .with_theme(if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        })
        .allow_missing_glyphs()
        // Not a test of the UI: the search box has no accessible name, which is fine for a picture.
        .with_accessibility_check(false)
        .wgpu()
        .with_context_setup(move |ctx| {
            theme::install_fonts(ctx);
            let palette = if dark { Palette::DARK } else { Palette::LIGHT };
            theme::apply_visuals(ctx, &palette);
        })
        .build_eframe(|_cc| Shot(app));

    // Let layout settle (panels size themselves over a couple of passes).
    harness.run_steps(6);
    let image = harness.render().expect("offscreen render");
    image.save(out).expect("write PNG");
    drop(harness);

    controller.shutdown();
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn render_readme_screenshots() {
    let Some(dir) = std::env::var_os("KAVOX_SNAPSHOT_DIR") else {
        return;
    };
    let mut dir = PathBuf::from(dir);
    if dir.is_relative() {
        // Resolve against the workspace root, not the crate dir cargo runs tests in.
        dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
    }
    std::fs::create_dir_all(&dir).expect("create screenshot dir");

    let ppp = std::env::var("KAVOX_SNAPSHOT_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.5);
    let started = Instant::now();
    for (file, scene, theme) in [
        ("soundboard-dark", Scene::Soundboard, Theme::Dark),
        ("soundboard-light", Scene::Soundboard, Theme::Light),
        ("voice-dark", Scene::Voice, Theme::Dark),
        ("voice-light", Scene::Voice, Theme::Light),
        ("pad-editor-dark", Scene::PadEditor, Theme::Dark),
    ] {
        render(scene, theme, ppp, &dir.join(format!("{file}.png")));
    }
    eprintln!("rendered screenshots in {:?}", started.elapsed());
}
