//! D64: built-in 2026 state income tax withholding rules, copied from each state's own 2026
//! employer withholding publication (named in `source`). Step 1 of the US state payroll plan:
//! the states whose wage withholding is one flat rate (Arizona: a rate the employee elects).
//! A company's own rule for a state always wins over these ([`crate::us_state::load_rule`]).
//!
//! Each rule applies to wages paid from `from` (the pay date) until the next entry for the same
//! state. Before a state's first entry there is no built-in rule (the company enters one).
//! Amounts are cents a year, rates ppm. What the employee's state form calls each allowance:
//! - Illinois IL-W-4: line 1 basic allowances (`allowances`, $2,925), line 2 additional
//!   allowances for 65+ / blind (`second allowances`, $1,000).
//! - Indiana WH-4: line 5 personal exemptions (`allowances`, $1,000), lines 6–7 dependents and
//!   first-time dependents (`second allowances`, $1,500); an adopted child on line 8 ($3,000)
//!   counts as two second allowances. County tax is not included.
//! - Iowa IA W-4 (2024 or later): line 7 total allowance in dollars (`annual credit`); an IA W-4
//!   from 2023 or earlier: allowances × $40.
//! - Colorado DR 0004 line 2: an amount replacing the standard deduction (`deduction override`).
//! - Arizona A-4: the elected percentage (`elected rate`); 2.0% when none was given.
//!
//! Not included (their withholding is not one rate): Ohio and Mississippi (brackets, step 2),
//! Utah and Massachusetts (credits / surtax, step 3).

use crate::us_state::StateRule;

pub struct Pack {
    pub state: &'static str,
    /// First pay date the rule applies to.
    pub from: &'static str,
    pub rule: StateRule,
    pub source: &'static str,
}

const fn flat(rate: i64, single: i64, married: i64) -> StateRule {
    StateRule {
        method: String::new(),
        flat_rate: rate,
        brackets: Vec::new(),
        brackets_married: None,
        standard_deduction_single: single,
        standard_deduction_married: married,
        allowance_deduction: 0,
        allowance_credit: 0,
        excludes_401k: true,
        resident_credit: false,
        round_to_dollar: false,
        standard_deduction_hoh: None,
        second_allowance_deduction: 0,
        second_allowance_credit: 0,
    }
}

fn pack(state: &'static str, from: &'static str, mut rule: StateRule, source: &'static str) -> Pack {
    if rule.method.is_empty() {
        rule.method = "FLAT".into();
    }
    Pack { state, from, rule, source }
}

