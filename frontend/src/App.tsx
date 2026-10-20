import { BrowserRouter, Routes, Route } from 'react-router-dom';
import './index.css';
import { AuthProvider } from './contexts/AuthContext';
import ProtectedRoute from './components/ProtectedRoute';
import Layout from './components/Layout';
import Login from './pages/Login';
import TwoFactorSetup from './pages/TwoFactorSetup';
import FirstRunSetup from './pages/FirstRunSetup';
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
import Invoices from './pages/Invoices';
import Bills from './pages/Bills';
import Expenses from './pages/Expenses';
import Payments from './pages/Payments';
import CustomerStatement from './pages/CustomerStatement';
import AccountActivity from './pages/AccountActivity';
import CreditNotes from './pages/CreditNotes';
import Ledger from './pages/Ledger';
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
          <Route path="/2fa/setup"  element={<TwoFactorSetup />} />
          <Route path="/2fa/verify" element={<TwoFactorVerify />} />
          <Route path="/company-setup" element={<ProtectedRoute permission="settings"><CompanySetup /></ProtectedRoute>} />

          {/* Protected app routes */}
          <Route path="/*" element={
            <ProtectedRoute>
              <Layout>
                <Routes>
                  <Route path="/"                         element={<Dashboard />} />
                  <Route path="/accounts"                 element={<Accounts />} />
                  <Route path="/customers"                element={<Customers />} />
                  <Route path="/vendors"                  element={<Vendors />} />
                  <Route path="/employees" element={
                    <ProtectedRoute permission="employees"><Employees /></ProtectedRoute>
                  } />
                  <Route path="/employees/:id" element={
                    <ProtectedRoute permission="employees"><EmployeeProfile /></ProtectedRoute>
                  } />
                  <Route path="/payroll" element={
                    <ProtectedRoute permission="payroll"><Payroll /></ProtectedRoute>
                  } />
                  <Route path="/payroll/:id" element={
                    <ProtectedRoute permission="payroll"><PayrollRun /></ProtectedRoute>
                  } />
                  <Route path="/invoices"                 element={<Invoices />} />
                  <Route path="/bills"                    element={<Bills />} />
                  <Route path="/expenses"                 element={<Expenses />} />
                  <Route path="/payments"                 element={<Payments />} />
                  <Route path="/credit-notes"             element={<CreditNotes kind="AR" key="AR" />} />
                  <Route path="/debit-notes"              element={<CreditNotes kind="AP" key="AP" />} />
                  <Route path="/ledger"                   element={<Ledger />} />
                  <Route path="/ledger/account/:id" element={
                    <ProtectedRoute permission="ledger"><AccountActivity /></ProtectedRoute>
                  } />
                  <Route path="/customers/:id/statement" element={
                    <ProtectedRoute permission="invoices"><CustomerStatement /></ProtectedRoute>
                  } />
                  <Route path="/reconcile"                element={<BankReconciliation />} />
                  <Route path="/reports/trial-balance"    element={<TrialBalance />} />
                  <Route path="/reports/profit-loss"      element={<ProfitLoss />} />
                  <Route path="/reports/balance-sheet"    element={<BalanceSheet />} />
                  <Route path="/reports/cash-flow"        element={<CashFlow />} />
                  <Route path="/reports/tax"              element={<TaxReturn />} />
                  <Route path="/taxes" element={
                    <ProtectedRoute permission="settings"><Taxes /></ProtectedRoute>
                  } />
                  <Route path="/reports/ar-aging"         element={<ArAging />} />
                  <Route path="/reports/ap-aging"         element={<ApAging />} />
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
                </Routes>
              </Layout>
            </ProtectedRoute>
          } />
        </Routes>
      </BrowserRouter>
    </AuthProvider>
  );
}
