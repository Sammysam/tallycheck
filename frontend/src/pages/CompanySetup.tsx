import { useNavigate } from 'react-router-dom';
import CompanyForm from '../components/CompanyForm';

/**
 * D25: shown to the administrator after the first sign-in, until the company is entered.
 * The app ships with no company's details; this is where the customer adds their own.
 */
export default function CompanySetup() {
  const navigate = useNavigate();
  return (
    <div className="h-screen overflow-auto bg-[#e9ecef] py-8 px-4">
      <div className="max-w-3xl mx-auto bg-white border border-[#cfcfcf] rounded-lg shadow-[0_16px_40px_rgba(0,0,0,0.12)] overflow-hidden">
        <div className="auth-panel px-6 py-5">
          <p className="text-[11px] font-medium uppercase tracking-[0.18em] text-orange-300">Welcome</p>
          <h1 className="text-lg font-semibold text-white mt-1">Set up your company</h1>
          <p className="text-sm text-slate-300 mt-1">
            These details go on your invoices and statements, and set the currency and financial year. You can change them
            later under Administration → Settings.
          </p>
        </div>
        <div className="px-6 py-5">
          <CompanyForm submitLabel="Save and continue" onSaved={() => navigate('/', { replace: true })} />
        </div>
      </div>
    </div>
  );
}
