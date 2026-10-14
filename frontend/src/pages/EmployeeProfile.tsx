import { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import ActionMenu from '../components/ActionMenu';
import ProtectedImage from '../components/ProtectedImage';
import EmployeeFields from '../components/EmployeeFields';
import SalarySection from '../components/SalarySection';
import { useAuth } from '../contexts/AuthContext';
import {
  CATEGORY_LABEL,
  EMPLOYMENT_TYPE_LABEL,
  STATUS_LABEL,
  emptyEmployeeForm,
  expiryState,
  formatBytes,
  formatDate,
  initials,
  toEmployeeBody,
  toEmployeeForm,
  type DocumentCategory,
  type Employee,
  type EmployeeDocument,
  type EmployeeForm,
} from '../lib/employees';

interface Profile extends Employee {
  documents: EmployeeDocument[];
}

const emptyDocForm = { title: '', category: 'CERTIFICATE' as DocumentCategory, expiryDate: '', notes: '' };
type DocForm = typeof emptyDocForm;

/** Pulls a file through the API (so the auth header goes with it) and opens or saves it. */
async function fetchFile(path: string, filename: string, mode: 'open' | 'save') {
  const { data } = await api.get(path, { responseType: 'blob' });
  const url = URL.createObjectURL(data);
  if (mode === 'open') {
    window.open(url, '_blank', 'noopener');
  } else {
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
  }
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

export default function EmployeeProfile() {
  const { id = '' } = useParams();
  const navigate = useNavigate();
  const { hasPermission } = useAuth();

  const [profile, setProfile] = useState<Profile | null>(null);
  const [loadError, setLoadError] = useState('');
  const [photoVersion, setPhotoVersion] = useState(0);

  // Edit details
  const [editing, setEditing] = useState(false);
  const [form, setForm] = useState<EmployeeForm>(emptyEmployeeForm);
  const [editError, setEditError] = useState('');
  const [saving, setSaving] = useState(false);

  // Photo
  const photoInput = useRef<HTMLInputElement>(null);
  const [photoBusy, setPhotoBusy] = useState(false);
  const [photoError, setPhotoError] = useState('');

  // Documents
  const [docMode, setDocMode] = useState<'closed' | 'new' | 'replace' | 'edit'>('closed');
  const [docTarget, setDocTarget] = useState<EmployeeDocument | null>(null);
  const [docForm, setDocForm] = useState<DocForm>(emptyDocForm);
  const [docFile, setDocFile] = useState<File | null>(null);
  const [docError, setDocError] = useState('');
  const [docBusy, setDocBusy] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const docFileInput = useRef<HTMLInputElement>(null);

  const load = useCallback(
    () =>
      api
        .get(`/employees/${id}`)
        .then((r) => { setProfile(r.data); setLoadError(''); })
        .catch((err) => setLoadError(errMsg(err, 'Employee not found'))),
    [id],
  );

  useEffect(() => { load(); }, [load]);

  // ─── Details ──────────────────────────────────────────────────────────────

  const startEdit = () => {
    if (!profile) return;
    setForm(toEmployeeForm(profile));
    setEditError('');
    setEditing(true);
  };

  const saveEdit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setEditError('');
    try {
      await api.patch(`/employees/${id}`, toEmployeeBody(form));
      setEditing(false);
      await load();
    } catch (err) {
      setEditError(errMsg(err, 'Failed to save changes'));
    } finally {
      setSaving(false);
    }
  };

  const removeEmployee = async () => {
    if (!profile) return;
    const docs = profile.documents.length;
    const warning = docs
      ? `Delete ${profile.name} and all ${docs} stored document${docs === 1 ? '' : 's'}? This cannot be undone.`
      : `Delete ${profile.name}? This cannot be undone.`;
    if (!window.confirm(warning)) return;
    try {
      await api.delete(`/employees/${id}`);
      navigate('/employees');
    } catch (err) {
      alert(errMsg(err, 'Failed to delete employee'));
    }
  };

  // ─── Photo ────────────────────────────────────────────────────────────────

  const uploadPhoto = async (file: File | undefined) => {
    if (!file) return;
    setPhotoBusy(true);
    setPhotoError('');
    const body = new FormData();
    body.append('file', file);
    try {
      await api.post(`/employees/${id}/photo`, body);
      setPhotoVersion((v) => v + 1);
      await load();
    } catch (err) {
      setPhotoError(errMsg(err, 'Failed to upload photo'));
    } finally {
      setPhotoBusy(false);
      if (photoInput.current) photoInput.current.value = '';
    }
  };

  const removePhoto = async () => {
    if (!window.confirm('Remove the profile photo?')) return;
    setPhotoBusy(true);
    try {
      await api.delete(`/employees/${id}/photo`);
      setPhotoVersion((v) => v + 1);
      await load();
    } catch (err) {
      setPhotoError(errMsg(err, 'Failed to remove photo'));
    } finally {
      setPhotoBusy(false);
    }
  };

  // ─── Documents ────────────────────────────────────────────────────────────

  const openDocForm = (mode: 'new' | 'replace' | 'edit', target: EmployeeDocument | null = null) => {
    setDocMode(mode);
    setDocTarget(target);
    setDocFile(null);
    setDocError('');
    setDocForm(
      target
        ? { title: target.title, category: target.category, expiryDate: target.expiryDate ?? '', notes: target.notes ?? '' }
        : emptyDocForm,
    );
    if (docFileInput.current) docFileInput.current.value = '';
  };

  const closeDocForm = () => { setDocMode('closed'); setDocTarget(null); };

  const submitDoc = async (e: React.FormEvent) => {
    e.preventDefault();
    setDocError('');
    setDocBusy(true);
    try {
      if (docMode === 'edit' && docTarget) {
        await api.patch(`/employees/${id}/documents/${docTarget.id}`, {
          title: docForm.title.trim(),
          category: docForm.category,
          expiryDate: docForm.expiryDate || null,
          notes: docForm.notes.trim() || null,
        });
      } else {
        if (!docFile) throw new Error('Choose a file to upload');
        const body = new FormData();
        body.append('file', docFile);
        body.append('title', docForm.title.trim());
        body.append('category', docForm.category);
        if (docForm.expiryDate) body.append('expiryDate', docForm.expiryDate);
        if (docForm.notes.trim()) body.append('notes', docForm.notes.trim());
        const path =
          docMode === 'replace' && docTarget
            ? `/employees/${id}/documents/${docTarget.id}/replace`
            : `/employees/${id}/documents`;
        await api.post(path, body);
      }
      closeDocForm();
      await load();
    } catch (err) {
      setDocError(errMsg(err, 'Failed to save document'));
    } finally {
      setDocBusy(false);
    }
  };

  const removeDoc = async (doc: EmployeeDocument) => {
    if (!window.confirm(`Delete "${doc.title}" (${doc.originalName})? The file will be removed from storage.`)) return;
    try {
      await api.delete(`/employees/${id}/documents/${doc.id}`);
      await load();
    } catch (err) {
      alert(errMsg(err, 'Failed to delete document'));
    }
  };

  const viewDoc = (doc: EmployeeDocument) =>
    fetchFile(`/employees/${id}/documents/${doc.id}/download?inline=1`, doc.originalName, 'open').catch((err) =>
      alert(errMsg(err, 'Could not open the file')),
    );

  const downloadDoc = (doc: EmployeeDocument) =>
    fetchFile(`/employees/${id}/documents/${doc.id}/download`, doc.originalName, 'save').catch((err) =>
      alert(errMsg(err, 'Could not download the file')),
    );

  // ─── Render ───────────────────────────────────────────────────────────────

  if (loadError) {
    return (
      <div>
        <PageHeader title="Employee" />
        <div className="card text-sm text-red-600">{loadError}</div>
        <button onClick={() => navigate('/employees')} className="btn-link mt-3">← Back to employees</button>
      </div>
    );
  }

  if (!profile) {
    return (
      <div className="flex items-center justify-center py-20">
        <div className="w-6 h-6 border-2 border-blue-600 border-t-transparent rounded-full animate-spin" />
      </div>
    );
  }

  const visibleDocs = showHistory ? profile.documents : profile.documents.filter((d) => d.isCurrent);
  const olderCount = profile.documents.filter((d) => !d.isCurrent).length;
  const canPreview = (doc: EmployeeDocument) => doc.mimeType === 'application/pdf' || doc.mimeType.startsWith('image/');

  return (
    <div>
      <div className="mb-3">
        <button onClick={() => navigate('/employees')} className="btn-link text-xs">← All employees</button>
      </div>

      <PageHeader
        title={profile.name}
        action={
          <>
            {!editing && <button onClick={startEdit} className="btn btn-secondary">Edit details</button>}
            <button onClick={removeEmployee} className="btn btn-danger">Delete</button>
          </>
        }
      />

      {/* ── Profile card ── */}
      <div className="card mb-5">
        <div className="flex flex-col sm:flex-row gap-6">
          {/* Photo */}
          <div className="flex flex-col items-center gap-2 shrink-0">
            <ProtectedImage
              src={profile.photoKey ? `/employees/${id}/photo` : null}
              alt={profile.name}
              fallback={initials(profile.name)}
              version={photoVersion}
              className="w-28 h-28 rounded-full text-3xl border border-gray-200"
            />
            <input
              ref={photoInput}
              type="file"
              accept="image/jpeg,image/png,image/webp"
              className="hidden"
              onChange={(e) => uploadPhoto(e.target.files?.[0])}
            />
            <div className="flex gap-3">
              <button
                type="button"
                className="btn-link text-xs"
                disabled={photoBusy}
                onClick={() => photoInput.current?.click()}
              >
                {photoBusy ? 'Uploading…' : profile.photoKey ? 'Change photo' : 'Add photo'}
              </button>
              {profile.photoKey && (
                <button type="button" className="btn-link text-xs" style={{ color: '#dc2626' }} disabled={photoBusy} onClick={removePhoto}>
                  Remove
                </button>
              )}
            </div>
            {photoError && <p className="text-xs text-red-600 text-center max-w-[160px]">{photoError}</p>}
          </div>

          {/* Details or edit form */}
          <div className="flex-1 min-w-0">
            {editing ? (
              <form onSubmit={saveEdit} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
                <EmployeeFields form={form} onChange={setForm} />
                {editError && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{editError}</p>}
                <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
                  <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : 'Save changes'}</button>
                  <button type="button" onClick={() => setEditing(false)} className="btn btn-secondary">Cancel</button>
                </div>
              </form>
            ) : (
              <>
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mb-3">
                  <span className="font-mono text-sm text-gray-500">{profile.employeeId}</span>
                  <StatusBadge status={profile.status} />
                  <span className="text-xs text-gray-400">{STATUS_LABEL[profile.status]}</span>
                </div>
                <dl className="grid grid-cols-1 sm:grid-cols-2 gap-x-8 gap-y-3 text-sm">
                  <Field label="Job title" value={profile.jobTitle} />
                  <Field label="Department" value={profile.department} />
                  <Field label="Email" value={profile.email} />
                  <Field label="Phone" value={profile.phone} />
                  <Field label="Join date" value={profile.joinDate} />
                  <Field label="Employment type" value={EMPLOYMENT_TYPE_LABEL[profile.employmentType] ?? profile.employmentType} />
                  <Field label="EPF member no." value={profile.epfNumber} />
                  <Field label="National ID" value={profile.nationalId} />
                  {profile.resignedDate && <Field label="Resigned" value={profile.resignedDate} />}
                  <Field label="Documents on file" value={String(profile.documents.filter((d) => d.isCurrent).length)} />
                  {profile.notes && (
                    <div style={{ gridColumn: '1 / -1' }}>
                      <dt className="text-xs font-medium text-gray-500">Notes</dt>
                      <dd className="text-gray-800 whitespace-pre-wrap">{profile.notes}</dd>
                    </div>
                  )}
                </dl>
              </>
            )}
          </div>
        </div>
      </div>

      {/* ── Salary (payroll permission only) ── */}
      {hasPermission('payroll') && (
        <SalarySection employeeId={id} isContract={profile.employmentType === 'CONTRACT'} />
      )}

      {/* ── Documents ── */}
      <div className="flex items-center justify-between mb-3">
        <h2 className="section-title mb-0">Documents</h2>
        <div className="flex items-center gap-4">
          {olderCount > 0 && (
            <label className="flex items-center gap-1.5 text-xs text-gray-500">
              <input type="checkbox" checked={showHistory} onChange={(e) => setShowHistory(e.target.checked)} />
              Show previous versions ({olderCount})
            </label>
          )}
          <button onClick={() => openDocForm('new')} className="btn btn-primary">+ Upload document</button>
        </div>
      </div>

      {docMode !== 'closed' && (
        <div className="card mb-4">
          <p className="text-sm font-medium text-gray-800 mb-3">
            {docMode === 'new' && 'Upload a new document'}
            {docMode === 'replace' && docTarget && <>Upload a new version of <span className="font-semibold">{docTarget.title}</span> — the current file is kept as history</>}
            {docMode === 'edit' && docTarget && <>Edit details of <span className="font-semibold">{docTarget.title}</span></>}
          </p>
          <form onSubmit={submitDoc} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            {docMode !== 'edit' && (
              <label className="block" style={{ gridColumn: '1 / -1' }}>
                <span className="text-xs font-medium text-gray-500">File * (PDF, JPG, PNG, WebP or Word · max 10 MB)</span>
                <input
                  ref={docFileInput}
                  type="file"
                  required
                  accept=".pdf,.jpg,.jpeg,.png,.webp,.doc,.docx,application/pdf,image/*"
                  onChange={(e) => setDocFile(e.target.files?.[0] ?? null)}
                />
              </label>
            )}
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Title *</span>
              <input required placeholder="e.g. Office Safety Certificate" value={docForm.title} onChange={(e) => setDocForm({ ...docForm, title: e.target.value })} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Category</span>
              <select value={docForm.category} onChange={(e) => setDocForm({ ...docForm, category: e.target.value as DocumentCategory })}>
                {(Object.keys(CATEGORY_LABEL) as DocumentCategory[]).map((c) => (
                  <option key={c} value={c}>{CATEGORY_LABEL[c]}</option>
                ))}
              </select>
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Expiry date (optional)</span>
              <input type="date" value={docForm.expiryDate} onChange={(e) => setDocForm({ ...docForm, expiryDate: e.target.value })} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Notes (optional)</span>
              <input placeholder="Issuer, reference number…" value={docForm.notes} onChange={(e) => setDocForm({ ...docForm, notes: e.target.value })} />
            </label>
            {docError && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{docError}</p>}
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary" disabled={docBusy}>
                {docBusy ? 'Saving…' : docMode === 'edit' ? 'Save details' : 'Upload'}
              </button>
              <button type="button" onClick={closeDocForm} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table
        data={visibleDocs}
        emptyMessage="No documents yet. Upload certificates, ID copies, contracts and the like here."
        columns={[
          {
            header: 'Document',
            accessor: (d) => (
              <div className={d.isCurrent ? '' : 'opacity-60'}>
                <div className="font-medium text-gray-900">
                  {d.title}
                  {!d.isCurrent && <span className="ml-2 text-xs text-gray-400">(previous version)</span>}
                </div>
                <div className="text-xs text-gray-400">
                  {d.originalName} · {formatBytes(Number(d.size))}
                </div>
                {d.notes && <div className="text-xs text-gray-500 mt-0.5">{d.notes}</div>}
              </div>
            ),
          },
          { header: 'Category', accessor: (d) => CATEGORY_LABEL[d.category] ?? d.category },
          {
            header: 'Expiry',
            accessor: (d) => {
              const s = expiryState(d.expiryDate);
              return s ? <span className={`px-2 py-0.5 rounded text-xs font-medium ${s.cls}`}>{s.label}</span> : <span className="text-gray-400">—</span>;
            },
          },
          {
            header: 'Uploaded',
            accessor: (d) => (
              <div className="text-xs">
                <div>{formatDate(d.createdAt)}</div>
                <div className="text-gray-400">{d.uploadedByName || '—'}</div>
              </div>
            ),
          },
          {
            header: '',
            accessor: (d) => (
              <ActionMenu
                items={[
                  ...(canPreview(d) ? [{ label: 'View', onSelect: () => viewDoc(d) }] : []),
                  { label: 'Download', onSelect: () => downloadDoc(d) },
                  ...(d.isCurrent
                    ? [
                        { label: 'Upload new version', onSelect: () => openDocForm('replace', d) },
                        { label: 'Edit details', onSelect: () => openDocForm('edit', d) },
                      ]
                    : []),
                  { label: 'Delete', danger: true, onSelect: () => removeDoc(d) },
                ]}
              />
            ),
          },
        ]}
      />
    </div>
  );
}

function Field({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <div>
      <dt className="text-xs font-medium text-gray-500">{label}</dt>
      <dd className="text-gray-800">{value || '—'}</dd>
    </div>
  );
}
