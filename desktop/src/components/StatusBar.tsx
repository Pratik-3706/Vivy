import React from 'react';
import { Settings, Smartphone } from 'lucide-react';
import { DeviceState } from '../lib/types';
import vivyLogo from '../assets/vivy.png';

interface StatusBarProps {
  device: DeviceState | null;
  localIp: string;
  hasKey: boolean;
  agentEnabled: boolean;
  isRunning: boolean;
  isPaused: boolean;
  phoneOpen: boolean;
  onTogglePhone: () => void;
  onToggleAgent: () => void;
  onOpenSettings: () => void;
}

export const StatusBar: React.FC<StatusBarProps> = ({
  device,
  localIp,
  hasKey,
  agentEnabled,
  isRunning,
  isPaused,
  phoneOpen,
  onTogglePhone,
  onToggleAgent,
  onOpenSettings,
}) => {
  const connected = Boolean(device?.connected);

  const details = connected && device
    ? [
        device.device_name,
        device.android_version && `Android ${device.android_version}`,
        device.battery_level >= 0 && `${device.battery_level}%${device.is_charging ? ' charging' : ''}`,
      ].filter(Boolean)
    : [];

  return (
    <header className="header">
      <div className="wordmark">
        <img src={vivyLogo} alt="" />
        Vivy
      </div>

      <div className="device-status">
        {connected ? (
          <>
            <span className={`dot ${device?.streaming ? 'ok' : 'warn'}`} />
            {details.map((d, i) => (
              <React.Fragment key={i}>
                {i > 0 && <span className="sep">·</span>}
                <span>{d}</span>
              </React.Fragment>
            ))}
            {!device?.streaming && (
              <>
                <span className="sep">·</span>
                <span>no screen yet</span>
              </>
            )}
            {device?.accessibility === false && (
              <>
                <span className="sep">·</span>
                <span className="warn-text" title="Vivy can't tap, type or read the screen until it's turned back on">
                  accessibility off
                </span>
              </>
            )}
          </>
        ) : (
          <>
            <span className="dot" />
            <span>No phone connected</span>
            <span className="sep">·</span>
            <span className="mono">{localIp}:8765</span>
          </>
        )}
      </div>

      <div className="header-right">
        {!hasKey && (
          <button className="key-warning" onClick={onOpenSettings}>
            Add an API key
          </button>
        )}

        <button
          role="switch"
          aria-checked={agentEnabled}
          className={`agent-switch ${agentEnabled ? '' : 'off'} ${isRunning && !isPaused ? 'busy' : ''}`}
          onClick={onToggleAgent}
          title={
            agentEnabled
              ? 'Turn off to stop Vivy and Matsumoto immediately and block new tasks. Ctrl+Shift+X stops the current task from anywhere.'
              : 'Agent is off. Nothing will touch the phone until you turn it back on.'
          }
        >
          <span>Agent</span>
          <span className="state">{!agentEnabled ? 'off' : isPaused ? 'paused' : isRunning ? 'working' : 'on'}</span>
          <span className="track" />
        </button>

        <button
          className={`icon-btn ${phoneOpen ? 'on' : ''}`}
          onClick={onTogglePhone}
          title={phoneOpen ? 'Hide phone screen' : 'Show phone screen'}
          aria-label="Toggle phone screen"
          aria-pressed={phoneOpen}
        >
          <Smartphone size={16} />
        </button>

        <button className="icon-btn" onClick={onOpenSettings} title="Settings" aria-label="Settings">
          <Settings size={16} />
        </button>
      </div>
    </header>
  );
};
