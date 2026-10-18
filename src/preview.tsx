// Dev-only UI preview with a mocked backend: `npm run dev`, then open /preview.html?view=main|wizard|settings|editor|empty&platform=windows|linux&theme=dark|light.
import { createRoot } from 'react-dom/client';
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import '@fontsource-variable/archivo/wdth.css';
import './styles.css';
import { App } from './App';
import { PALETTE, type Sound, type UiState } from './api';
import { openSettings, useStore } from './store';

const params = new URLSearchParams(location.search);
const view = params.get('view') ?? 'main';
const platform = params.get('platform') ?? 'windows';
const theme = (params.get('theme') ?? 'dark') as 'dark' | 'light';

const names = [
  ['Airhorn', 'Ctrl+1', 2300], ['Vine boom', 'Ctrl+2', 1100], ['Bruh', 'Ctrl+3', 900], ['Sad trombone', 'Ctrl+4', 4200],
  ['Applause', 'Numpad5', 6100], ['Ba dum tss', 'F13', 1600], ['Emotional damage', null, 2500], ['Windows XP startup', null, 4100],
  ['Rizz', 'Ctrl+Alt+R', 800], ['Lofi beat (loop)', null, 94000], ['Crickets', null, 5000], ['Wow', 'Shift+W', 700],
] as const;

const sounds: Record<string, Sound> = {};
names.forEach(([name, hotkey, durationMs], i) => {
  const id = `s${i}`;
  sounds[id] = {
    id, name, file: `${id}.mp3`, sha256: id, durationMs, volume: 1, normGainDb: 0,
    color: PALETTE[i % PALETTE.length], hotkey, retrigger: i === 4 ? 'overlap' : i === 10 ? 'toggle' : 'restart', loop: i === 9,
  };
});

const empty = view === 'empty';
const cableLabel = platform === 'windows' ? 'CABLE Output (VB-Audio Virtual Cable)' : 'Kavox Mic';
const state: UiState = {
  settings: {
    schema: 1,
    audio: {
      micDevice: null, virtualOutput: null, monitorOutput: null, micPassthrough: true, micGain: 1, micMuted: false,
      soundsToMicGain: 0.8, soundsToMonitorGain: 0.5, monitorEnabled: true, normalize: true,
    },
    hotkeys: { stopAll: 'Ctrl+0', toggleMute: 'Ctrl+Shift+M' },
    app: { startMinimized: false, closeToTray: true, launchOnLogin: false, theme, setupComplete: view !== 'wizard' },
  },
  library: {
    schema: 1,
    activeBoard: 'main',
    boards: [
      { id: 'main', name: 'Main', sounds: empty ? [] : Object.keys(sounds) },
      { id: 'memes', name: 'Memes', sounds: [] },
      { id: 'stream', name: 'Stream alerts', sounds: [] },
    ],
    sounds: empty ? {} : sounds,
  },
  devices: {
    inputs: [{ id: 'mic', label: 'Microphone (Blue Yeti)', isDefault: true }],
    outputs: [
      { id: 'hp', label: 'Headphones (Realtek Audio)', isDefault: true },
      { id: 'cable', label: 'CABLE Input (VB-Audio Virtual Cable)', isDefault: false },
    ],
    virtualCandidates: platform === 'windows' ? ['cable'] : [],
  },
  virtualStatus:
    params.get('status') === 'missing'
      ? { kind: 'notInstalled' }
      : { kind: 'ok', device: platform === 'windows' ? 'CABLE Input (VB-Audio Virtual Cable)' : 'Kavox Mic', micLabel: cableLabel },
  streams: {
    mic: { device: 'mic', label: 'Microphone (Blue Yeti)', rate: 48000, fallback: false },
    virtual: { device: 'cable', label: 'CABLE Input', rate: 48000, fallback: false },
    monitor: { device: 'hp', label: 'Headphones', rate: 48000, fallback: false },
  },
  playing: [],
  notices: [],
  platform,
  backend: 'preview',
  readOnly: false,
  version: '0.1.0',
  hotkeysSupported: platform === 'windows',
  autostartEnabled: false,
};

mockWindows('main');
mockIPC((cmd) => (cmd === 'get_state' ? state : null), { shouldMockEvents: true });

createRoot(document.getElementById('root')!).render(<App />);

// Once loaded, show a few pads playing and live meters.
const ready = setInterval(() => {
  if (!useStore.getState().ready) return;
  clearInterval(ready);
  if (!empty) {
    void emit('playback', { soundId: 's0', voiceId: 1, state: 'progress', frame: 40_000, frames: 110_400 });
    void emit('playback', { soundId: 's5', voiceId: 2, state: 'progress', frame: 60_000, frames: 76_800 });
  }
  void emit('levels', { mic: 0.18, virtual: 0.5, monitor: 0.3 });
  if (view === 'settings') openSettings((params.get('tab') as 'audio') ?? 'audio');
  if (view === 'editor') useStore.setState({ editing: 's3' });
  if (view === 'toast') void emit('notice', { level: 'warn', message: 'Headphones (Realtek Audio) isn’t connected, so you’ll hear sounds on the default output.' });
}, 20);
