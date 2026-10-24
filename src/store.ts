import { create } from 'zustand';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import {
  api,
  errorMessage,
  AUDIO_EXTENSIONS,
  type AppSettings,
  type AudioSettings,
  type DeviceList,
  type Levels,
  type Library,
  type Notice,
  type NoticeLevel,
  type PlaybackEvent,
  type Settings,
  type StreamStatus,
  type VirtualStatus,
  type VoiceSettings,
} from './api';

export interface Voice {
  voiceId: number;
  frame: number;
  frames: number;
}

export interface Toast {
  id: number;
  level: NoticeLevel;
  message: string;
}

export interface Confirm {
  title: string;
  message: string;
  action: string;
  onConfirm: () => void;
}

export type View = 'sounds' | 'voice';

export type SettingsTab = 'audio' | 'hotkeys' | 'app' | 'about';

interface KavoxState {
  ready: boolean;
  loadError: string | null;
  version: string;
  platform: string;
  hotkeysSupported: boolean;
  autostartEnabled: boolean;
  readOnly: boolean;
  settings: Settings;
  library: Library;
  devices: DeviceList;
  virtualStatus: VirtualStatus;
  streams: StreamStatus;
  playing: Record<string, Voice[]>;
  levels: Levels;
  toasts: Toast[];
  search: string;
  editing: string | null;
  dialog: null | 'settings' | 'wizard';
  settingsTab: SettingsTab;
  confirm: Confirm | null;
  dragging: boolean;
  view: View;
  lastAudioEdit: number;
  lastVoiceEdit: number;
}

export const useStore = create<KavoxState>(() => ({
  ready: false,
  loadError: null,
  version: '',
  platform: '',
  hotkeysSupported: true,
  autostartEnabled: false,
  readOnly: false,
  settings: {} as Settings,
  library: { schema: 1, activeBoard: '', boards: [], sounds: {} },
  devices: { inputs: [], outputs: [], virtualCandidates: [] },
  virtualStatus: { kind: 'notInstalled' },
  streams: { mic: null, virtual: null, monitor: null },
  playing: {},
  levels: { mic: 0, virtual: 0, monitor: 0 },
  toasts: [],
  search: '',
  editing: null,
  dialog: null,
  settingsTab: 'audio',
  confirm: null,
  dragging: false,
  view: 'sounds',
  lastAudioEdit: 0,
  lastVoiceEdit: 0,
}));

const set = useStore.setState;
const get = useStore.getState;

let nextToast = 0;

export function toast(level: NoticeLevel, message: string) {
  const id = ++nextToast;
  set((s) => ({ toasts: [...s.toasts.slice(-3), { id, level, message }] }));
  setTimeout(() => dismissToast(id), level === 'error' ? 10_000 : 6_000);
}

export function dismissToast(id: number) {
  set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
}

/** Runs a command and turns failures into an error toast. */
export function run<T>(promise: Promise<T>): Promise<T | undefined> {
  return promise.catch((e) => {
    toast('error', errorMessage(e));
    return undefined;
  });
}

let initialized = false;

export async function init() {
  if (initialized) return;
  initialized = true;
  await Promise.all([
    listen<PlaybackEvent>('playback', (e) => applyPlayback(e.payload)),
    listen<Levels>('levels', (e) => set({ levels: e.payload })),
    listen<DeviceList>('devices-changed', (e) => set({ devices: e.payload })),
    listen<VirtualStatus>('virtual-status', (e) => set({ virtualStatus: e.payload })),
    listen<Library>('library-changed', (e) => applyLibrary(e.payload)),
    listen<Settings>('settings-changed', (e) => applySettings(e.payload)),
    listen<StreamStatus>('streams-changed', (e) => set({ streams: e.payload })),
    listen<Notice>('notice', (e) => toast(e.payload.level, e.payload.message)),
  ]);
  document.addEventListener('visibilitychange', () => void api.setWindowVisible(!document.hidden).catch(() => {}));

  try {
    const s = await api.getState();
    const playing: Record<string, Voice[]> = {};
    for (const p of s.playing) (playing[p.soundId] ??= []).push({ voiceId: p.voiceId, frame: p.frame, frames: p.frames });
    set({
      ready: true,
      version: s.version,
      platform: s.platform,
      hotkeysSupported: s.hotkeysSupported,
      autostartEnabled: s.autostartEnabled,
      readOnly: s.readOnly,
      settings: s.settings,
      library: s.library,
      devices: s.devices,
      virtualStatus: s.virtualStatus,
      streams: s.streams,
      playing,
      dialog: s.settings.app.setupComplete ? null : 'wizard',
    });
    s.notices.forEach((n) => toast(n.level, n.message));
  } catch (e) {
    set({ loadError: errorMessage(e) });
  }
}

