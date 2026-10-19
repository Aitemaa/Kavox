import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { errorMessage, type DeviceInfo } from '../api';
import { fromKeyboardEvent, isRisky, keycaps } from '../hotkey';
import { useStore } from '../store';

const ICONS = {
  plus: <path d="M12 5v14M5 12h14" />,
  gear: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1.08-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1.08 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </>
  ),
  mic: (
    <>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
    </>
  ),
  micOff: <path d="M15 9.34V6a3 3 0 0 0-5.94-.6M9 9v2a3 3 0 0 0 5.12 2.12M19 11a7 7 0 0 1-.78 3.22M5 11a7 7 0 0 0 11.5 5.36M12 18v3M3 3l18 18" />,
  headphones: (
    <>
      <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
      <path d="M21 19a2 2 0 0 1-2 2h-1v-6h3zM3 19a2 2 0 0 0 2 2h1v-6H3z" />
    </>
  ),
  broadcast: (
    <>
      <path d="M4.9 19.1a10 10 0 0 1 0-14.2M19.1 4.9a10 10 0 0 1 0 14.2M7.8 16.2a6 6 0 0 1 0-8.4M16.2 7.8a6 6 0 0 1 0 8.4" />
      <circle cx="12" cy="12" r="2" />
    </>
  ),
  stop: <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none" />,
  play: <path d="M8 5.5v13l10.5-6.5z" fill="currentColor" stroke="none" />,
  loop: (
    <>
      <path d="m17 2 4 4-4 4" />
      <path d="M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4" />
      <path d="M21 13v2a3 3 0 0 1-3 3H3" />
    </>
  ),
  restart: (
    <>
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
      <path d="M3 3v5h5" />
    </>
  ),
  overlap: (
    <>
      <path d="m12 2 10 5-10 5L2 7z" />
      <path d="m2 17 10 5 10-5M2 12l10 5 10-5" />
    </>
  ),
  toggle: (
    <>
      <path d="M12 2v10" />
      <path d="M18.4 6.6a9 9 0 1 1-12.8 0" />
    </>
  ),
  close: <path d="M18 6 6 18M6 6l12 12" />,
  trash: <path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14" />,
  check: <path d="M20 6 9 17l-5-5" />,
  warning: (
    <>
      <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" />
      <path d="M12 9v4M12 17h.01" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 16v-4M12 8h.01" />
    </>
  ),
  folder: <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />,
  copy: (
    <>
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </>
  ),
  external: <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6M15 3h6v6M10 14 21 3" />,
  dots: (
    <>
      <circle cx="5" cy="12" r="1.6" fill="currentColor" />
      <circle cx="12" cy="12" r="1.6" fill="currentColor" />
      <circle cx="19" cy="12" r="1.6" fill="currentColor" />
    </>
  ),
  pencil: (
    <>
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
    </>
  ),
};

export type IconName = keyof typeof ICONS;

export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {ICONS[name]}
    </svg>
  );
}

const BRAND_RED = '#e8262d';

/** The logo's K (as in assets/icon.svg): a stem, a dot where the arms meet, then the arms. Dash, dot, dash is K in Morse code. */
function logoK(x: number, y: number, h: number, w: number) {
  const gap = 0.36 * w;
  const tip = x + 0.74 * h;
  const dot = { cx: x + w + gap, cy: y + h / 2, r: w / 2 };
  const arms = [y, y + h].map((ty) => {
    const n = Math.hypot(tip - dot.cx, ty - dot.cy);
    const s = w + gap;
    return { x1: dot.cx + ((tip - dot.cx) / n) * s, y1: dot.cy + ((ty - dot.cy) / n) * s, x2: tip, y2: ty };
  });
  return { lines: [{ x1: x, y1: y, x2: x, y2: y + h }, ...arms], dot };
}

const MARK_K = logoK(9.85, 7.87, 16.27, 2.4);
const WORD_K = logoK(0, 0, 100, 10);

