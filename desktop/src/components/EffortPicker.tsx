import React, { useEffect, useRef, useState } from 'react';
import { HelpCircle } from 'lucide-react';
import { providerOf } from '../lib/providers';

const LABELS: Record<string, string> = {
  none: 'None',
  minimal: 'Minimal',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'Extra high',
  max: 'Max',
};
/** Every provider that takes an effort defaults to medium. */
const RECOMMENDED_ID = 'medium';

export const effortLabel = (id: string) => LABELS[id] ?? id;

/** Position of a saved effort among a provider's levels; unset or unknown = medium. */
export function effortIndex(levels: string[], value: string): number {
  const i = levels.indexOf(value);
  return i < 0 ? Math.max(0, levels.indexOf(RECOMMENDED_ID)) : i;
}

/**
 * Quick thinking-effort switch shown next to the model under the message box.
 * Hidden for providers that don't take an effort setting.
 */
export const EffortPicker: React.FC<{
  value: string;
  provider: string;
  disabled: boolean;
  onChange: (id: string) => void;
}> = ({ value, provider, disabled, onChange }) => {
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

  const levels = providerOf(provider).efforts;
  if (levels.length === 0) return null;

  const index = effortIndex(levels, value);
  const level = effortLabel(levels[index]);
  const recommended = levels.indexOf(RECOMMENDED_ID);
  const last = levels.length - 1;
  // Fraction along the track; the CSS turns it into a position the thumb can reach.
  const at = (i: number) => ({ '--at': i / last }) as React.CSSProperties;

  return (
    <div className="effort-picker" ref={ref}>
      <button
        className={`model-button effort-button ${open ? 'open' : ''}`}
        onClick={() => setOpen((o) => !o)}
        disabled={disabled}
        title={disabled ? 'Change the effort when the current task is done' : 'Thinking effort'}
        aria-haspopup="dialog"
        aria-expanded={open}
      >
        {level}
      </button>
      {open && (
        <div className="effort-pop" role="dialog" aria-label="Thinking effort">
          <div className="effort-head">
            <span className="effort-title">Effort</span>
            <span className="effort-value">{level}</span>
            <span
              className="effort-help"
              title="How long the model thinks before each step. Higher is better on hard tasks, but slower and costs more. Models that don't reason ignore it."
            >
              <HelpCircle size={15} />
            </span>
          </div>
          <div className="effort-ends">
            <span>Faster</span>
            <span>Smarter</span>
          </div>
          <div className="effort-slider" style={at(index)}>
            <div className="effort-track" />
            {levels.map((l, i) => (
              <span
                key={l}
                className={`effort-stop ${i === recommended ? 'recommended' : ''}`}
                style={at(i)}
              />
            ))}
            <input
              type="range"
              min={0}
              max={last}
              step={1}
              value={index}
              onChange={(e) => onChange(levels[Number(e.target.value)])}
              aria-label="Thinking effort"
              aria-valuetext={level}
            />
          </div>
          <div className="effort-labels">
            {recommended >= 0 && <span style={at(recommended)}>Recommended</span>}
          </div>
        </div>
      )}
    </div>
  );
};
