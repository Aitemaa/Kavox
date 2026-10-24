import type { IconName } from './components/ui';

export interface VoiceInfo {
  id: string;
  name: string;
  icon: IconName;
  color: string;
  /** Mirrors `voice.rs` in kavox-core, which is what the audio engine actually uses. */
  semitones: number;
  wobble?: boolean;
}

export const NORMAL = 'normal';
export const CUSTOM = 'custom';

export const VOICES: VoiceInfo[] = [
  { id: 'chipmunk', name: 'Chipmunk', icon: 'chipmunk', color: '#f0a020', semitones: 10 },
  { id: 'pixie', name: 'Pixie', icon: 'pixie', color: '#f0607e', semitones: 5 },
  { id: 'deep', name: 'Deep', icon: 'deep', color: '#3b9cff', semitones: -4 },
  { id: 'giant', name: 'Giant', icon: 'giant', color: '#7d8c5a', semitones: -8 },
  { id: 'monster', name: 'Monster', icon: 'monster', color: '#a3a29d', semitones: -12 },
  { id: 'wobbly', name: 'Wobbly', icon: 'wobbly', color: '#1fb5a0', semitones: 0, wobble: true },
  { id: 'drunk', name: 'Drunk', icon: 'drunk', color: '#c9ad7a', semitones: -2, wobble: true },
  { id: 'alien', name: 'Alien', icon: 'alien', color: '#1fb5a0', semitones: 3, wobble: true },
];

export function voiceName(id: string): string {
  if (id === CUSTOM) return 'Custom';
  return VOICES.find((v) => v.id === id)?.name ?? 'Normal';
}

/** "+5 st", "−4 st" or "0 st", with a real minus sign. */
export function formatSemitones(st: number): string {
  const rounded = Math.round(st * 10) / 10;
  if (rounded === 0) return '0 st';
  return `${rounded > 0 ? '+' : '−'}${Math.abs(rounded)} st`;
}
