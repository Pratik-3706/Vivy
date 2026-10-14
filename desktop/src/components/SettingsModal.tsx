import React, { useEffect, useState } from 'react';
import { Eye, EyeOff, X } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { tauriApi } from '../lib/tauri';
import { KeyField, PROVIDERS, ProviderId, providerOf } from '../lib/providers';
import { effortIndex, effortLabel } from './EffortPicker';
import { usePairing } from '../lib/pairing';

interface SettingsModalProps {
  isOpen: boolean;
  localIp: string;
  onClose: () => void;
  onSaved: () => void;
}

const DEFAULT_MODEL = 'z-ai/glm-5.3-flash';

const NO_KEYS: Record<KeyField, string> = {
  aicredits_key: '',
  openai_key: '',
  anthropic_key: '',
  google_key: '',
  custom_key: '',
};

const SecretField: React.FC<{
  id: string;
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
}> = ({ id, value, onChange, placeholder }) => {
  const [visible, setVisible] = useState(false);
  return (
    <div className="with-action">
      <input
        id={id}
        className="field mono"
        type={visible ? 'text' : 'password'}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
      />
      <button
        type="button"
        className="icon-btn"
        onClick={() => setVisible((v) => !v)}
        aria-label={visible ? 'Hide' : 'Show'}
      >
        {visible ? <EyeOff size={14} /> : <Eye size={14} />}
      </button>
    </div>
  );
};

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, localIp, onClose, onSaved }) => {
  const [provider, setProvider] = useState<ProviderId>('aicredits');
  // Every provider keeps its own key, so switching back and forth loses nothing.
  const [keys, setKeys] = useState<Record<KeyField, string>>(NO_KEYS);
  const [tavilyKey, setTavilyKey] = useState('');
  const [modelName, setModelName] = useState(DEFAULT_MODEL);
  // "Other model": a typed model ID instead of one of the cards.
  const [otherModel, setOtherModel] = useState(false);
  const [visionModel, setVisionModel] = useState('');
  const [effort, setEffort] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isOpen) return;
    setError(null);
    tauriApi
      .getSettings()
      .then((s) => {
        const p = providerOf(s.provider);
        setProvider(p.id);
        setOtherModel(!p.models.some((m) => m.id === s.model_name));
        setKeys({
          aicredits_key: s.aicredits_key || '',
          openai_key: s.openai_key || '',
          anthropic_key: s.anthropic_key || '',
          google_key: s.google_key || '',
          custom_key: s.custom_key || '',
        });
        setTavilyKey(s.tavily_key || '');
        setModelName(s.model_name || DEFAULT_MODEL);
        setVisionModel(s.vision_model || '');
        setEffort(s.reasoning_effort || '');
        setBaseUrl(s.base_url || '');
      })
      .catch((e) => setError(String(e)));
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isOpen, onClose]);

  const current = providerOf(provider);

  const chooseProvider = (id: ProviderId) => {
    setProvider(id);
    const next = providerOf(id);
    // Switch to the new provider's recommended model unless the one typed belongs to it.
    if (next.models.length && !next.models.some((m) => m.id === modelName.trim())) {
      setModelName(next.models[0].id);
      setOtherModel(false);
    }
  };

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    if (provider === 'custom' && !baseUrl.trim()) {
      setError('Enter the endpoint URL for the custom provider.');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await tauriApi.saveSettings({
        provider,
        aicredits_key: keys.aicredits_key.trim(),
        openai_key: keys.openai_key.trim(),
        anthropic_key: keys.anthropic_key.trim(),
        google_key: keys.google_key.trim(),
        custom_key: keys.custom_key.trim(),
        tavily_key: tavilyKey.trim(),
        model_name: modelName.trim() || current.models[0]?.id || DEFAULT_MODEL,
        vision_model: visionModel.trim(),
        reasoning_effort: effort,
        base_url: baseUrl.trim(),
      });
      onSaved();
      onClose();
    } catch (err) {
      setError(`Couldn't save: ${err}`);
    } finally {
      setSaving(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="dialog" onSubmit={save} role="dialog" aria-labelledby="settings-title">
        <div className="dialog-head">
          <h2 id="settings-title">Settings</h2>
          <button type="button" className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>

        <div className="dialog-body">
          <div className="form-row">
            <label>Provider</label>
            <div className="segmented" role="radiogroup" aria-label="Provider">
              {PROVIDERS.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  role="radio"
                  aria-checked={p.id === provider}
                  className={p.id === provider ? 'on' : ''}
                  onClick={() => chooseProvider(p.id)}
                >
                  {p.name}
                </button>
              ))}
            </div>
          </div>

          {provider === 'custom' && (
            <div className="form-row">
              <label htmlFor="base-url">Endpoint URL</label>
              <input
                id="base-url"
                className="field mono"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://openrouter.ai/api/v1"
                spellCheck={false}
              />
              <span className="help">Any OpenAI-compatible endpoint (OpenRouter, a local server, etc.).</span>
            </div>
          )}

          <div className="form-row">
            <label htmlFor="api-key">{current.name} API key</label>
            <SecretField
              id="api-key"
              value={keys[current.keyField]}
              onChange={(v) => setKeys((k) => ({ ...k, [current.keyField]: v }))}
              placeholder={current.keyPlaceholder}
            />
            <span className="help">
              Stored in Windows Credential Manager.
              {current.keyUrl && (
                <>
                  {' '}
                  <button type="button" className="link" onClick={() => openUrl(current.keyUrl!)}>
                    Get a key
                  </button>
                </>
              )}
            </span>
          </div>

          <div className="form-row">
            <label>Model</label>
            {current.models.length > 0 && (
              <div className="model-cards" role="radiogroup" aria-label="Model">
                {current.models.map((m) => {
                  const on = !otherModel && m.id === modelName.trim();
                  return (
                    <button
                      key={m.id}
                      type="button"
                      role="radio"
                      aria-checked={on}
                      className={`model-card ${on ? 'on' : ''}`}
                      onClick={() => {
                        setModelName(m.id);
                        setOtherModel(false);
                      }}
                    >
                      <span className="mc-top">
                        <span className="mc-name">{m.name}</span>
                      </span>
                      <span className="mc-bottom">
                        <span className="mc-id">{m.id}</span>
                        <span className={`mc-tag ${m.tier ?? ''}`}>{m.note}</span>
                      </span>
                    </button>
                  );
                })}
                <button
                  type="button"
                  role="radio"
                  aria-checked={otherModel}
                  className={`model-card ${otherModel ? 'on' : ''}`}
                  onClick={() => {
                    if (!otherModel) setModelName('');
                    setOtherModel(true);
                  }}
                >
                  <span className="mc-top">
                    <span className="mc-name">Other model</span>
                  </span>
                  <span className="mc-id">Type any model ID</span>
                </button>
              </div>
            )}
            {(otherModel || current.models.length === 0) && (
              <input
                id="model"
                className="field mono"
                value={modelName}
                onChange={(e) => setModelName(e.target.value)}
                placeholder={current.models[0]?.id ?? 'model-id'}
                spellCheck={false}
                autoFocus={current.models.length > 0}
                aria-label="Model ID"
              />
            )}
            <span className="help">Needs to accept images and support tool calling.</span>
          </div>

          {current.efforts.length > 0 ? (
            <div className="form-row">
              <label>Thinking effort</label>
              <div className="segmented" role="radiogroup" aria-label="Thinking effort">
                {current.efforts.map((e, i) => (
                  <button
                    key={e}
                    type="button"
                    role="radio"
                    aria-checked={effortIndex(current.efforts, effort) === i}
                    className={effortIndex(current.efforts, effort) === i ? 'on' : ''}
                    onClick={() => setEffort(e)}
                    title={e === 'medium' ? 'Recommended' : undefined}
                  >
                    {effortLabel(e)}
                  </button>
                ))}
              </div>
              <span className="help">
                Higher thinks longer before acting: better on hard tasks, slower and costs more. Models that
                don&rsquo;t reason ignore it.
              </span>
            </div>
          ) : (
            current.effortNote && (
              <div className="form-row">
                <label>Thinking effort</label>
                <span className="help">{current.effortNote}</span>
              </div>
            )
          )}

          <div className="form-row">
            <label htmlFor="vision-model">Vision model (optional)</label>
            <input
              id="vision-model"
              className="field mono"
              value={visionModel}
              onChange={(e) => setVisionModel(e.target.value)}
              placeholder="Same as model"
              spellCheck={false}
            />
            <span className="help">
              Used only when the screen can&rsquo;t be read as text (games, images, maps). Must accept images.
            </span>
          </div>

          <div className="form-row">
            <label htmlFor="tavily">Tavily key (optional)</label>
            <SecretField id="tavily" value={tavilyKey} onChange={setTavilyKey} placeholder="tvly-..." />
            <span className="help">Enables web search during tasks.</span>
          </div>

          <PhonePairingRow localIp={localIp} />

          {error && <div className="banner">{error}</div>}

        </div>

        <div className="dialog-foot">
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="btn btn-primary" disabled={saving}>
            {saving ? 'Saving…' : 'Save'}
          </button>
        </div>
      </form>
    </div>
  );
};

/** Which phone is paired, with a way to unpair it. */
const PhonePairingRow: React.FC<{ localIp: string }> = ({ localIp }) => {
  const { pairing, forget } = usePairing();
  const [confirm, setConfirm] = useState(false);
  const paired = pairing?.paired;
  return (
    <div className="form-section phone-row">
      <span>
        {paired ? (
          <>
            Paired with <strong>{paired.device_name}</strong> at <span className="mono">{localIp}:8765</span>
          </>
        ) : (
          <>
            No phone paired yet. Pair one from the phone panel (<span className="mono">{localIp}:8765</span>).
          </>
        )}
      </span>
      {paired && (
        <button
          type="button"
          className={`link ${confirm ? 'danger' : ''}`}
          onClick={() => {
            if (!confirm) return setConfirm(true);
            setConfirm(false);
            forget();
          }}
          onBlur={() => setConfirm(false)}
        >
          {confirm ? 'Click again to forget' : 'Forget phone'}
        </button>
      )}
    </div>
  );
};
