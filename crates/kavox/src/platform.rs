//! Platform integration: config/data/log paths, autostart, file dialogs, URLs and icon decoding.

use std::path::{Path, PathBuf};

use auto_launch::AutoLaunchBuilder;
use kavox_core::Paths;

pub const APP_ID: &str = "io.github.aitemaa.kavox";
pub const AUTOSTART_FLAG: &str = "--autostart";
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "oga", "flac"];
pub const VB_CABLE_URL: &str = "https://vb-audio.com/Cable/";
pub const REPO_URL: &str = "https://github.com/Aitemaa/Kavox";

const ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/64x64.png");

/// Resolves config, sound library and log paths matching the existing Kavox directory layout.
pub fn app_paths() -> Paths {
    let config_root = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let data_root = dirs::data_dir().unwrap_or_else(|| config_root.clone());
    #[cfg(windows)]
    let log_root = dirs::data_local_dir().unwrap_or_else(|| data_root.clone());
    #[cfg(not(windows))]
    let log_root = data_root.clone();

    Paths {
        config_dir: config_root.join(APP_ID),
        sounds_dir: data_root.join(APP_ID).join("sounds"),
        log_dir: log_root.join(APP_ID).join("logs"),
    }
}

/// Decodes the bundled 64x64 PNG icon to RGBA bytes + dimensions.
pub fn decoded_icon() -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((rgba, info.width, info.height))
}

pub fn viewport_icon() -> Option<eframe::egui::IconData> {
    let (rgba, width, height) = decoded_icon()?;
    Some(eframe::egui::IconData {
        rgba,
        width,
        height,
    })
}

fn auto_launch_handle() -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_str = exe.to_string_lossy().into_owned();
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name("Kavox")
        .set_app_path(&exe_str)
        .set_args(&[AUTOSTART_FLAG]);
    builder.build().map_err(|e| e.to_string())
}

pub fn is_autostart_enabled() -> bool {
    auto_launch_handle()
        .and_then(|al| al.is_enabled().map_err(|e| e.to_string()))
        .unwrap_or(false)
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let al = auto_launch_handle()?;
    let current = al.is_enabled().unwrap_or(false);
    if current == enabled {
        return Ok(());
    }
    if enabled {
        al.enable()
            .map_err(|e| format!("Couldn't change launch on login: {e}"))
    } else {
        al.disable()
            .map_err(|e| format!("Couldn't change launch on login: {e}"))
    }
}

pub fn open_url(url: &str) -> Result<(), String> {
    open::that_detached(url).map_err(|e| e.to_string())
}

pub fn open_logs_folder(log_dir: &Path) -> Result<(), String> {
    let _ = std::fs::create_dir_all(log_dir);
    open::that_detached(log_dir).map_err(|e| e.to_string())
}

pub fn open_sound_settings() -> Result<(), String> {
    if cfg!(windows) {
        open::that_detached("ms-settings:sound").map_err(|e| e.to_string())
    } else {
        Err("Open your desktop's sound settings to change devices.".into())
    }
}

/// Opens a native multi-file audio picker (blocking; call on a worker thread).
pub fn pick_audio_files() -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Add sounds")
        .add_filter("Audio", AUDIO_EXTENSIONS)
        .pick_files()
        .unwrap_or_default()
}
