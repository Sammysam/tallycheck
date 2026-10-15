//! Payroll runs (reference `payroll-runs.service.ts` and `payroll-rates.ts`): monthly runs built
//! from each employee's salary in force, posting to the ledger, salary payment, statutory
//! remittances and voids.

pub mod rates;
pub mod runs;

#[cfg(test)]
mod tests;
