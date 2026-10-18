import { useRef, type CSSProperties, type KeyboardEvent } from 'react';
import { DndContext, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent } from '@dnd-kit/core';
import { SortableContext, arrayMove, rectSortingStrategy, useSortable } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { api, type Sound } from '../api';
import { activeBoard, confirmDeleteSound, formatDuration, pickAndImport, reorder, run, useStore } from '../store';
import { Icon, Keycaps } from './ui';

export function PadGrid() {
  const library = useStore((s) => s.library);
  const search = useStore((s) => s.search);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }));
  const gridRef = useRef<HTMLDivElement>(null);

  const board = activeBoard(library);
  if (!board) return null;
  const all = board.sounds.map((id) => library.sounds[id]).filter((s): s is Sound => Boolean(s));
  const query = search.trim().toLowerCase();
  const sounds = query ? all.filter((s) => s.name.toLowerCase().includes(query)) : all;

  if (all.length === 0) return <EmptyBoard boardName={board.name} />;
  if (sounds.length === 0) {
    return (
      <p className="empty-search">
        No sounds on {board.name} match "{search.trim()}".
      </p>
    );
  }

  const move = (id: string, to: number) => {
    const from = board.sounds.indexOf(id);
    const target = Math.max(0, Math.min(board.sounds.length - 1, to));
    if (from !== target) reorder(board.id, arrayMove(board.sounds, from, target));
  };

  const onDragEnd = (e: DragEndEvent) => {
    if (e.over && e.active.id !== e.over.id) move(String(e.active.id), board.sounds.indexOf(String(e.over.id)));
  };

  // Arrow keys move focus across the grid; Alt + arrow moves the pad itself.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: 'up', ArrowDown: 'down' }[e.key];
    if (step === undefined || !gridRef.current) return;
    const pads = Array.from(gridRef.current.querySelectorAll<HTMLButtonElement>('[data-pad]'));
    const index = pads.indexOf(document.activeElement as HTMLButtonElement);
    if (index < 0) return;
    const columns = pads.filter((p) => p.offsetTop === pads[0].offsetTop).length || 1;
    const delta = step === 'up' ? -columns : step === 'down' ? columns : (step as number);
    const next = index + delta;
    if (next < 0 || next >= pads.length) return;
    e.preventDefault();
    if (e.altKey && !query) {
      const id = pads[index].dataset.pad!;
      move(id, board.sounds.indexOf(pads[next].dataset.pad!));
      requestAnimationFrame(() => gridRef.current?.querySelector<HTMLButtonElement>(`[data-pad="${id}"]`)?.focus());
    } else {
      pads[next].focus();
    }
  };

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
      <SortableContext items={sounds.map((s) => s.id)} strategy={rectSortingStrategy} disabled={Boolean(query)}>
        <div className="pad-grid" ref={gridRef} onKeyDown={onKeyDown} role="list" aria-label={`Sounds on ${board.name}`}>
          {sounds.map((s) => (
            <Pad key={s.id} sound={s} />
          ))}
          {!query && (
            <div role="listitem" className="pad-slot">
              <button type="button" className="pad pad-add" onClick={() => void pickAndImport()}>
                <Icon name="plus" size={22} />
                <span>Add sounds</span>
              </button>
            </div>
          )}
        </div>
      </SortableContext>
    </DndContext>
  );
}

function Pad({ sound }: { sound: Sound }) {
  const voices = useStore((s) => s.playing[sound.id]);
  const { listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: sound.id });
  const padRef = useRef<HTMLButtonElement>(null);

  const playing = Boolean(voices?.length);
  const latest = voices?.[voices.length - 1];
  const progress = latest && latest.frames ? Math.min(1, latest.frame / latest.frames) : 0;
  const style = {
    transform: CSS.Translate.toString(transform),
    transition,
    '--pad': sound.color,
    '--progress': playing ? 1 - progress : 0,
  } as CSSProperties;

  const play = () => {
    const pad = padRef.current;
    if (pad) {
      // Restart the hit flash even on rapid presses.
      pad.classList.remove('is-hit');
      void pad.offsetWidth;
      pad.classList.add('is-hit');
    }
    void run(api.playSound(sound.id));
  };

  const edit = () => useStore.setState({ editing: sound.id });

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (e.key === 'F2') {
      e.preventDefault();
      edit();
    } else if (e.key === 'Delete') {
      e.preventDefault();
      confirmDeleteSound(sound.id);
    }
  };

  const modeIcon = sound.loop ? 'loop' : sound.retrigger === 'toggle' ? 'toggle' : sound.retrigger === 'overlap' ? 'overlap' : null;
  const modeLabel = sound.loop ? 'Loops' : sound.retrigger === 'toggle' ? 'Press again to stop' : 'Overlaps';

  return (
    <div
      role="listitem"
      ref={setNodeRef}
      style={style}
      className={`pad-slot ${playing ? 'is-playing' : ''} ${isDragging ? 'is-dragging' : ''}`}
    >
      <button
        ref={padRef}
        type="button"
        className="pad"
        data-pad={sound.id}
        onClick={play}
        onContextMenu={(e) => {
          e.preventDefault();
          edit();
        }}
        onKeyDown={onKeyDown}
        onAnimationEnd={() => padRef.current?.classList.remove('is-hit')}
        aria-label={`${sound.name}${sound.hotkey ? `, hotkey ${sound.hotkey}` : ''}`}
        {...listeners}
      >
        <span className="pad-led" aria-hidden="true" />
        <span className="pad-name">{sound.name}</span>
        <span className="pad-foot">
          {sound.hotkey ? <Keycaps hotkey={sound.hotkey} small /> : <span />}
          <span className="pad-info">
            {modeIcon && (
              <span className="pad-mode" title={modeLabel}>
                <Icon name={modeIcon} size={13} />
              </span>
            )}
            {formatDuration(sound.durationMs)}
          </span>
        </span>
        <span className="pad-progress" aria-hidden="true" />
      </button>
      {playing && (
        <button type="button" className="pad-stop" aria-label={`Stop ${sound.name}`} onClick={() => void run(api.stopSound(sound.id))}>
          <Icon name="stop" size={11} />
          Stop
        </button>
      )}
      <button type="button" className="pad-corner pad-edit" aria-label={`Edit ${sound.name}`} onClick={edit}>
        <Icon name="dots" size={16} />
      </button>
    </div>
  );
}

function EmptyBoard({ boardName }: { boardName: string }) {
  return (
    <div className="empty-board">
      <div className="empty-pads" aria-hidden="true">
        {Array.from({ length: 8 }, (_, i) => (
          <span key={i} />
        ))}
      </div>
      <h2>{boardName} has no sounds yet</h2>
      <p>Drop audio files anywhere in this window, or pick them from your computer. WAV, MP3, OGG and FLAC all work.</p>
      <button type="button" className="button button-primary" onClick={() => void pickAndImport()}>
        <Icon name="plus" />
        Add sounds
      </button>
    </div>
  );
}
