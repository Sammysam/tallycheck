import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import './index.css';
import { AuthProvider } from './contexts/AuthContext';
import ProtectedRoute from './components/ProtectedRoute';
import Layout from './components/Layout';
import Login from './pages/Login';
import TwoFactorSetup from './pages/TwoFactorSetup';
import FirstRunSetup from './pages/FirstRunSetup';
import ForgotPassword from './pages/ForgotPassword';
import RecoveryKey from './pages/RecoveryKey';
import CompanySetup from './pages/CompanySetup';
import TwoFactorVerify from './pages/TwoFactorVerify';
import Dashboard from './pages/Dashboard';
import Accounts from './pages/Accounts';
import Customers from './pages/Customers';
import Vendors from './pages/Vendors';
import Employees from './pages/Employees';
import EmployeeProfile from './pages/EmployeeProfile';
import Payroll from './pages/Payroll';
import PayrollRun from './pages/PayrollRun';
import UsPayroll from './pages/UsPayroll';
import UsPayrollRun from './pages/UsPayrollRun';
import UsPayrollForms from './pages/UsPayrollForms';
import Invoices from './pages/Invoices';
import Estimates from './pages/Estimates';
import RecurringInvoices from './pages/RecurringInvoices';
import Bills from './pages/Bills';
import Expenses from './pages/Expenses';
import Payments from './pages/Payments';
import CustomerStatement from './pages/CustomerStatement';
import AccountActivity from './pages/AccountActivity';
import TransactionDetail from './pages/TransactionDetail';
import CreditNotes from './pages/CreditNotes';
import Ledger from './pages/Ledger';
import ImportData from './pages/ImportData';
import MoveBooks from './pages/MoveBooks';
import Items from './pages/Items';
import BankTransactions from './pages/BankTransactions';
import BankReconciliation from './pages/BankReconciliation';
import TrialBalance from './pages/reports/TrialBalance';
import ProfitLoss from './pages/reports/ProfitLoss';
import BalanceSheet from './pages/reports/BalanceSheet';
import CashFlow from './pages/reports/CashFlow';
import TaxReturn from './pages/reports/TaxReturn';
import Taxes from './pages/Taxes';
import ArAging from './pages/reports/ArAging';
import ApAging from './pages/reports/ApAging';
import Users from './pages/Users';
import AdminCentre from './pages/AdminCentre';
import Roles from './pages/Roles';
import AuditLog from './pages/AuditLog';
import Settings from './pages/Settings';

