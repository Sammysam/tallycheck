import {
  EMPLOYMENT_TYPE_LABEL,
  STATUS_LABEL,
  type EmployeeForm,
  type EmployeeStatus,
  type EmploymentType,
} from '../lib/employees';

/** Shared between the create form on the list page and the edit form on the profile. */
export default function EmployeeFields({
  form,
  onChange,
}: {
  form: EmployeeForm;
  onChange: (next: EmployeeForm) => void;
}) {
  const set =
    (key: keyof EmployeeForm) =>
    (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>) =>
      onChange({ ...form, [key]: e.target.value });

  return (
    <>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Employee ID *</span>
        <input required placeholder="e.g. CEN-042" value={form.employeeId} onChange={set('employeeId')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Full name *</span>
        <input required placeholder="Full name" value={form.name} onChange={set('name')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Job title</span>
        <input placeholder="Job title" value={form.jobTitle} onChange={set('jobTitle')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Department</span>
        <input placeholder="Department" value={form.department} onChange={set('department')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Email</span>
        <input type="email" placeholder="Email" value={form.email} onChange={set('email')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Phone</span>
        <input placeholder="Phone" value={form.phone} onChange={set('phone')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Join date</span>
        <input type="date" value={form.joinDate} onChange={set('joinDate')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Status</span>
        <select value={form.status} onChange={set('status')}>
          {(Object.keys(STATUS_LABEL) as EmployeeStatus[]).map((s) => (
            <option key={s} value={s}>{STATUS_LABEL[s]}</option>
          ))}
        </select>
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Employment type</span>
        <select value={form.employmentType} onChange={set('employmentType')}>
          {(Object.keys(EMPLOYMENT_TYPE_LABEL) as EmploymentType[]).map((t) => (
            <option key={t} value={t}>{EMPLOYMENT_TYPE_LABEL[t]}</option>
          ))}
        </select>
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">Resigned date</span>
        <input type="date" value={form.resignedDate} onChange={set('resignedDate')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">EPF member no.</span>
        <input placeholder="e.g. 96" value={form.epfNumber} onChange={set('epfNumber')} />
      </label>
      <label className="block">
        <span className="text-xs font-medium text-gray-500">National ID (NIC)</span>
        <input placeholder="NIC number" value={form.nationalId} onChange={set('nationalId')} />
      </label>
      <label className="block" style={{ gridColumn: '1 / -1' }}>
        <span className="text-xs font-medium text-gray-500">Notes</span>
        <textarea rows={2} placeholder="Anything worth remembering" value={form.notes} onChange={set('notes')} />
      </label>
    </>
  );
}
