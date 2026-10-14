import { useCallback, useEffect, useState } from 'react';
import { PairingView } from './types';
import { subscribeAll, tauriApi } from './tauri';

/**
 * Live pairing state: the paired phone and the current code. Refreshes when
 * the PC pairs or forgets a phone, and fetches a new code when one expires.
 */
export function usePairing() {
  const [pairing, setPairing] = useState<PairingView | null>(null);
  const [now, setNow] = useState(Date.now());

  const refresh = useCallback(() => {
    tauriApi
      .getPairing()
      .then(setPairing)
      .catch((err) => console.error('Pairing error:', err));
  }, []);

  useEffect(() => {
    refresh();
    return subscribeAll([tauriApi.onPairingChanged(setPairing), tauriApi.onPhoneConnected(refresh)]);
  }, [refresh]);

  // Tick for the countdown; an expired code is replaced on the next fetch.
  const expiresAt = pairing?.code_expires_at ?? null;
  useEffect(() => {
    if (!expiresAt) return;
    const id = window.setInterval(() => {
      const t = Date.now();
      setNow(t);
      if (t >= expiresAt) refresh();
    }, 1000);
    return () => window.clearInterval(id);
  }, [expiresAt, refresh]);

  const run = (action: () => Promise<PairingView>) =>
    action()
      .then(setPairing)
      .catch((err) => console.error('Pairing error:', err));

  const secondsLeft = expiresAt ? Math.max(0, Math.ceil((expiresAt - now) / 1000)) : 0;

  return {
    pairing,
    secondsLeft,
    newCode: () => run(tauriApi.startPairing),
    cancel: () => run(tauriApi.cancelPairing),
    forget: () => run(tauriApi.forgetPhone),
  };
}

export function formatCountdown(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}