export default function App() {
  return (
    <AuthProvider>
      <BrowserRouter>
        <Routes>
          {/* Public auth routes */}
          <Route path="/login"      element={<Login />} />
          <Route path="/setup"      element={<FirstRunSetup />} />
          <Route path="/forgot-password" element={<ForgotPassword />} />
          <Route path="/2fa/setup"  element={<TwoFactorSetup />} />
          <Route path="/2fa/verify" element={<TwoFactorVerify />} />
          <Route path="/company-setup" element={<ProtectedRoute permission="settings"><CompanySetup /></ProtectedRoute>} />

          {/* Protected app routes */}
          <Route path="/*" element={
            <ProtectedRoute>
              <Layout>
                <Routes>
                  <Route path="/"                         element={<Dashboard />} />
                  <Route path="/recovery-key"             element={<RecoveryKey />} />
                  <Route path="/accounts"                 element={<Accounts />} />
                  <Route path="/customers"                element={<ProtectedRoute permission="customers"><Customers /></ProtectedRoute>} />
                  <Route path="/vendors"                  element={<ProtectedRoute permission="vendors"><Vendors /></ProtectedRoute>} />
                  <Route path="/employees" element={
                    <ProtectedRoute permission="employees"><Employees /></ProtectedRoute>
                  } />
                  <Route path="/employees/:id" element={
                    <ProtectedRoute permission="employees"><EmployeeProfile /></ProtectedRoute>
                  } />
                  <Route path="/payroll" element={
                    <ProtectedRoute permission="payroll"><Payroll /></ProtectedRoute>
                  } />
                  <Route path="/payroll/us" element={
                    <ProtectedRoute permission="payroll"><UsPayroll /></ProtectedRoute>
                  } />
                  <Route path="/payroll/us/forms" element={
                    <ProtectedRoute permission="payroll"><UsPayrollForms /></ProtectedRoute>
                  } />
                  <Route path="/payroll/us/:id" element={
                    <ProtectedRoute permission="payroll"><UsPayrollRun /></ProtectedRoute>
                  } />
                  <Route path="/payroll/:id" element={
                    <ProtectedRoute permission="payroll"><PayrollRun /></ProtectedRoute>
                  } />
                  <Route path="/invoices"                 element={<ProtectedRoute permission="invoices"><Invoices /></ProtectedRoute>} />
                  <Route path="/recurring-invoices"       element={<ProtectedRoute permission="invoices"><RecurringInvoices /></ProtectedRoute>} />
                  <Route path="/estimates"                element={<ProtectedRoute permission="invoices"><Estimates /></ProtectedRoute>} />
                  <Route path="/bills"                    element={<ProtectedRoute permission="bills"><Bills /></ProtectedRoute>} />
                  <Route path="/expenses"                 element={<ProtectedRoute permission="expenses"><Expenses /></ProtectedRoute>} />
                  <Route path="/payments"                 element={<ProtectedRoute permission="payments"><Payments /></ProtectedRoute>} />
                  <Route path="/credit-notes"             element={<ProtectedRoute permission="invoices"><CreditNotes kind="AR" key="AR" /></ProtectedRoute>} />
                  <Route path="/debit-notes"              element={<ProtectedRoute permission="bills"><CreditNotes kind="AP" key="AP" /></ProtectedRoute>} />
                  <Route path="/ledger"                   element={<ProtectedRoute permission="ledger"><Ledger /></ProtectedRoute>} />
                  <Route path="/import"                   element={<ImportData />} />
                  <Route path="/move-books"               element={<MoveBooks />} />
                  <Route path="/items"                    element={<Items />} />
                  <Route path="/bank-transactions" element={
                    <ProtectedRoute permission="banking"><BankTransactions /></ProtectedRoute>
                  } />
                  <Route path="/ledger/entries/:id" element={
                    <ProtectedRoute permission="ledger"><TransactionDetail /></ProtectedRoute>
                  } />
                  <Route path="/ledger/account/:id" element={
                    <ProtectedRoute permission="ledger"><AccountActivity /></ProtectedRoute>
                  } />
                  <Route path="/customers/:id/statement" element={
                    <ProtectedRoute permission="invoices"><CustomerStatement /></ProtectedRoute>
                  } />
                  <Route path="/reconcile"                element={<ProtectedRoute permission="ledger"><BankReconciliation /></ProtectedRoute>} />
                  <Route path="/reports/trial-balance"    element={<ProtectedRoute permission="ledger"><TrialBalance /></ProtectedRoute>} />
                  <Route path="/reports/profit-loss"      element={<ProtectedRoute permission="reports"><ProfitLoss /></ProtectedRoute>} />
                  <Route path="/reports/balance-sheet"    element={<ProtectedRoute permission="reports"><BalanceSheet /></ProtectedRoute>} />
                  <Route path="/reports/cash-flow"        element={<ProtectedRoute permission="reports"><CashFlow /></ProtectedRoute>} />
                  <Route path="/reports/tax"              element={<ProtectedRoute permission="reports"><TaxReturn /></ProtectedRoute>} />
                  <Route path="/taxes" element={
                    <ProtectedRoute permission="settings"><Taxes /></ProtectedRoute>
                  } />
                  <Route path="/reports/ar-aging"         element={<ProtectedRoute permission="reports"><ArAging /></ProtectedRoute>} />
                  <Route path="/reports/ap-aging"         element={<ProtectedRoute permission="reports"><ApAging /></ProtectedRoute>} />
                  <Route path="/admin" element={
                    <ProtectedRoute permission="users"><AdminCentre /></ProtectedRoute>
                  } />
                  <Route path="/users" element={
                    <ProtectedRoute permission="users"><Users /></ProtectedRoute>
                  } />
                  <Route path="/roles" element={
                    <ProtectedRoute permission="users"><Roles /></ProtectedRoute>
                  } />
                  <Route path="/audit" element={
                    <ProtectedRoute permission="audit"><AuditLog /></ProtectedRoute>
                  } />
                  <Route path="/settings" element={
                    <ProtectedRoute permission="settings"><Settings /></ProtectedRoute>
                  } />
                  {/* N2: each screen above needs its own right (else the dashboard). C8: an address that is not a screen (e.g. /invoices/new) goes to its list, not a blank page. */}
                  <Route path="/:section/*" element={<SectionFallback />} />
                </Routes>
              </Layout>
            </ProtectedRoute>
          } />
        </Routes>
      </BrowserRouter>
    </AuthProvider>
  );
}

/** C8: /invoices/new and other unknown addresses under a screen open that screen's list. */
function SectionFallback() {
  const first = window.location.pathname.split('/').filter(Boolean)[0] ?? '';
  const known = ['invoices', 'bills', 'estimates', 'customers', 'vendors', 'expenses', 'payments', 'items', 'employees', 'payroll', 'ledger', 'reports', 'accounts'];
  return <Navigate to={known.includes(first) ? `/${first}` : '/'} replace />;
}
