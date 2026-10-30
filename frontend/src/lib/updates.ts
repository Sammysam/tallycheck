// D45 in-app updates: the desktop app checks Cenango's release page through the server
// (/api/system/update), and installs a newer version on request. Administrator, main PC only.
import { useEffect, useState } from 'react';
import api from '../api/client';

export interface UpdateInfo {
  version: string;
  notes: string | null;
  date: string | null;
}

export interface UpdateStatus {
  current: string;
  /** False when this build has no update feed (or the server runs without the desktop app). */
  supported: boolean;
  available: UpdateInfo | null;
  /** Why the release page couldn't be checked (usually no internet). */
  error: string | null;
}

export const checkForUpdate = async () => (await api.get<UpdateStatus>('/system/update')).data;

export const installUpdate = async () =>
  (await api.post<{ current: string; installing: UpdateInfo }>('/system/update/install')).data;

/** Checks once, quietly, when an Administrator opens the app; null until there is something to offer. */
export function useAvailableUpdate(enabled: boolean): [UpdateStatus | null, (s: UpdateStatus) => void] {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    checkForUpdate()
      .then((s) => { if (alive && s.available) setStatus(s); })
      .catch(() => { /* not on the main PC, or offline: say nothing */ });
    return () => { alive = false; };
  }, [enabled]);
  return [status, setStatus];
}