function applyPlayback(p: PlaybackEvent) {
  set((s) => {
    const voices = s.playing[p.soundId] ?? [];
    const others = voices.filter((v) => v.voiceId !== p.voiceId);
    const playing = { ...s.playing };
    if (p.state === 'ended') {
      if (others.length) playing[p.soundId] = others;
      else delete playing[p.soundId];
    } else {
      const voice = { voiceId: p.voiceId, frame: p.frame, frames: p.frames };
      const exists = voices.some((v) => v.voiceId === p.voiceId);
      playing[p.soundId] = exists ? voices.map((v) => (v.voiceId === p.voiceId ? voice : v)) : [...voices, voice];
    }
    return { playing };
  });
}

function applyLibrary(library: Library) {
  set((s) => ({ library, editing: s.editing && library.sounds[s.editing] ? s.editing : null }));
}

/** Keeps in-flight slider values from jumping back while their own echoes arrive. */
function applySettings(next: Settings) {
  const { lastAudioEdit, lastVoiceEdit, settings } = get();
  const now = Date.now();
  set({
    settings: {
      ...next,
      audio: now - lastAudioEdit < 400 ? settings.audio : next.audio,
      voice: now - lastVoiceEdit < 400 ? { ...next.voice, ...settings.voice, hotkeys: next.voice.hotkeys } : next.voice,
    },
  });
}

export function updateAudio(patch: Partial<AudioSettings>) {
  const audio = { ...get().settings.audio, ...patch };
  set((s) => ({ settings: { ...s.settings, audio }, lastAudioEdit: Date.now() }));
  return run(api.setAudioSettings(audio));
}

export function updateVoice(patch: Partial<VoiceSettings>) {
  const voice = { ...get().settings.voice, ...patch };
  set((s) => ({ settings: { ...s.settings, voice }, lastVoiceEdit: Date.now() }));
  return run(api.setVoiceChanger(voice));
}

export function setView(view: View) {
  set({ view, editing: null });
}

export async function updateApp(patch: Partial<AppSettings>) {
  const previous = get().settings.app;
  const app = { ...previous, ...patch };
  set((s) => ({ settings: { ...s.settings, app } }));
  try {
    await api.setAppSettings(app);
    if (patch.launchOnLogin !== undefined) set({ autostartEnabled: patch.launchOnLogin });
  } catch (e) {
    set((s) => ({ settings: { ...s.settings, app: previous } }));
    toast('error', errorMessage(e));
  }
}

export async function importPaths(paths: string[]) {
  if (!paths.length) return;
  const result = await run(api.importSounds(paths, get().library.activeBoard));
  if (!result) return;
  if (result.added.length === 1) toast('info', `Added "${result.added[0].name}"`);
  else if (result.added.length > 1) toast('info', `Added ${result.added.length} sounds`);
  for (const failure of result.failed) toast('error', `${failure.file}: ${failure.error}`);
}

export async function pickAndImport() {
  const picked = await open({
    multiple: true,
    title: 'Add sounds',
    filters: [{ name: 'Audio', extensions: AUDIO_EXTENSIONS }],
  });
  const paths = picked == null ? [] : Array.isArray(picked) ? picked : [picked];
  await importPaths(paths);
}

export function reorder(boardId: string, ids: string[]) {
  set((s) => ({
    library: { ...s.library, boards: s.library.boards.map((b) => (b.id === boardId ? { ...b, sounds: ids } : b)) },
  }));
  void run(api.reorderSounds(boardId, ids));
}

export function openSettings(tab: SettingsTab = 'audio') {
  set({ dialog: 'settings', settingsTab: tab });
}

export function askConfirm(confirm: Confirm) {
  set({ confirm });
}

export function confirmDeleteSound(id: string) {
  const sound = get().library.sounds[id];
  if (!sound) return;
  askConfirm({
    title: `Delete "${sound.name}"?`,
    message: 'The sound and its hotkey are removed from Kavox. The original file on your computer stays where it is.',
    action: 'Delete sound',
    onConfirm: () => void run(api.deleteSound(id)),
  });
}

export function activeBoard(library: Library) {
  return library.boards.find((b) => b.id === library.activeBoard) ?? library.boards[0];
}

export function formatDuration(ms: number) {
  if (ms < 10_000) return `${(ms / 1000).toFixed(1)} s`;
  const total = Math.round(ms / 1000);
  return total < 60 ? `${total} s` : `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

/** What voice apps should pick as their microphone, if Kavox knows. */
export function micLabel(status: VirtualStatus): string | null {
  return status.kind === 'ok' || status.kind === 'cableIsDefaultOutput' ? status.micLabel : null;
}
