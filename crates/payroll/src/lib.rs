//! Payroll runs (reference `payroll-runs.service.ts` and `payroll-rates.ts`): monthly runs built
//! from each employee's salary in force, posting to the ledger, salary payment, statutory
//! remittances and voids. `us` is the United States tax engine (federal withholding, FICA, FUTA,
//! SUTA) and `us_setup` the US company settings, each employee's pay and Form W-4, and the
//! paycheck preview (D42, ported from the web edition); `us_runs` the US pay runs, `us_reports`
//! the Form 941, 940 and W-2 figures and `us_state` state income tax from the rules each company
//! enters (D43).

pub mod rates;
pub mod runs;
pub mod us;
pub mod us_reports;
pub mod us_runs;
pub mod us_setup;
pub mod us_state;

#[cfg(test)]
mod tests;
