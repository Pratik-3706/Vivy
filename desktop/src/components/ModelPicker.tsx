import React, { useEffect, useRef, useState } from 'react';
import { Check, ChevronDown } from 'lucide-react';
import { providerOf } from '../lib/providers';

/** Compact model switcher shown under the message box: the current provider's models. */
export const ModelPicker: React.FC<{
  value: string;
  provider: string;
  disabled: boolean;
  onChange: (id: string) => void;
}> = ({ value, provider, disabled, onChange }) => {
  const { name: providerName, models: MODELS } = providerOf(provider);
  const label = (id: string) => MODELS.find((m) => m.id === id)?.name ?? id.split('/').pop() ?? id;
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    window.addEventListener('mousedown', close);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', close);
      window.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const custom = value && !MODELS.some((m) => m.id === value);

  // A custom endpoint has no known model list: show the model, change it in Settings.
  if (provider === 'custom') {
    return (
      <div className="model-picker">
        <span className="model-button static" title={`Custom endpoint: ${value}. Change it in Settings.`}>
          {label(value)}
        </span>
      </div>
    );
  }

  return (
    <div className="model-picker" ref={ref}>
      <button
        className="model-button"
        onClick={() => setOpen((o) => !o)}
        disabled={disabled}
        title={disabled ? 'Change the model when the current task is done' : `${providerName}: ${value}`}
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        {label(value)}
        <ChevronDown size={12} />
      </button>
      {open && (
        <div className="model-menu" role="listbox">
          <div className="model-menu-head">{providerName}</div>
          {MODELS.map((m) => (
            <button
              key={m.id}
              role="option"
              aria-selected={m.id === value}
              className="model-option"
              onClick={() => {
                setOpen(false);
                if (m.id !== value) onChange(m.id);
              }}
            >
              <span className="model-name">{m.name}</span>
              <span className="model-note">{m.note}</span>
              {m.id === value && <Check size={13} />}
            </button>
          ))}
          {custom && (
            <div className="model-option current-custom">
              <span className="model-name">{value}</span>
              <span className="model-note">Custom, from Settings</span>
              <Check size={13} />
            </div>
          )}
        </div>
      )}
    </div>
  );
};
