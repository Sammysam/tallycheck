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
//! D65, step 2, the bracket states: Delaware, Hawaii, Kansas (K-4 lines B + C personal allowances
//! $9,160, lines D + E dependents / head of household $2,320), Minnesota, Mississippi (Form 89-350
//! line 6 exemption dollars as `annual deduction`; married with an employed spouse uses the single
//! amount), Missouri (married with a working spouse uses the single amount), Montana, Nebraska, New
//! Mexico, North Dakota (allowances only on a W-4 from 2019 or earlier), Ohio (from 1 August 2026),
//! Oklahoma, Rhode Island, Vermont, Virginia (VA-4 personal exemptions $930, 65+ / blind $800) and
//! West Virginia (IT-104 two-earner table = single, one-earner table = married).
//! D66, step 3: Alabama, Arkansas, California, Connecticut, Maine, Maryland, New Jersey, New York
//! and Wisconsin use their own formula ([`crate::us_state_formulas`]).
//! Not built in: Oregon (its formula and its tables disagree by $263 a year), Utah and
//! Massachusetts (2026 publications not yet read), South Carolina and DC (no 2026 withholding
//! method published).

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
        brackets_hoh: None,
        allowance_wage_limit: None,
        round_annual_to_dollar: false,
        formula: None,
    }
}

/// A rule worked out by its state's own formula (D66).
fn formula(id: &str) -> StateRule {
    StateRule { method: "FORMULA".into(), formula: Some(id.into()), ..flat(0, 0, 0) }
}

/// Brackets from `(upper bound in dollars, rate in ppm)`, the last bound 0 meaning no limit.
fn br(rows: &[(i64, i64)]) -> Vec<crate::us_state::Bracket> {
    rows.iter().map(|(upper, rate)| (if *upper == 0 { None } else { Some(upper * 100) }, *rate)).collect()
}

