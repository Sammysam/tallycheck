import { Link } from 'react-router-dom';

/** D35: a document number that opens its transaction (the posted journal entry) in full. */
export default function TxnLink({ entryId, children }: { entryId?: string | null; children: React.ReactNode }) {
  if (!entryId) return <>{children}</>;
  return (
    <Link className="report-link" to={`/ledger/entries/${entryId}`} title="Open the transaction" onClick={(e) => e.stopPropagation()}>
      {children}
    </Link>
  );
}
