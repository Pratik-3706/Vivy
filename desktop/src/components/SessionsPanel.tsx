import React, { useEffect, useRef, useState } from 'react';
import { Pencil, Plus, Trash2 } from 'lucide-react';
import { SessionMeta } from '../lib/types';

interface SessionsPanelProps {
  sessions: SessionMeta[];
  activeId: string | null;
  disabled: boolean;
  onNew: () => void;
  onOpen: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onDelete: (id: string) => void;
}

const DAY = 24 * 60 * 60 * 1000;

function bucket(ts: number): string {
  const startOfToday = new Date().setHours(0, 0, 0, 0);
  if (ts >= startOfToday) return 'Today';
  if (ts >= startOfToday - DAY) return 'Yesterday';
  if (ts >= startOfToday - 7 * DAY) return 'Previous 7 days';
  return 'Older';
}

export const SessionsPanel: React.FC<SessionsPanelProps> = ({
  sessions,
  activeId,
  disabled,
  onNew,
  onOpen,
  onRename,
  onDelete,
}) => {
  const groups: [string, SessionMeta[]][] = [];
  for (const s of sessions) {
    const label = bucket(s.updated_at);
    const last = groups[groups.length - 1];
    if (last && last[0] === label) last[1].push(s);
    else groups.push([label, [s]]);
  }

  return (
    <aside className="sessions" aria-label="Chats">
      <button className="btn btn-ghost new-chat" onClick={onNew} disabled={disabled}>
        <Plus size={14} />
        New chat
      </button>
      <div className="sessions-list">
        {sessions.length === 0 && <p className="sessions-empty">Your chats will show up here.</p>}
        {groups.map(([label, items]) => (
          <div key={label} className="sessions-group">
            <div className="sessions-label">{label}</div>
            {items.map((s) => (
              <SessionRow
                key={s.id}
                session={s}
                active={s.id === activeId}
                disabled={disabled}
                onOpen={onOpen}
                onRename={onRename}
                onDelete={onDelete}
              />
            ))}
          </div>
        ))}
      </div>
    </aside>
  );
};

const SessionRow: React.FC<{
  session: SessionMeta;
  active: boolean;
  disabled: boolean;
  onOpen: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onDelete: (id: string) => void;
}> = ({ session, active, disabled, onOpen, onRename, onDelete }) => {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(session.title);
  const [confirming, setConfirming] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (editing) inputRef.current?.select();
  }, [editing]);

  // Ask once before deleting; the confirmation lapses after a few seconds.
  useEffect(() => {
    if (!confirming) return;
    const id = window.setTimeout(() => setConfirming(false), 3000);
    return () => window.clearTimeout(id);
  }, [confirming]);

  const commit = () => {
    setEditing(false);
    if (draft.trim() && draft.trim() !== session.title) onRename(session.id, draft.trim());
    else setDraft(session.title);
  };

  if (editing) {
    return (
      <div className="session-row editing">
        <input
          ref={inputRef}
          className="field"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit();
            if (e.key === 'Escape') {
              setDraft(session.title);
              setEditing(false);
            }
          }}
        />
      </div>
    );
  }

  return (
    <div className={`session-row ${active ? 'active' : ''} ${disabled && !active ? 'locked' : ''}`}>
      <button
        className="session-open"
        onClick={() => !active && onOpen(session.id)}
        onDoubleClick={() => {
          setDraft(session.title);
          setEditing(true);
        }}
        disabled={disabled && !active}
        title={disabled && !active ? 'Stop the current task to switch chats' : session.title}
      >
        {session.title}
      </button>
      <span className="session-actions">
        <button
          className="icon-btn"
          title="Rename"
          aria-label="Rename chat"
          onClick={() => {
            setDraft(session.title);
            setEditing(true);
          }}
        >
          <Pencil size={12} />
        </button>
        <button
          className={`icon-btn ${confirming ? 'danger' : ''}`}
          title={confirming ? 'Click again to delete' : 'Delete'}
          aria-label="Delete chat"
          disabled={disabled && active}
          onClick={() => (confirming ? onDelete(session.id) : setConfirming(true))}
        >
          <Trash2 size={12} />
        </button>
      </span>
    </div>
  );
};
