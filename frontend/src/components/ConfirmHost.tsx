import { useSyncExternalStore } from 'react';
import ConfirmDialog from './ConfirmDialog';
import { currentConfirm, subscribeConfirm } from '../lib/confirm';

/** N3: shows the question asked with `askConfirm()`. */
export default function ConfirmHost() {
  const ask = useSyncExternalStore(subscribeConfirm, currentConfirm);
  if (!ask) return null;
  return (
    <ConfirmDialog
      key={ask.id}
      title={ask.title}
      confirmLabel={ask.confirmLabel}
      danger={ask.danger}
      onConfirm={() => ask.resolve(true)}
      onCancel={() => ask.resolve(false)}
    >
      <p className="whitespace-pre-line">{ask.message}</p>
    </ConfirmDialog>
  );
}
