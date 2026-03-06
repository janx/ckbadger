'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { useRouter } from '@/src/navigation';
import { focusGlobalSearchInput, isEditableElement } from '@/lib/search-focus';
import { resolveSearchRoute } from '@/lib/search-routing';

interface CommandItem {
  id: string;
  label: string;
  keywords: string[];
  href: string;
}

interface ShortcutHint {
  keys: string;
  description: string;
}

const COMMANDS: CommandItem[] = [
  { id: 'go-home', label: 'Go to Home', keywords: ['home', 'index'], href: '/' },
  { id: 'go-blocks', label: 'Go to Blocks', keywords: ['block', 'blocks'], href: '/blocks' },
  {
    id: 'go-transactions',
    label: 'Go to Transactions',
    keywords: ['tx', 'transaction', 'transactions'],
    href: '/transactions',
  },
  { id: 'go-dao', label: 'Go to DAO', keywords: ['dao', 'deposit'], href: '/dao' },
  { id: 'go-assets', label: 'Go to Assets', keywords: ['asset', 'token', 'nft'], href: '/assets' },
  { id: 'go-scripts', label: 'Go to Scripts', keywords: ['script'], href: '/scripts' },
  { id: 'go-charts', label: 'Go to Charts', keywords: ['chart', 'stats'], href: '/charts' },
  {
    id: 'go-hardforks',
    label: 'Go to Hardforks',
    keywords: ['hardfork', 'hard fork', 'upgrade', 'network upgrade'],
    href: '/hardforks',
  },
];

const GOTO_CHORD_TIMEOUT_MS = 1200;

const GOTO_SHORTCUTS: Record<string, { href?: string; focusSearch?: boolean }> = {
  h: { href: '/' },
  b: { href: '/blocks' },
  t: { href: '/transactions' },
  d: { href: '/dao' },
  a: { href: '/assets' },
  s: { href: '/scripts' },
  c: { href: '/charts' },
};

const SHORTCUT_HINTS: ShortcutHint[] = [
  { keys: 'Ctrl/Cmd+K', description: 'Open command palette' },
  { keys: '?', description: 'Open keyboard shortcuts help' },
  { keys: '/', description: 'Focus global search bar' },
  { keys: 'g b', description: 'Go to Blocks' },
  { keys: 'g t', description: 'Go to Transactions' },
  { keys: 'g d', description: 'Go to DAO' },
  { keys: 'g a', description: 'Go to Assets' },
  { keys: 'g s', description: 'Go to Scripts' },
  { keys: 'g c', description: 'Go to Charts' },
  { keys: 'g h', description: 'Go to Home' },
  { keys: 'Esc', description: 'Close panel' },
];

type PaletteMode = 'commands' | 'shortcuts';

function normalize(value: string): string {
  return value.trim().toLowerCase();
}

function matchesCommand(command: CommandItem, query: string): boolean {
  if (!query) return true;
  const haystack = [command.label, ...command.keywords].join(' ').toLowerCase();
  return haystack.includes(query);
}