/** Speckles the logo's white strokes like the icon: grey noise multiplied in. `frequency` is per user unit (~0.65 per pixel). */
function Grain({ id, frequency }: { id: string; frequency: number }) {
  const ramp = { type: 'linear', slope: 0.883, intercept: 0.443 } as const;
  return (
    <filter id={id} x="-5%" y="-5%" width="110%" height="110%" colorInterpolationFilters="sRGB">
      <feTurbulence type="fractalNoise" baseFrequency={frequency} numOctaves={3} seed={4} result="noise" />
      <feColorMatrix in="noise" type="matrix" values="0.33 0.33 0.33 0 0 0.33 0.33 0.33 0 0 0.33 0.33 0.33 0 0 0 0 0 0 1" result="grey" />
      <feComponentTransfer in="grey" result="mask">
        <feFuncR {...ramp} />
        <feFuncG {...ramp} />
        <feFuncB {...ramp} />
      </feComponentTransfer>
      <feComposite in="SourceGraphic" in2="mask" operator="arithmetic" k1={1} k2={0} k3={0} k4={0} />
    </filter>
  );
}

export function LogoMark({ size = 28 }: { size?: number }) {
  const grain = `logo-grain${useId().replace(/:/g, '')}`;
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" className="logo-mark">
      <defs>
        <Grain id={grain} frequency={(0.65 * size) / 32} />
      </defs>
      <rect width="32" height="32" rx="7.2" fill="#222" />
      <g stroke="#fff" strokeWidth={2.4} strokeLinecap="round" filter={`url(#${grain})`}>
        {MARK_K.lines.map((l, i) => (
          <line key={i} {...l} />
        ))}
      </g>
      <circle {...MARK_K.dot} fill={BRAND_RED} />
    </svg>
  );
}

/** Line-drawn "KAVOX" (as in assets/banner.svg) on a 100-unit cap height, kerned by eye. */
export function Wordmark({ height = 16 }: { height?: number }) {
  const grain = `word-grain${useId().replace(/:/g, '')}`;
  return (
    <svg className="wordmark" width={(height * 485) / 110} height={height} viewBox="-5 -5 485 110" role="img" aria-label="Kavox">
      <defs>
        <Grain id={grain} frequency={(0.65 * height) / 110} />
      </defs>
      <g fill="none" stroke="currentColor" strokeWidth={10} strokeLinecap="round" strokeLinejoin="round" filter={`url(#${grain})`}>
        {WORD_K.lines.map((l, i) => (
          <line key={i} {...l} />
        ))}
        <polyline points="110,100 149,0 188,100" />
        <polyline points="188,0 227,100 266,0" />
        <circle cx={339} cy={50} r={50} />
        <path d="M409 0 475 100M475 0 409 100" />
      </g>
      <circle {...WORD_K.dot} fill={BRAND_RED} />
    </svg>
  );
}

export function Modal({
  open,
  onClose,
  label,
  className = '',
  children,
}: {
  open: boolean;
  onClose: () => void;
  label: string;
  className?: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    const target = ref.current?.querySelector<HTMLElement>('[data-autofocus]') ?? ref.current;
    target?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onCloseRef.current();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      previous?.focus?.();
    };
  }, [open]);

  if (!open) return null;
  return createPortal(
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`dialog ${className}`} role="dialog" aria-modal="true" aria-label={label} ref={ref} tabIndex={-1}>
        {children}
      </div>
    </div>,
    document.body,
  );
}

