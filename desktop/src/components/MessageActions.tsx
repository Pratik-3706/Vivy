import React, { createContext, useContext, useState } from 'react';
import { Check, Copy, Pencil, RotateCcw } from 'lucide-react';

/** A message the user sent: its text and the id of its "task" log. */
export type PromptRef = { text: string; taskId: string };

/** Callbacks for the hover actions, provided once by the feed. */
export const MessageActionsContext = createContext<{
  canRun: boolean;
  onEdit: (prompt: PromptRef) => void;
  onResend: (prompt: PromptRef) => void;
}>({ canRun: false, onEdit: () => {}, onResend: () => {} });

/**
 * Small toolbar that appears when hovering a message.
 * `copyText` is copied; `prompt` (if any) can be edited or sent again, which
 * replaces it and everything after it.
 */
export const MessageActions: React.FC<{ copyText: string; prompt?: PromptRef; resendLabel?: string }> = ({
  copyText,
  prompt,
  resendLabel = 'Resend',
}) => {
  const { canRun, onEdit, onResend } = useContext(MessageActionsContext);
  const [copied, setCopied] = useState(false);

  const copy = () => {
    navigator.clipboard.writeText(copyText).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    });
  };

  return (
    <div className="msg-actions">
      <button className="icon-btn" onClick={copy} title={copied ? 'Copied' : 'Copy'} aria-label="Copy">
        {copied ? <Check size={13} /> : <Copy size={13} />}
      </button>
      {prompt !== undefined && resendLabel === 'Resend' && (
        <button className="icon-btn" onClick={() => onEdit(prompt)} title="Edit" aria-label="Edit">
          <Pencil size={13} />
        </button>
      )}
      {prompt !== undefined && (
        <button
          className="icon-btn"
          onClick={() => onResend(prompt)}
          disabled={!canRun}
          title={canRun ? resendLabel : 'Wait for the current task to finish'}
          aria-label={resendLabel}
        >
          <RotateCcw size={13} />
        </button>
      )}
    </div>
  );
};