/// Every built-in rule, oldest first within a state.
pub fn all() -> Vec<Pack> {
    vec![
        pack(
            "AZ",
            "2026-01-01",
            StateRule { method: "EMPLOYEE_RATE".into(), ..flat(20_000, 0, 0) },
            "Arizona Form A-4 (2026), Employee's Arizona Withholding Election: 0.5–3.5% of gross taxable wages as elected; 2.0% without an A-4",
        ),
        pack(
            "CO",
            "2026-01-01",
            flat(44_000, 550_000, 1_100_000),
            "Colorado DR 1098 (10/21/25), 2026 Colorado Withholding Worksheet for Employers: 4.40%; $5,500 / $11,000 (married filing jointly) unless DR 0004 line 2 gives an amount",
        ),
        pack(
            "GA",
            "2026-01-01",
            StateRule { allowance_deduction: 500_000, ..flat(51_900, 1_500_000, 3_000_000) },
            "Georgia Employer's Withholding Tax Guide 2026 (rev. Sept 2026): 5.19% for pay dates before 11 May 2026; $15,000 / $30,000 (married, one spouse working); $5,000 per allowance",
        ),
        pack(
            "GA",
            "2026-05-11",
            StateRule { allowance_deduction: 500_000, ..flat(49_900, 1_500_000, 3_000_000) },
            "Georgia Employer's Withholding Tax Guide 2026 (rev. Sept 2026): 4.99% from 11 May 2026 (HB 463); $15,000 / $30,000 (married, one spouse working); $5,000 per allowance",
        ),
        pack(
            "ID",
            "2026-07-23",
            StateRule { round_to_dollar: true, ..flat(53_000, 1_610_000, 3_220_000) },
            "Idaho EPB00744 (07-23-2026), Table for Percentage Computation Method of Withholding: 5.3% over $16,100 / $32,200 (married); allowances $0 (child tax credit sunset); round to the whole dollar",
        ),
        pack(
            "IL",
            "2026-01-01",
            StateRule { allowance_deduction: 292_500, second_allowance_deduction: 100_000, ..flat(49_500, 0, 0) },
            "Illinois Booklet IL-700-T (R-12/25), 2026: 4.95%; IL-W-4 line 1 allowances $2,925, line 2 allowances $1,000",
        ),
        pack(
            "IN",
            "2026-01-01",
            StateRule { allowance_deduction: 100_000, second_allowance_deduction: 150_000, ..flat(29_500, 0, 0) },
            "Indiana Departmental Notice #1 (2026): 2.95% state rate; WH-4 personal exemptions $1,000, dependents $1,500 (county tax not included)",
        ),
        pack(
            "IA",
            "2026-01-01",
            StateRule { allowance_credit: 4_000, standard_deduction_hoh: Some(1_950_000), ..flat(38_000, 1_300_000, 2_600_000) },
            "Iowa Individual Income Tax Withholding Formula, effective 1 January 2026: 3.8%; $13,000 / $26,000 (married, spouse without earned income) / $19,500 (head of household); IA W-4 line 7 dollars as a credit ($40 per allowance on a 2023 or earlier IA W-4)",
        ),
        pack(
            "KY",
            "2026-01-01",
            flat(35_000, 336_000, 336_000),
            "Kentucky 2026 Withholding Tax Formula, 42A003 (TCF)(10-2025): 3.5% after a $3,360 standard deduction",
        ),
        pack(
            "LA",
            "2026-01-01",
            StateRule { standard_deduction_hoh: Some(2_575_000), ..flat(30_900, 1_287_500, 2_575_000) },
            "Louisiana R-1306 (1/26), Withholding Tables and Formulas effective 1 January 2026: 3.09%; $12,875 / $25,750 (married filing jointly, surviving spouse, head of household)",
        ),
        pack(
            "MI",
            "2026-01-01",
            StateRule { allowance_deduction: 590_000, ..flat(42_500, 0, 0) },
            "Michigan Form 446 (Rev. 02-26), 2026 Michigan Income Tax Withholding Guide: 4.25%; $5,900 per MI-W4 exemption",
        ),
        pack(
            "NC",
            "2026-01-01",
            StateRule {
                allowance_deduction: 250_000,
                standard_deduction_hoh: Some(1_912_500),
                round_to_dollar: true,
                ..flat(40_900, 1_275_000, 1_275_000)
            },
            "North Carolina NC-30 (Web 11-25), 2026 Income Tax Withholding Tables and Instructions: 4.09%; $12,750 / $19,125 (head of household); $2,500 per NC-4 allowance; round to the whole dollar",
        ),
        pack(
            "PA",
            "2026-01-01",
            StateRule { excludes_401k: false, ..flat(30_700, 0, 0) },
            "Pennsylvania REV-415 (SU) 01-23, Employer Withholding Information Guide, and pa.gov Employer Withholding: 3.07% of compensation; 401(k) deferrals are taxable (section 125 health plans are not)",
        ),
    ]
}

/// The built-in rule for `state` on pay date `date`, if there is one.
pub fn built_in(state: &str, date: &str) -> Option<Pack> {
    all().into_iter().filter(|p| p.state == state && p.from <= date).max_by(|a, b| a.from.cmp(b.from))
}

#[cfg(test)]
mod tests {
    //! Each state's worked examples from its own 2026 publication (those marked "computed" are
    //! worked from the published formula where the state prints no example).
    use super::*;
    use crate::us_state::{withholding, StateEmployee};
    use app_core::money::Money;

    fn usd(s: &str) -> Money {
        Money::parse_decimal(s).unwrap_or_else(|| panic!("{s}"))
    }

    fn rule(state: &str, date: &str) -> StateRule {
        built_in(state, date).unwrap_or_else(|| panic!("no {state} rule on {date}")).rule
    }