export function Switch({
  checked,
  onChange,
  label,
  description,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: ReactNode;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="field switch-field">
      <div className="field-text">
        <label htmlFor={id}>{label}</label>
        {description && <p className="field-hint">{description}</p>}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        className="switch"
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="switch-thumb" />
      </button>
    </div>
  );
}

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
  icon?: IconName;
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: SegmentOption<T>[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          className={o.value === value ? 'is-on' : ''}
          onClick={() => onChange(o.value)}
        >
          {o.icon && <Icon name={o.icon} size={15} />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** `null` means the system default. A saved device that's unplugged still shows, marked as such. */
export function DeviceSelect({
  label,
  value,
  devices,
  onChange,
  defaultLabel = 'System default',
  hint,
}: {
  label: string;
  value: string | null;
  devices: DeviceInfo[];
  onChange: (id: string | null) => void;
  defaultLabel?: string;
  hint?: ReactNode;
}) {
  const id = useId();
  const missing = value !== null && !devices.some((d) => d.id === value);
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <select id={id} value={value ?? ''} onChange={(e) => onChange(e.target.value || null)}>
        <option value="">{defaultLabel}</option>
        {devices.map((d) => (
          <option key={d.id} value={d.id}>
            {d.label}
            {d.isDefault ? ' (default)' : ''}
          </option>
        ))}
        {missing && <option value={value}>{value} (not connected)</option>}
      </select>
      {hint && <p className="field-hint">{hint}</p>}
    </div>
  );
}

export function Keycaps({ hotkey, small }: { hotkey: string; small?: boolean }) {
  const platform = useStore((s) => s.platform);
  return (
    <span className={`keycaps ${small ? 'keycaps-small' : ''}`} aria-label={hotkey}>
      {keycaps(hotkey, platform).map((k, i) => (
        <kbd key={i}>{k}</kbd>
      ))}
    </span>
  );
}

export function HotkeyRecorder({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string | null;
  onChange: (hotkey: string | null) => Promise<unknown>;
}) {
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const id = useId();

  const commit = async (hotkey: string | null) => {
    setRecording(false);
    try {
      await onChange(hotkey);
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const onKeyDown = (e: ReactKeyboardEvent) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const result = fromKeyboardEvent(e.nativeEvent);
    if (result.kind === 'cancel') setRecording(false);
    else if (result.kind === 'clear') void commit(null);
    else if (result.kind === 'set') void commit(result.hotkey);
    else if (result.kind === 'unsupported') setError(`${result.key} can't be used as a hotkey. Try a letter, number, F-key or numpad key.`);
  };

  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <div className="hotkey-field">
        <button
          id={id}
          type="button"
          className={`hotkey-button ${recording ? 'is-recording' : ''}`}
          onClick={() => {
            setError(null);
            setRecording((r) => !r);
          }}
          onKeyDown={onKeyDown}
          onBlur={() => setRecording(false)}
        >
          {recording ? 'Press a key combination…' : value ? <Keycaps hotkey={value} /> : <span className="muted">Not set</span>}
        </button>
        {value && !recording && (
          <button type="button" className="icon-button" aria-label={`Clear ${label}`} onClick={() => void commit(null)}>
            <Icon name="close" size={16} />
          </button>
        )}
      </div>
      {recording && <p className="field-hint">Esc cancels, Backspace clears.</p>}
      {!recording && value && isRisky(value) && (
        <p className="field-hint is-warn">
          Other apps won't receive this key while Kavox runs. Add Ctrl or Alt, or use F13–F24 or the numpad.
        </p>
      )}
      {error && <p className="field-hint is-error">{error}</p>}
    </div>
  );
}

/** Perceptual fader: position² × max, so the middle of the track is about a quarter of max gain. */
export function Fader({
  label,
  value,
  max,
  level,
  onChange,
}: {
  label: string;
  value: number;
  max: number;
  level?: number;
  onChange: (value: number) => void;
}) {
  const position = Math.sqrt(Math.max(0, value) / max);
  return (
    <div className="fader">
      {level !== undefined && <MeterFill className="fader-level" level={level} />}
      <input
        type="range"
        min={0}
        max={1000}
        value={Math.round(position * 1000)}
        aria-label={label}
        aria-valuetext={`${Math.round(value * 100)}%`}
        onChange={(e) => onChange(Number((max * (Number(e.target.value) / 1000) ** 2).toFixed(3)))}
      />
    </div>
  );
}

export function LevelBar({ level, label }: { level: number; label: string }) {
  return (
    <div className="level-bar" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={1} aria-valuenow={meterFill(level)}>
      <MeterFill className="level-bar-fill" level={level} />
    </div>
  );
}

/** The lit part of a meter; it turns red near 0 dBFS, where the limiter starts working. */
function MeterFill({ className, level }: { className: string; level: number }) {
  const fill = meterFill(level);
  return <div className={`${className} ${fill > 0.95 ? 'is-hot' : ''}`} style={{ '--fill': fill } as CSSProperties} />;
}

/** Peak (0–1) to meter fill on a −60…0 dBFS scale. */
export function meterFill(peak: number) {
  if (peak <= 0) return 0;
  return Math.min(1, Math.max(0, (20 * Math.log10(peak) + 60) / 60));
}
