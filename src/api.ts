// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs.
import { invoke } from '@tauri-apps/api/core';

export type Retrigger = 'restart' | 'overlap' | 'toggle';
export type Theme = 'system' | 'light' | 'dark';
export type NoticeLevel = 'info' | 'warn' | 'error';
export type GlobalHotkey = 'stopAll' | 'toggleMute';

export interface Sound {
  id: string;
  name: string;
  file: string;
  sha256: string;
  durationMs: number;
  volume: number;
  normGainDb: number;
  color: string;
  hotkey: string | null;
  retrigger: Retrigger;
  loop: boolean;
}

export interface Board {
  id: string;
  name: string;
  sounds: string[];
}

export interface Library {
  schema: number;
  activeBoard: string;
  boards: Board[];
  sounds: Record<string, Sound>;
}

export interface AudioSettings {
  micDevice: string | null;
  virtualOutput: string | null;
  monitorOutput: string | null;
  micPassthrough: boolean;
  micGain: number;
  micMuted: boolean;
  soundsToMicGain: number;
  soundsToMonitorGain: number;
  monitorEnabled: boolean;
  normalize: boolean;
}

export interface AppSettings {
  startMinimized: boolean;
  closeToTray: boolean;
  launchOnLogin: boolean;
  theme: Theme;
  setupComplete: boolean;
}

export interface Settings {
  schema: number;
  audio: AudioSettings;
  hotkeys: { stopAll: string | null; toggleMute: string | null };
  app: AppSettings;
}

export interface DeviceInfo {
  id: string;
  label: string;
  isDefault: boolean;
}

export interface DeviceList {
  inputs: DeviceInfo[];
  outputs: DeviceInfo[];
  virtualCandidates: string[];
}

export type VirtualStatus =
  | { kind: 'ok'; device: string; micLabel: string }
  | { kind: 'notInstalled' }
  | { kind: 'missing'; device: string }
  | { kind: 'cableIsDefaultOutput'; device: string; micLabel: string }
  | { kind: 'failed'; message: string };

export interface StreamInfo {
  device: string;
  label: string;
  rate: number;
  fallback: boolean;
}

export interface StreamStatus {
  mic: StreamInfo | null;
  virtual: StreamInfo | null;
  monitor: StreamInfo | null;
}

export interface PlaybackEvent {
  soundId: string;
  voiceId: number;
  state: 'started' | 'progress' | 'ended';
  frame: number;
  frames: number;
}

export interface Levels {
  mic: number;
  virtual: number;
  monitor: number;
}

export interface Notice {
  level: NoticeLevel;
  message: string;
}

export interface UiState {
  settings: Settings;
  library: Library;
  devices: DeviceList;
  virtualStatus: VirtualStatus;
  streams: StreamStatus;
  playing: PlaybackEvent[];
  notices: Notice[];
  platform: string;
  backend: string;
  readOnly: boolean;
  version: string;
  hotkeysSupported: boolean;
  autostartEnabled: boolean;
}

export interface ImportResult {
  added: Sound[];
  failed: { file: string; error: string }[];
}

export type SoundPatch = Partial<Pick<Sound, 'name' | 'volume' | 'color' | 'retrigger' | 'loop'>>;

export const api = {
  getState: () => invoke<UiState>('get_state'),
  refreshDevices: () => invoke<DeviceList>('refresh_devices'),
  setAudioSettings: (audio: AudioSettings) => invoke<void>('set_audio_settings', { audio }),
  setAppSettings: (settings: AppSettings) => invoke<void>('set_app_settings', { settings }),
  importSounds: (paths: string[], board: string | null) => invoke<ImportResult>('import_sounds', { paths, board }),
  updateSound: (id: string, patch: SoundPatch) => invoke<Sound>('update_sound', { id, patch }),
  deleteSound: (id: string) => invoke<void>('delete_sound', { id }),
  reorderSounds: (board: string, ids: string[]) => invoke<void>('reorder_sounds', { board, ids }),
  playSound: (id: string) => invoke<void>('play_sound', { id }),
  stopSound: (id: string) => invoke<void>('stop_sound', { id }),
  stopAll: () => invoke<void>('stop_all'),
  setMicMuted: (muted: boolean | null = null) => invoke<boolean>('set_mic_muted', { muted }),
  setSoundHotkey: (id: string, hotkey: string | null) => invoke<void>('set_sound_hotkey', { id, hotkey }),
  setGlobalHotkey: (which: GlobalHotkey, hotkey: string | null) => invoke<void>('set_global_hotkey', { which, hotkey }),
  playTestTone: (output: 'virtual' | 'monitor') => invoke<void>('play_test_tone', { output }),
  createBoard: (name: string) => invoke<Board>('create_board', { name }),
  renameBoard: (id: string, name: string) => invoke<void>('rename_board', { id, name }),
  deleteBoard: (id: string) => invoke<void>('delete_board', { id }),
  setActiveBoard: (id: string) => invoke<void>('set_active_board', { id }),
  copyDiagnostics: () => invoke<string>('copy_diagnostics'),
  openLogsFolder: () => invoke<void>('open_logs_folder'),
  openSoundSettings: () => invoke<void>('open_sound_settings'),
  setWindowVisible: (visible: boolean) => invoke<void>('set_window_visible', { visible }),
};

export function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object' && 'message' in error) return String(error.message);
  return String(error);
}

// Pad dot colours, readable on both the dark and light pad tiles. Keep in sync with PALETTE in crates/kavox-core/src/library.rs.
export const PALETTE = ['#e8262d', '#f0a020', '#c9ad7a', '#1fb5a0', '#3b9cff', '#f0607e', '#a3a29d', '#7d8c5a'];
export const AUDIO_EXTENSIONS = ['wav', 'mp3', 'ogg', 'oga', 'flac'];
export const VB_CABLE_URL = 'https://vb-audio.com/Cable/';
export const REPO_URL = 'https://github.com/Aitemaa/Kavox';
