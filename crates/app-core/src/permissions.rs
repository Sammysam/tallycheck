//! Copy of the reference `backend/src/common/constants/permissions.ts`, plus `ledger:post` (D80).

pub struct Permission {
    pub key: &'static str,
    pub label: &'static str,
}

pub const PERMISSIONS: &[Permission] = &[
    Permission { key: "dashboard", label: "Dashboard" },
    Permission { key: "accounts", label: "Accounts" },
    Permission { key: "customers", label: "Customers" },
    Permission { key: "vendors", label: "Vendors" },
    Permission { key: "employees", label: "Employees (profiles, photos & documents)" },
    Permission { key: "payroll", label: "Payroll (salaries, increments, bank details)" },
    Permission { key: "invoices", label: "Invoices" },
    Permission { key: "invoices:all", label: "Invoices — see everyone's (not just own)" },
    Permission { key: "bills", label: "Bills" },
    Permission { key: "expenses", label: "Expenses" },
    Permission { key: "payments", label: "Payments" },
    Permission { key: "banking", label: "Banking" },
    Permission { key: "ledger", label: "Ledger" },
    // D80: seeing the ledger and changing it are separate rights (the Viewer role is read-only).
    Permission { key: "ledger:post", label: "Ledger — post and reverse entries, reconcile" },
    Permission { key: "reports", label: "Reports" },
    Permission { key: "users", label: "Users & Roles" },
    Permission { key: "audit", label: "Activity Log" },
    Permission { key: "settings", label: "Settings" },
];

pub fn keys() -> impl Iterator<Item = &'static str> {
    PERMISSIONS.iter().map(|p| p.key)
}

pub fn is_known(key: &str) -> bool {
    keys().any(|k| k == key)
}

/// Name of the system role that bypasses permission checks.
pub const ADMIN_ROLE: &str = "Admin";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eighteen_unique_keys_in_reference_order() {
        let k: Vec<_> = keys().collect();
        assert_eq!(k.len(), 18);
        assert_eq!(k.first(), Some(&"dashboard"));
        assert_eq!(k.last(), Some(&"settings"));
        let mut sorted = k.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 18);
        assert!(is_known("invoices:all"));
        assert!(!is_known("admin"));
    }
}