    fn tax(state: &str, wages: &str, periods: i64, e: StateEmployee) -> Money {
        withholding(usd(wages), periods, &rule(state, "2026-11-06"), &e).unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn single() -> StateEmployee {
        StateEmployee::default()
    }

    #[test]
    fn kentucky_monthly_example() {
        // 42A003: monthly $3,270 → (39,240 − 3,360) × 3.5% ÷ 12 = 104.65.
        assert_eq!(tax("KY", "3270", 12, single()), usd("104.65"));
    }

    #[test]
    fn iowa_examples() {
        // Withholding formula 2026, examples 1, 3 and 4.
        let credit = |dollars: i64| StateEmployee { annual_credit: dollars * 100, ..single() };
        assert_eq!(tax("IA", "2100", 26, credit(40)), usd("59.26"));
        assert_eq!(tax("IA", "2100", 26, StateEmployee { head_of_household: true, ..credit(160) }), usd("45.15"));
        assert_eq!(tax("IA", "5000", 12, credit(40)), usd("145.50"));
        // A 2023 IA W-4 with 1 allowance is the same $40 credit.
        assert_eq!(tax("IA", "2100", 26, StateEmployee { allowances: 1, ..single() }), usd("59.26"));
    }

    #[test]
    fn louisiana_examples() {
        // R-1306: weekly $700 single → 13.98; biweekly $4,600 married → 111.54.
        assert_eq!(tax("LA", "700", 52, single()), usd("13.98"));
        assert_eq!(tax("LA", "4600", 26, StateEmployee { married: true, ..single() }), usd("111.54"));
    }

    #[test]
    fn indiana_example() {
        // Departmental Notice #1: weekly $800, 5 personal, 3 dependents, 1 first-time dependent,
        // 2 adopted (= 4 more dependents) → 13.96.
        assert_eq!(
            tax("IN", "800", 52, StateEmployee { allowances: 5, second_allowances: 8, ..single() }),
            usd("13.96")
        );
    }

    #[test]
    fn illinois_example() {
        // IL-700-T "Alice": weekly $800, 2 line 1 and 2 line 2 allowances → 32.13.
        assert_eq!(
            tax("IL", "800", 52, StateEmployee { allowances: 2, second_allowances: 2, ..single() }),
            usd("32.13")
        );
    }

    #[test]
    fn north_carolina_example() {
        // NC-30: single, weekly $450, 2 allowances → 231.09 ÷ 52 = 4.44 → $4.
        assert_eq!(tax("NC", "450", 52, StateEmployee { allowances: 2, ..single() }), usd("4"));
    }

    #[test]
    fn georgia_examples_and_the_mid_year_rate() {
        // Guide 2026 p. 51–52: married (one spouse working), semimonthly $2,000, 1 allowance → 27.03;
        // head of household, biweekly $935, 2 allowances → 0.
        let married = StateEmployee { married: true, allowances: 1, ..single() };
        assert_eq!(tax("GA", "2000", 24, married.clone()), usd("27.03"));
        assert_eq!(
            tax("GA", "935", 26, StateEmployee { head_of_household: true, allowances: 2, ..single() }),
            Money::ZERO
        );
        // Before 11 May 2026: 5.19% → (48,000 − 35,000) × 5.19% ÷ 24 = 28.11.
        let before = withholding(usd("2000"), 24, &rule("GA", "2026-05-10"), &married).expect("tax");
        assert_eq!(before, usd("28.11"));
    }

    #[test]
    fn idaho_examples() {
        // Computing Withholding: unmarried, biweekly $1,212 → $31; married, weekly $1,000 → $20.
        assert_eq!(tax("ID", "1212", 26, StateEmployee { allowances: 4, ..single() }), usd("31"));
        assert_eq!(tax("ID", "1000", 52, StateEmployee { married: true, allowances: 4, ..single() }), usd("20"));
        assert!(built_in("ID", "2026-07-22").is_none(), "the January–July 2026 table is not built in");
    }

    #[test]
    fn computed_examples() {
        // Michigan: biweekly $2,000, 2 exemptions → (52,000 − 11,800) × 4.25% ÷ 26 = 65.71.
        assert_eq!(tax("MI", "2000", 26, StateEmployee { allowances: 2, ..single() }), usd("65.71"));
        // Colorado: biweekly $2,000, single → (52,000 − 5,500) × 4.40% ÷ 26 = 78.69; DR 0004 $10,000.
        assert_eq!(tax("CO", "2000", 26, single()), usd("78.69"));
        assert_eq!(
            tax("CO", "2000", 26, StateEmployee { deduction_override: Some(1_000_000), ..single() }),
            usd("71.08")
        );
        // Pennsylvania: biweekly $2,000 → 61.40. Arizona: 2.5% of $2,000 = 50.00; no A-4 = 2.0%.
        assert_eq!(tax("PA", "2000", 26, single()), usd("61.40"));
        assert_eq!(tax("AZ", "2000", 26, StateEmployee { elected_rate: Some(25_000), ..single() }), usd("50"));
        assert_eq!(tax("AZ", "2000", 26, single()), usd("40"));
    }

    #[test]
    fn every_pack_is_well_formed() {
        for p in all() {
            assert!(crate::us_setup::STATES.contains(&p.state), "{}", p.state);
            assert!(!crate::us_state::NO_INCOME_TAX.contains(&p.state), "{}", p.state);
            assert!(p.from.len() == 10 && p.from.starts_with("2026-"), "{} {}", p.state, p.from);
            assert!((0..=1_000_000).contains(&p.rule.flat_rate), "{}", p.state);
            assert!(!p.source.is_empty());
            assert_eq!(
                crate::us_state::StateRule::from_snapshot(&p.rule.to_snapshot()).as_ref(),
                Some(&p.rule),
                "{}",
                p.state
            );
        }
    }
}
