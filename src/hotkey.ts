// Turns key presses into accelerator strings the global-hotkey crate accepts (W3C `KeyboardEvent.code` names).

const MODIFIER_CODES = new Set([
  'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'ShiftLeft', 'ShiftRight', 'MetaLeft', 'MetaRight', 'OSLeft', 'OSRight',
]);

const NAMED_CODES = new Set([
  'Backquote', 'Backslash', 'BracketLeft', 'BracketRight', 'Pause', 'Comma', 'Equal', 'Minus', 'Period', 'Quote',
  'Semicolon', 'Slash', 'CapsLock', 'Enter', 'Space', 'Tab', 'Delete', 'End', 'Home', 'Insert', 'PageDown', 'PageUp',
  'PrintScreen', 'ScrollLock', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'NumLock', 'NumpadAdd',
  'NumpadDecimal', 'NumpadDivide', 'NumpadEnter', 'NumpadEqual', 'NumpadMultiply', 'NumpadSubtract',
  'AudioVolumeDown', 'AudioVolumeUp', 'AudioVolumeMute', 'MediaPlayPause', 'MediaStop', 'MediaTrackNext',
  'MediaTrackPrevious', 'Backspace',
  ...Array.from({ length: 10 }, (_, i) => `Numpad${i}`),
  ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`),
]);

/** Keys that are part of normal typing; binding them alone swallows them system-wide. */
const TYPING_KEYS = new Set([
  'Space', 'Enter', 'Tab', 'Backspace', 'Minus', 'Equal', 'Comma', 'Period', 'Slash', 'Semicolon', 'Quote',
  'BracketLeft', 'BracketRight', 'Backslash', 'Backquote', 'Delete', 'CapsLock',
]);

export type RecordResult =
  | { kind: 'pending' }
  | { kind: 'cancel' }
  | { kind: 'clear' }
  | { kind: 'set'; hotkey: string }
  | { kind: 'unsupported'; key: string };

export function fromKeyboardEvent(e: KeyboardEvent): RecordResult {
  if (MODIFIER_CODES.has(e.code)) return { kind: 'pending' };
  const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(
    (m): m is string => Boolean(m),
  );
  if (mods.length === 0 && e.code === 'Escape') return { kind: 'cancel' };
  if (mods.length === 0 && (e.code === 'Backspace' || e.code === 'Delete')) return { kind: 'clear' };
  const key = keyToken(e.code);
  if (!key) return { kind: 'unsupported', key: e.key || e.code };
  return { kind: 'set', hotkey: [...mods, key].join('+') };
}

function keyToken(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  return NAMED_CODES.has(code) ? code : null;
}

const PRETTY: Record<string, string> = {
  Backquote: '`', Backslash: '\\', BracketLeft: '[', BracketRight: ']', Comma: ',', Equal: '=', Minus: '-',
  Period: '.', Quote: "'", Semicolon: ';', Slash: '/', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←',
  ArrowRight: '→', NumpadAdd: 'Num +', NumpadSubtract: 'Num −', NumpadMultiply: 'Num ×', NumpadDivide: 'Num ÷',
  NumpadDecimal: 'Num .', NumpadEnter: 'Num Enter', NumpadEqual: 'Num =', PageUp: 'Page Up', PageDown: 'Page Down',
  PrintScreen: 'Print Screen', AudioVolumeUp: 'Volume up', AudioVolumeDown: 'Volume down', AudioVolumeMute: 'Mute',
  MediaPlayPause: 'Play/Pause', MediaStop: 'Media stop', MediaTrackNext: 'Next track', MediaTrackPrevious: 'Previous track',
};

/** Keycap labels for display, e.g. "Ctrl+Numpad5" → ["Ctrl", "Num 5"]. */
export function keycaps(hotkey: string, platform: string): string[] {
  return hotkey.split('+').map((part) => {
    if (part === 'Super') return platform === 'windows' ? 'Win' : 'Super';
    const numpad = /^Numpad([0-9])$/.exec(part);
    if (numpad) return `Num ${numpad[1]}`;
    return PRETTY[part] ?? part;
  });
}

/** True when a binding would swallow a key people type with. */
export function isRisky(hotkey: string): boolean {
  const parts = hotkey.split('+');
  const key = parts[parts.length - 1];
  const mods = parts.slice(0, -1);
  if (mods.some((m) => m !== 'Shift')) return false;
  return /^[A-Z0-9]$/.test(key) || TYPING_KEYS.has(key);
}
