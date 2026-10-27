//! Payroll runs (reference `payroll-runs.service.ts` and `payroll-rates.ts`): monthly runs built
//! from each employee's salary in force, posting to the ledger, salary payment, statutory
//! remittances and voids. `us` is the United States tax engine (federal withholding, FICA, FUTA,
//! SUTA) and `us_setup` the US company settings, each employee's pay and Form W-4, and the
//! paycheck preview (D42, ported from the web edition).

pub mod rates;
pub mod runs;
pub mod us;
pub mod us_setup;

#[cfg(test)]
mod tests;