/// A bracket rule: single brackets, married brackets (None = the same) and standard deductions.
fn bracketed(single: &[(i64, i64)], married: Option<&[(i64, i64)]>, std_single: i64, std_married: i64) -> StateRule {
    StateRule {
        method: "BRACKETS".into(),
        brackets: br(single),
        brackets_married: married.map(br),
        ..flat(0, std_single, std_married)
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
        // ── D66, step 3: states with their own formula (crate::us_state_formulas) ──
        pack(
            "AL",
            "2026-01-01",
            formula("AL-2026"),
            "Alabama Withholding Tax Tables and Instructions for Employers and Withholding Agents (Revised January 2026): standard deduction phased out by income, federal income tax withheld subtracted, A4 personal exemption and dependents; 2% / 4% / 5%",
        ),
        pack(
            "AR",
            "2026-01-01",
            formula("AR-2026"),
            "Arkansas Withholding Tax Formula Method, effective 1 January 2026 (revised for H.B. 1001, top rate 3.7%): $2,470 standard deduction, $100 midrange table, $29 credit per AR4EC exemption (low-income tables not offered)",
        ),
        pack(
            "CA",
            "2026-01-01",
            formula("CA-2026"),
            "California Withholding Schedules for 2026, Method B (Exact Calculation Method), EDD: low-income exemption, $1,000 per DE 4 estimated-deduction allowance, standard deduction, annual rate tables, $168.30 credit per regular allowance (annualized)",
        ),
        pack(
            "MD",
            "2026-01-01",
            formula("MD-2026"),
            "Maryland Employer Withholding Guide effective January 2026 (Revised December 2025): $3,400 standard deduction, $3,200 per MW507 exemption, state rates from 4.75% plus the county rate (2026 county rates, Comptroller memo of 4 February 2026)",
        ),
        pack(
            "ME",
            "2026-01-01",
            formula("ME-2026"),
            "Maine 2026 Withholding Tables for Individual Income Tax (Revised August 2026, retroactive to 1 January 2026): standard deduction phased out by wages, $5,300 per W-4ME allowance, 5.8% / 6.75% / 7.15% and the 2% surcharge; whole dollars",
        ),
        pack(
            "NY",
            "2026-01-01",
            formula("NY-2026"),
            "New York State NYS-50-T-NYS (1/26), Method II exact calculation tables II-A to II-D and Method III (New York City and Yonkers taxes are not included)",
        ),
        pack(
            "CT",
            "2026-01-01",
            formula("CT-2026"),
            "Connecticut TPG-211, 2026 Withholding Calculation Rules (Rev. 12/25): CT-W4 withholding code A, B, C, D, E or F; Tables A–E (exemption, initial tax, 2% add-back, recapture, personal credit); no CT-W4 on file: 6.99%",
        ),
        pack(
            "NJ",
            "2026-01-01",
            formula("NJ-2026"),
            "New Jersey NJ-WT (September 2025) and Tables for Percentage Method of Withholding (from 1 October 2020, current for 2026): rate tables A–E per pay period, NJ-W4 rate letter or A (single / separate) / B (joint / head of household)",
        ),
        pack(
            "WI",
            "2026-01-01",
            formula("WI-2026"),
            "Wisconsin Publication W-166 (1/26), alternate method: deduction phased out by wages, $400 per WT-4 exemption, 3.54% / 4.65% / 5.30% / 7.65% (rates unchanged for 2026, Withholding Tax Update 2026-1)",
        ),
        // ── D65, step 2: bracket states ──────────────────────────────────────────
        pack(
            "DE",
            "2026-01-01",
            StateRule {
                allowance_credit: 11_000,
                ..bracketed(&[(2_000, 0), (5_000, 22_000), (10_000, 39_000), (20_000, 48_000), (25_000, 52_000), (60_000, 55_500), (0, 66_000)], None, 325_000, 650_000)
            },
            "Delaware Division of Revenue, Employer's Guide §17, tax computation table effective 1 January 2025 (unchanged for 2026): $3,250 / $6,500 standard deduction; $110 credit per allowance",
        ),
        pack(
            "HI",
            "2026-01-01",
            StateRule {
                allowance_deduction: 114_400,
                ..bracketed(
                    &[(9_600, 14_000), (14_400, 32_000), (19_200, 55_000), (24_000, 64_000), (36_000, 68_000), (48_000, 72_000), (125_000, 76_000), (0, 79_000)],
                    Some(&[(19_200, 14_000), (28_800, 32_000), (38_400, 55_000), (48_000, 64_000), (72_000, 68_000), (96_000, 72_000), (250_000, 76_000), (0, 79_000)]),
                    435_000,
                    435_000,
                )
            },
            "Hawaii Booklet A, Employer's Tax Guide (Rev. 2025), for withholding from 1 January 2026: $4,350 lump-sum allowance; $1,144 per HW-4 allowance",
        ),
        pack(
            "KS",
            "2026-01-01",
            StateRule {
                allowance_deduction: 916_000,
                second_allowance_deduction: 232_000,
                ..bracketed(&[(23_000, 52_000), (0, 55_800)], Some(&[(46_000, 52_000), (0, 55_800)]), 360_500, 824_000)
            },
            "Kansas KW-100 Withholding Tax Guide (Rev. 10-24; no 2026 rate change, Notice 25-06): $3,605 / $8,240; K-4 personal allowances $9,160, dependents and head of household $2,320",
        ),
        pack(
            "MN",
            "2026-01-01",
            StateRule {
                allowance_deduction: 530_000,
                ..bracketed(
                    &[(4_700, 0), (38_010, 53_500), (114_130, 68_000), (207_850, 78_500), (0, 98_500)],
                    Some(&[(14_700, 0), (63_400, 53_500), (208_180, 68_000), (352_630, 78_500), (0, 98_500)]),
                    0,
                    0,
                )
            },
            "Minnesota Withholding Tax Instruction Booklet and Tax Tables 2026, computer formula: $5,300 per W-4MN allowance",
        ),
        pack(
            "MS",
            "2026-01-01",
            StateRule {
                standard_deduction_hoh: Some(340_000),
                round_to_dollar: true,
                ..bracketed(&[(10_000, 0), (0, 40_000)], None, 230_000, 460_000)
            },
            "Mississippi Pub 89-700 (tables effective 1 January 2026) and Computer Payroll Accounting 2026: 0% to $10,000, 4% above; $2,300 / $4,600 (spouse not employed) / $3,400 head of family; Form 89-350 exemption dollars; whole dollars",
        ),
        pack(
            "MO",
            "2026-01-01",
            StateRule {
                standard_deduction_hoh: Some(2_415_000),
                round_to_dollar: true,
                ..bracketed(
                    &[(1_348, 0), (2_696, 20_000), (4_044, 25_000), (5_392, 30_000), (6_740, 35_000), (8_088, 40_000), (9_436, 45_000), (0, 47_000)],
                    None,
                    1_610_000,
                    3_220_000,
                )
            },
            "Missouri 2026 Withholding Tax Formula (Form 4282 Rev. 03-2026): $16,100 / $32,200 (spouse does not work) / $24,150 head of household; whole dollars",
        ),
        pack(
            "MT",
            "2026-01-01",
            StateRule {
                standard_deduction_hoh: Some(2_415_000),
                brackets_hoh: Some(br(&[(71_250, 47_000), (0, 56_500)])),
                round_to_dollar: true,
                ..bracketed(&[(47_500, 47_000), (0, 56_500)], Some(&[(95_000, 47_000), (0, 56_500)]), 1_610_000, 3_220_000)
            },
            "Montana Employer and Information Agent Guide with Tax Tables (V4 November 2025), 2026 formula: $16,100 / $32,200 / $24,150; 4.7% and 5.65%; whole dollars",
        ),
        pack(
            "NE",
            "2026-01-01",
            StateRule {
                allowance_deduction: 244_000,
                ..bracketed(
                    &[(3_430, 0), (6_710, 22_600), (21_810, 32_200), (31_610, 42_100), (40_130, 43_500), (75_370, 44_800), (0, 46_000)],
                    Some(&[(8_190, 0), (13_010, 22_600), (32_400, 32_200), (50_400, 42_100), (62_530, 43_500), (82_920, 44_800), (0, 46_000)]),
                    0,
                    0,
                )
            },
            "Nebraska Circular EN 2026 (Rev. 11-2025): $2,440 per W-4N allowance (the 1.5% minimum for employers of more than 24 is not applied)",
        ),
        pack(
            "NM",
            "2026-01-01",
            StateRule {
                standard_deduction_hoh: Some(1_207_500),
                brackets_hoh: Some(br(&[(8_000, 15_000), (25_000, 32_000), (50_000, 43_000), (100_000, 47_000), (315_000, 49_000), (0, 59_000)])),
                ..bracketed(
                    &[(5_500, 15_000), (16_500, 32_000), (33_500, 43_000), (66_500, 47_000), (210_000, 49_000), (0, 59_000)],
                    Some(&[(8_000, 15_000), (25_000, 32_000), (50_000, 43_000), (100_000, 47_000), (315_000, 49_000), (0, 59_000)]),
                    805_000,
                    1_610_000,
                )
            },
            "New Mexico FYI-104 (Rev. 11/2025), effective 1 January 2026: $8,050 / $16,100 / $12,075 head of household (married brackets)",
        ),
        pack(
            "ND",
            "2026-01-01",
            StateRule {
                allowance_deduction: 505_000,
                brackets_hoh: Some(br(&[(78_475, 0), (289_675, 19_500), (0, 25_000)])),
                round_to_dollar: true,
                ..bracketed(&[(57_625, 0), (258_450, 19_500), (0, 25_000)], Some(&[(57_500, 0), (168_525, 19_500), (0, 25_000)]), 0, 0)
            },
            "North Dakota Income Tax Withholding Rates and Instructions for wages paid in 2026 (form 28216, 12/2025): $5,050 per allowance on a W-4 from 2019 or earlier; whole dollars",
        ),
        pack(
            "OH",
            "2026-08-01",
            StateRule {
                allowance_deduction: 65_000,
                ..bracketed(&[(26_050, 16_000), (100_000, 29_900), (0, 34_000)], None, 0, 0)
            },
            "Ohio Employer Withholding Optional Computer Formula, effective 1 August 2026 (Rev. 07/26): $650 per IT 4 exemption (earlier 2026 pay dates are not built in)",
        ),
        pack(
            "OK",
            "2026-01-01",
            StateRule {
                allowance_deduction: 100_000,
                round_to_dollar: true,
                ..bracketed(
                    &[(10_100, 0), (11_250, 25_000), (13_550, 35_000), (0, 45_000)],
                    Some(&[(20_200, 0), (22_500, 25_000), (27_100, 35_000), (0, 45_000)]),
                    0,
                    0,
                )
            },
            "Oklahoma OW-2, 2026 Oklahoma Income Tax Withholding Tables (Rev. 11-2025): $1,000 per allowance; whole dollars",
        ),
        pack(
            "RI",
            "2026-01-01",
            StateRule {
                allowance_deduction: 100_000,
                allowance_wage_limit: Some(29_080_000),
                ..bracketed(&[(82_050, 37_500), (186_450, 47_500), (0, 59_900)], None, 0, 0)
            },
            "Rhode Island 2026 Employer's Income Tax Withholding Tables (updated 11/04/2025): $1,000 per exemption, none above $290,800 of annual wages",
        ),
        pack(
            "VT",
            "2026-01-01",
            StateRule {
                allowance_deduction: 540_000,
                ..bracketed(
                    &[(50_750, 33_500), (122_850, 66_000), (256_300, 76_000), (0, 87_500)],
                    Some(&[(84_700, 33_500), (204_750, 66_000), (312_050, 76_000), (0, 87_500)]),
                    392_500,
                    1_177_500,
                )
            },
            "Vermont GB-1210-2026, 2026 Income Tax Withholding Instructions, Tables and Charts: $3,925 / $11,775; $5,400 per W-4VT allowance",
        ),
        pack(
            "VA",
            "2026-01-01",
            StateRule {
                allowance_deduction: 93_000,
                second_allowance_deduction: 80_000,
                round_annual_to_dollar: true,
                ..bracketed(&[(3_000, 20_000), (5_000, 30_000), (17_000, 50_000), (0, 57_500)], None, 875_000, 875_000)
            },
            "Virginia Employer Withholding Instructions (Rev. 10/26), formula for wages paid after 1 July 2025: $8,750; VA-4 exemptions $930, 65+ / blind $800",
        ),
        pack(
            "WV",
            "2026-01-01",
            StateRule {
                allowance_deduction: 200_000,
                round_to_dollar: true,
                ..bracketed(
                    &[(7_500, 22_200), (18_750, 29_600), (30_000, 33_300), (45_000, 44_400), (0, 48_200)],
                    Some(&[(10_000, 22_200), (25_000, 29_600), (40_000, 33_300), (60_000, 44_400), (0, 48_200)]),
                    0,
                    0,
                )
            },
            "West Virginia IT-100.2A (October 2024 tables, in use until the 2026 rate cut): $2,000 per exemption; two-earner / one-earner tables; whole dollars",
        ),
        pack(
            "WV",
            "2026-04-01",
            StateRule {
                allowance_deduction: 200_000,
                round_to_dollar: true,
                ..bracketed(
                    &[(7_500, 21_100), (18_750, 28_100), (30_000, 31_600), (45_000, 42_200), (0, 45_800)],
                    Some(&[(10_000, 21_100), (25_000, 28_100), (40_000, 31_600), (60_000, 42_200), (0, 45_800)]),
                    0,
                    0,
                )
            },
            "West Virginia IT-100.2A (March 2026), SB 392 rates: $2,000 per exemption; two-earner / one-earner tables; whole dollars (applied from 1 April 2026; the state gave no start date)",
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
    fn step_2_bracket_state_examples() {
        let s = single;
        let m = |allowances: i64| StateEmployee { married: true, allowances, ..single() };
        let a = |allowances: i64| StateEmployee { allowances, ..single() };
        // Vermont GB-1210 p.7: weekly $1,800, married, 2 allowances → 45.76 annualized (the
        // per-period table, with rounded figures, prints 45.77).
        assert_eq!(tax("VT", "1800", 52, m(2)), usd("45.76"));
        // Virginia p.22: semimonthly $2,649, 5 exemptions → $2,628 a year → 109.50.
        assert_eq!(tax("VA", "2649", 24, a(5)), usd("109.50"));
        // Oklahoma OW-2 p.7: semimonthly $1,825, married, 2 allowances → $37.
        assert_eq!(tax("OK", "1825", 24, m(2)), usd("37"));
        // Rhode Island: weekly $2,195, 1 exemption → 87.57; above $290,800 a year no exemption.
        assert_eq!(tax("RI", "2195", 52, a(1)), usd("87.57"));
        let rich = tax("RI", "6000", 52, a(3));
        assert_eq!(rich, tax("RI", "6000", 52, s()), "exemptions drop to zero above the limit");
        // North Dakota: weekly $1,800 single → $13 (2020+ W-4); 2 allowances on an older W-4 → $10.
        assert_eq!(tax("ND", "1800", 52, s()), usd("13"));
        assert_eq!(tax("ND", "1800", 52, a(2)), usd("10"));
        // Minnesota: weekly $1,000 single, 2 allowances → 38.70 (the table shows $39 rounded).
        assert_eq!(tax("MN", "1000", 52, a(2)), usd("38.70"));
        // Missouri: $35,000 a year paid monthly, married with a working spouse (single amount) → $59.
        assert_eq!(tax("MO", "2916.67", 12, s()), usd("59"));
        // Montana: semimonthly $1,375 single → $33; head of household $4,950 biweekly → $201; both
        // spouses working (single table) $2,950 biweekly → $114. Married, biweekly $5,950: the
        // guide's per-period table (rounded thresholds) prints $232; the annual formula gives
        // 6,018.75 ÷ 26 = 231.49 → $231 (the guide notes such "insignificant variances").
        assert_eq!(tax("MT", "1375", 24, s()), usd("33"));
        assert_eq!(tax("MT", "5950", 26, m(0)), usd("231"));
        assert_eq!(tax("MT", "4950", 26, StateEmployee { head_of_household: true, ..single() }), usd("201"));
        assert_eq!(tax("MT", "2950", 26, s()), usd("114"));
        // Nebraska: weekly $1,005 single, 1 allowance → 34.19.
        assert_eq!(tax("NE", "1005", 52, a(1)), usd("34.19"));
        // New Mexico p.2: married, weekly $1,000 → 21.78 annualized (+ $20 extra = 41.78).
        assert_eq!(tax("NM", "1000", 52, StateEmployee { extra: usd("20"), ..m(0) }), usd("41.78"));
        // Delaware: $25,000 a year single, 1 allowance → $722 a year (biweekly 27.77); married with 3
        // allowances → $339 (weekly 6.52).
        assert_eq!(tax("DE", "961.54", 26, a(1)), usd("27.77"));
        assert_eq!(tax("DE", "480.77", 52, m(3)), usd("6.52"));
        // Hawaii Booklet A: weekly $500 single, 3 allowances → 9.58.
        assert_eq!(tax("HI", "500", 52, a(3)), usd("9.58"));
        // Kansas KW-100 p.6: married, semimonthly $2,000, 3 allowances (2 personal, 1 dependent) →
        // 41.43 annualized (41.44 by the per-period table).
        assert_eq!(tax("KS", "2000", 24, StateEmployee { second_allowances: 1, ..m(2) }), usd("41.43"));
        // Mississippi (computed): single, biweekly $2,000, $6,000 exemption → 51.85 → $52.
        let ms = StateEmployee { annual_deduction: 600_000, ..single() };
        assert_eq!(tax("MS", "2000", 26, ms.clone()), usd("52"));
        assert_eq!(tax("MS", "1890", 26, ms), usd("47"), "the official table shows 47 at $1,890");
        // Ohio (computed, from August 2026): biweekly $2,000, 2 exemptions → 44.38.
        assert_eq!(tax("OH", "2000", 26, a(2)), usd("44.38"));
        assert!(built_in("OH", "2026-07-31").is_none());
        // West Virginia (computed): biweekly $2,000, 2 exemptions, two-earner → 61.55 → $62; the
        // rate cut applies from April 2026.
        assert_eq!(tax("WV", "2000", 26, a(2)), usd("62"));
        let before = withholding(usd("2000"), 26, &rule("WV", "2026-03-31"), &a(2)).expect("tax");
        assert_eq!(before, usd("65"));
    }

    #[test]
    fn every_pack_is_well_formed() {
        for p in all() {
            assert!(crate::us_setup::STATES.contains(&p.state), "{}", p.state);
            assert!(!crate::us_state::NO_INCOME_TAX.contains(&p.state), "{}", p.state);
            assert!(p.from.len() == 10 && p.from.starts_with("2026-"), "{} {}", p.state, p.from);
            assert!((0..=1_000_000).contains(&p.rule.flat_rate), "{}", p.state);
            assert!(!p.source.is_empty());
            // Every built-in formula exists and works out a paycheck.
            let e = crate::us_state::StateEmployee { allowances: 1, ..Default::default() };
            assert!(crate::us_state::withholding(Money::from_cents(250_000), 26, &p.rule, &e).is_ok(), "{}", p.state);
            assert_eq!(
                crate::us_state::StateRule::from_snapshot(&p.rule.to_snapshot()).as_ref(),
                Some(&p.rule),
                "{}",
                p.state
            );
        }
    }
}
