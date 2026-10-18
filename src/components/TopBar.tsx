import { useEffect, useRef, useState } from 'react';
import { api, type Board } from '../api';
import { askConfirm, openSettings, pickAndImport, run, useStore } from '../store';
import { Icon, LogoMark } from './ui';

export function TopBar() {
  return (
    <header className="topbar">
      <div className="brand">
        <LogoMark />
        <span className="wordmark">Kavox</span>
      </div>
      <BoardTabs />
      <div className="topbar-actions">
        <SearchBox />
        <button type="button" className="button button-primary" onClick={() => void pickAndImport()}>
          <Icon name="plus" />
          <span className="hide-narrow">Add sounds</span>
        </button>
        <button type="button" className="icon-button" aria-label="Settings" onClick={() => openSettings()}>
          <Icon name="gear" />
        </button>
      </div>
    </header>
  );
}

function BoardTabs() {
  const boards = useStore((s) => s.library.boards);
  const active = useStore((s) => s.library.activeBoard);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [menu, setMenu] = useState<{ id: string; anchor: DOMRect } | null>(null);

  return (
    <nav className="boards" aria-label="Boards">
      <div className="board-tabs" role="tablist">
        {boards.map((board) =>
          renaming === board.id ? (
            <NameInput
              key={board.id}
              initial={board.name}
              label="Board name"
              onDone={(name) => {
                setRenaming(null);
                if (name && name !== board.name) void run(api.renameBoard(board.id, name));
              }}
            />
          ) : (
            <div key={board.id} className={`board-tab ${board.id === active ? 'is-active' : ''}`}>
              <button
                type="button"
                role="tab"
                aria-selected={board.id === active}
                onClick={() => board.id !== active && void run(api.setActiveBoard(board.id))}
                onDoubleClick={() => setRenaming(board.id)}
              >
                {board.name}
                <span className="board-count">{board.sounds.length}</span>
              </button>
              {board.id === active && (
                <button
                  type="button"
                  className="board-menu-button"
                  aria-label={`${board.name} options`}
                  aria-expanded={menu?.id === board.id}
                  onClick={(e) =>
                    setMenu(menu?.id === board.id ? null : { id: board.id, anchor: e.currentTarget.getBoundingClientRect() })
                  }
                >
                  <Icon name="dots" size={14} />
                </button>
              )}
              {menu?.id === board.id && (
                <BoardMenu
                  board={board}
                  anchor={menu.anchor}
                  canDelete={boards.length > 1}
                  onClose={() => setMenu(null)}
                  onRename={() => {
                    setMenu(null);
                    setRenaming(board.id);
                  }}
                />
              )}
            </div>
          ),
        )}
        {creating ? (
          <NameInput
            initial=""
            label="New board name"
            placeholder="New board"
            onDone={(name) => {
              setCreating(false);
              if (name) void run(api.createBoard(name));
            }}
          />
        ) : (
          <button type="button" className="icon-button board-add" aria-label="New board" onClick={() => setCreating(true)}>
            <Icon name="plus" size={16} />
          </button>
        )}
      </div>
    </nav>
  );
}

function BoardMenu({
  board,
  anchor,
  canDelete,
  onClose,
  onRename,
}: {
  board: Board;
  anchor: DOMRect;
  canDelete: boolean;
  onClose: () => void;
  onRename: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector('button')?.focus();
    const onDown = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && onClose();
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('mousedown', onDown);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', onDown);
      window.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  return (
    <div className="menu" role="menu" ref={ref} style={{ top: anchor.bottom + 6, left: Math.max(8, anchor.right - 200) }}>
      <button type="button" role="menuitem" onClick={onRename}>
        <Icon name="pencil" size={15} />
        Rename board
      </button>
      <button
        type="button"
        role="menuitem"
        className="is-danger"
        disabled={!canDelete}
        title={canDelete ? undefined : 'Kavox needs at least one board'}
        onClick={() => {
          onClose();
          askConfirm({
            title: `Delete ${board.name}?`,
            message:
              board.sounds.length > 0
                ? `Its ${board.sounds.length} ${board.sounds.length === 1 ? 'sound is' : 'sounds are'} removed from Kavox too. Original files on your computer aren't touched.`
                : 'The board is empty, so nothing else is removed.',
            action: 'Delete board',
            onConfirm: () => void run(api.deleteBoard(board.id)),
          });
        }}
      >
        <Icon name="trash" size={15} />
        Delete board
      </button>
    </div>
  );
}

function NameInput({ initial, label, placeholder, onDone }: { initial: string; label: string; placeholder?: string; onDone: (name: string) => void }) {
  const [value, setValue] = useState(initial);
  const done = useRef(false);
  const finish = (name: string) => {
    if (done.current) return;
    done.current = true;
    onDone(name.trim());
  };
  return (
    <input
      className="board-name-input"
      aria-label={label}
      placeholder={placeholder}
      value={value}
      maxLength={60}
      autoFocus
      onFocus={(e) => e.target.select()}
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => finish(value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') finish(value);
        if (e.key === 'Escape') finish('');
      }}
    />
  );
}

function SearchBox() {
  const search = useStore((s) => s.search);
  return (
    <div className="search">
      <Icon name="search" size={16} />
      <input
        id="sound-search"
        type="search"
        placeholder="Find a sound"
        aria-label="Find a sound"
        value={search}
        onChange={(e) => useStore.setState({ search: e.target.value })}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            useStore.setState({ search: '' });
            (e.target as HTMLInputElement).blur();
          }
          if (e.key === 'Enter') document.querySelector<HTMLButtonElement>('[data-pad]')?.focus();
        }}
      />
    </div>
  );
}