export function CommandPalette() {
  const router = useRouter();
  const inputRef = useRef<HTMLInputElement>(null);
  const [isOpen, setIsOpen] = useState(false);
  const [mode, setMode] = useState<PaletteMode>('commands');
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [isGotoChordActive, setIsGotoChordActive] = useState(false);
  const gotoTimerRef = useRef<number | null>(null);

  const normalizedQuery = normalize(query);

  const matchingCommands = useMemo(
    () => COMMANDS.filter((command) => matchesCommand(command, normalizedQuery)),
    [normalizedQuery]
  );

  useEffect(() => {
    if (!isOpen) return;
    setSelectedIndex(0);
  }, [matchingCommands.length, isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    if (mode !== 'commands') return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [isOpen, mode]);

  const closePalette = () => {
    setIsOpen(false);
    setMode('commands');
    setQuery('');
  };

  useEffect(() => {
    return () => {
      if (gotoTimerRef.current !== null) {
        window.clearTimeout(gotoTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    const handleWindowKeyDown = (event: KeyboardEvent) => {
      const isEditable = isEditableElement(event.target);
      const isMetaOrCtrl = event.metaKey || event.ctrlKey;
      const key = event.key.toLowerCase();

      if (isMetaOrCtrl && !event.altKey && key === 'k') {
        event.preventDefault();
        setIsGotoChordActive(false);
        setMode('commands');
        setIsOpen(true);
        return;
      }

      if (event.key === 'Escape' && isOpen) {
        event.preventDefault();
        closePalette();
        return;
      }

      if (
        event.key === '?' &&
        !isOpen &&
        !isEditable &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey
      ) {
        event.preventDefault();
        setIsGotoChordActive(false);
        if (gotoTimerRef.current !== null) {
          window.clearTimeout(gotoTimerRef.current);
          gotoTimerRef.current = null;
        }
        setQuery('');
        setMode('shortcuts');
        setIsOpen(true);
        return;
      }

      if (
        !isOpen &&
        !isEditable &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.shiftKey
      ) {
        if (isGotoChordActive) {
          const action = GOTO_SHORTCUTS[key];
          setIsGotoChordActive(false);

          if (gotoTimerRef.current !== null) {
            window.clearTimeout(gotoTimerRef.current);
            gotoTimerRef.current = null;
          }

          if (action?.focusSearch) {
            if (focusGlobalSearchInput()) {
              event.preventDefault();
            }
            return;
          }

          if (action?.href) {
            event.preventDefault();
            router.push(action.href);
            return;
          }
        } else if (key === 'g') {
          event.preventDefault();
          setIsGotoChordActive(true);
          if (gotoTimerRef.current !== null) {
            window.clearTimeout(gotoTimerRef.current);
          }
          gotoTimerRef.current = window.setTimeout(() => {
            setIsGotoChordActive(false);
            gotoTimerRef.current = null;
          }, GOTO_CHORD_TIMEOUT_MS);
          return;
        }
      }

      if (
        event.key === '/' &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.shiftKey &&
        !isOpen &&
        !isEditable
      ) {
        if (focusGlobalSearchInput()) {
          event.preventDefault();
        }
      }
    };

    window.addEventListener('keydown', handleWindowKeyDown);
    return () => window.removeEventListener('keydown', handleWindowKeyDown);
  }, [isGotoChordActive, isOpen, router]);

  const execute = () => {
    if (mode !== 'commands') return;

    const command =
      matchingCommands[selectedIndex] ?? (matchingCommands.length > 0 ? matchingCommands[0] : null);

    if (command) {
      router.push(command.href);
    } else if (normalizedQuery) {
      const route = resolveSearchRoute(query);
      if (!route) {
        return;
      }
      router.push(route);
    } else {
      return;
    }

    closePalette();
  };

  const handleInputKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (matchingCommands.length === 0) return;
      setSelectedIndex((prev) => Math.min(prev + 1, matchingCommands.length - 1));
      return;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (matchingCommands.length === 0) return;
      setSelectedIndex((prev) => Math.max(prev - 1, 0));
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      execute();
      return;
    }

    if (event.key === 'Escape') {
      event.preventDefault();
      closePalette();
    }
  };

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-start justify-center bg-slate-950/70 px-4 pt-[12vh]"
      onClick={closePalette}
      role="presentation"
    >
      <div
        className="w-full max-w-2xl rounded-xl border border-slate-700 bg-slate-900 shadow-2xl"
        onClick={(event) => event.stopPropagation()}
      >
        {mode === 'commands' ? (
          <>
            <div className="border-b border-slate-700 p-3">
              <input
                ref={inputRef}
                type="text"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={handleInputKeyDown}
                placeholder="Type a command, or search block / tx / address"
                className="focus:border-terminal-green focus:ring-terminal-green w-full rounded-md border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100 outline-none focus:ring-1"
                aria-label="Command palette input"
              />
            </div>

            <div className="max-h-[360px] overflow-auto p-2">
              {matchingCommands.length > 0 ? (
                <ul className="space-y-1">
                  {matchingCommands.map((command, index) => (
                    <li key={command.id}>
                      <button
                        type="button"
                        className={`w-full rounded-md px-3 py-2 text-left font-mono text-sm transition-colors ${
                          index === selectedIndex
                            ? 'text-terminal-green bg-slate-800'
                            : 'text-slate-300 hover:bg-slate-800/60'
                        }`}
                        onClick={() => {
                          setSelectedIndex(index);
                          router.push(command.href);
                          closePalette();
                        }}
                      >
                        {command.label}
                      </button>
                    </li>
                  ))}
                </ul>
              ) : (
                <div className="rounded-md px-3 py-2 font-mono text-sm text-slate-400">
                  No command matched. Press Enter to run search.
                </div>
              )}
            </div>
          </>
        ) : (
          <>
            <div className="border-b border-slate-700 px-4 py-3">
              <h2 className="font-mono text-sm uppercase tracking-wide text-slate-200">
                Keyboard Shortcuts
              </h2>
            </div>

            <div className="max-h-[360px] overflow-auto p-2">
              <ul className="space-y-1">
                {SHORTCUT_HINTS.map((shortcut) => (
                  <li
                    key={shortcut.keys}
                    className="flex items-center justify-between gap-4 rounded-md px-3 py-2"
                  >
                    <span className="text-slate-300">{shortcut.description}</span>
                    <span className="rounded border border-slate-700 px-2 py-0.5 font-mono text-xs text-slate-300">
                      {shortcut.keys}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
