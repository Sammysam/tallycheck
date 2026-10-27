//! United States payroll taxes for one pay period of one employee: federal income tax withholding
//! (IRS Publication 15-T, Worksheet 1A with the Percentage Method tables for automated payroll
//! systems), social security and Medicare (FICA, including Additional Medicare Tax), FUTA and the
//! employer's state unemployment tax (SUTA). State and local income tax are not covered yet.
//!
//! Sources for 2026: Publication 15-T (2026) page 10 (Worksheet 1A) and page 12 (annual tables);
//! Publication 15 (2026): social security 6.2% each on wages up to $184,500, Medicare 1.45% each,
//! Additional Medicare Tax 0.9% withheld on wages over $200,000 in the calendar year (no employer
//! share), FUTA 6.0% on the first $7,000 with a credit of up to 5.4% (0.6% after the credit,
//! unless the state is a credit reduction state). The withholding arithmetic is checked against
//! every row of the 2026 wage bracket tables (tests/fixtures/us_2026_wage_brackets.json).
//!
//! All arithmetic is in integer cents. Rates are in parts per million (6.2% = 62_000) so that any
//! published rate is exact. Each tax is rounded half-up to the cent once.

use app_core::money::Money;
use app_core::{AppError, AppResult};

/// 100% in parts per million.
pub const PPM: i64 = 1_000_000;

/// Filing status from Step 1(c) of a 2020 or later Form W-4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilingStatus {
    /// Single or married filing separately.
    Single,
    MarriedJointly,
    HeadOfHousehold,
}

/// The employee's Form W-4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum W4 {
    /// A Form W-4 from 2020 or later.
    Since2020 {
        status: FilingStatus,
        /// Step 2(c): multiple jobs or spouse works.
        step2_checkbox: bool,
        /// Step 3: credits for dependents and others (annual).
        credits: Money,
        /// Step 4(a): other income (annual).
        other_income: Money,
        /// Step 4(b): deductions (annual).
        deductions: Money,
        /// Step 4(c): extra withholding each pay period.
        extra: Money,
    },
    /// A Form W-4 from 2019 or earlier (married uses the married filing jointly table).
    Before2020 { married: bool, allowances: i64, extra: Money },
}

/// Wages already paid this calendar year, before this pay period, for the wage-base limits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct YearToDate {
    pub social_security_wages: Money,
    pub medicare_wages: Money,
    pub futa_wages: Money,
    pub suta_wages: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsInputs {
    /// Calendar year of the pay date: picks the tax tables.
    pub year: i32,
    /// Pay periods per year: 260 daily, 52 weekly, 26 biweekly, 24 semimonthly, 12 monthly,
    /// 4 quarterly, 2 semiannually (Publication 15-T, Table 3).
    pub periods_per_year: i64,
    /// Gross wages this pay period.
    pub gross: Money,
    /// Pre-tax deductions that reduce income tax wages only (e.g. traditional 401(k)).
    pub pretax_income_tax_only: Money,
    /// Pre-tax deductions that reduce income tax, FICA and FUTA wages (e.g. section 125 plans).
    pub pretax_all_taxes: Money,
    pub w4: W4,
    /// Exempt from federal income tax withholding (a valid exemption claimed on Form W-4).
    pub exempt_from_withholding: bool,
    pub ytd: YearToDate,
    /// The employer's state unemployment rate and wage base (None = no limit), from the state.
    pub suta_rate: i64,
    pub suta_wage_base: Option<Money>,
    /// Extra FUTA rate when the state is a credit reduction state for the year (usually 0).
    pub futa_credit_reduction: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsFigures {
    pub income_tax_wages: Money,
    pub federal_income_tax: Money,
    pub social_security_wages: Money,
    pub social_security_employee: Money,
    pub social_security_employer: Money,
    pub medicare_wages: Money,
    pub medicare_employee: Money,
    pub additional_medicare: Money,
    pub medicare_employer: Money,
    pub futa_wages: Money,
    pub futa: Money,
    pub suta_wages: Money,
    pub suta: Money,
    /// Federal income tax + social security + Medicare + Additional Medicare.
    pub employee_taxes: Money,
    /// Gross − pre-tax deductions − employee taxes.
    pub net_pay: Money,
    /// Social security + Medicare + FUTA + SUTA (employer shares).
    pub employer_taxes: Money,
    /// Gross + employer taxes.
    pub cost_to_employer: Money,
}

// ─── Tables ─────────────────────────────────────────────────────────────────

/// One row of an annual percentage method schedule: at least `over` (cents), withhold `base`
/// (cents) plus `percent` of the amount over `over`.
#[derive(Debug, Clone, Copy)]
struct Bracket {
    over: i64,
    base: i64,
    percent: i64,
}

const fn b(over_dollars: i64, base_cents: i64, percent: i64) -> Bracket {
    Bracket { over: over_dollars * 100, base: base_cents, percent }
}

/// The federal figures for one calendar year.
pub struct UsTables {
    year: i32,
    /// Worksheet 1A line 1g: (married filing jointly, everyone else), cents.
    std_mfj: i64,
    std_other: i64,
    /// Worksheet 1A line 1k: per withholding allowance on a 2019 or earlier Form W-4, cents.
    allowance: i64,
    /// [standard, Step 2 checkbox] for each filing status.
    mfj: [&'static [Bracket]; 2],
    single: [&'static [Bracket]; 2],
    hoh: [&'static [Bracket]; 2],
    social_security_rate: i64,
    social_security_wage_base: i64,
    medicare_rate: i64,
    additional_medicare_rate: i64,
    additional_medicare_threshold: i64,
    futa_rate: i64,
    futa_max_credit: i64,
    futa_wage_base: i64,
}

// Publication 15-T (2026), page 12.
const MFJ_2026: [Bracket; 8] = [
    b(0, 0, 0),
    b(19_300, 0, 10),
    b(44_100, 248_000, 12),
    b(120_100, 1_160_000, 22),
    b(230_700, 3_593_200, 24),
    b(422_850, 8_204_800, 32),
    b(531_750, 11_689_600, 35),
    b(788_000, 20_658_350, 37),
];
const MFJ_CHECKBOX_2026: [Bracket; 8] = [
    b(0, 0, 0),
    b(16_100, 0, 10),
    b(28_500, 124_000, 12),
    b(66_500, 580_000, 22),
    b(121_800, 1_796_600, 24),
    b(217_875, 4_102_400, 32),
    b(272_325, 5_844_800, 35),
    b(400_450, 10_329_175, 37),
];
const SINGLE_2026: [Bracket; 8] = [
    b(0, 0, 0),
    b(7_500, 0, 10),
    b(19_900, 124_000, 12),
    b(57_900, 580_000, 22),
    b(113_200, 1_796_600, 24),
    b(209_275, 4_102_400, 32),
    b(263_725, 5_844_800, 35),
    b(648_100, 19_297_925, 37),
];
const SINGLE_CHECKBOX_2026: [Bracket; 8] = [
    b(0, 0, 0),
    b(8_050, 0, 10),
    b(14_250, 62_000, 12),
    b(33_250, 290_000, 22),
    b(60_900, 898_300, 24),
    b(108_938, 2_051_200, 32),
    b(136_163, 2_922_400, 35),
    b(328_350, 9_648_963, 37),
];
const HOH_2026: [Bracket; 8] = [
    b(0, 0, 0),
    b(15_550, 0, 10),
    b(33_250, 177_000, 12),
    b(83_000, 774_000, 22),
    b(121_250, 1_615_500, 24),
    b(217_300, 3_920_700, 32),
    b(271_750, 5_663_100, 35),
    b(656_150, 19_117_100, 37),
];
const HOH_CHECKBOX_2026: [Bracket; 8] = [
    b(0, 0, 0),
    b(12_075, 0, 10),
    b(20_925, 88_500, 12),
    b(45_800, 387_000, 22),
    b(64_925, 807_750, 24),
    b(112_950, 1_960_350, 32),
    b(140_175, 2_831_550, 35),
    b(332_375, 9_558_550, 37),
];

const TABLES: &[UsTables] = &[UsTables {
    year: 2026,
    std_mfj: 1_290_000,
    std_other: 860_000,
    allowance: 430_000,
    mfj: [&MFJ_2026, &MFJ_CHECKBOX_2026],
    single: [&SINGLE_2026, &SINGLE_CHECKBOX_2026],
    hoh: [&HOH_2026, &HOH_CHECKBOX_2026],
    social_security_rate: 62_000,
    social_security_wage_base: 18_450_000,
    medicare_rate: 14_500,
    additional_medicare_rate: 9_000,
    additional_medicare_threshold: 20_000_000,
    futa_rate: 60_000,
    futa_max_credit: 54_000,
    futa_wage_base: 700_000,
}];

/// The tables for `year`, or a clear error when that year hasn't been added.
pub fn tables(year: i32) -> AppResult<&'static UsTables> {
    TABLES
        .iter()
        .find(|t| t.year == year)
        .ok_or_else(|| AppError::bad_request(format!("US payroll tax tables for {year} are not set up yet")))
}

/// Years with tables, for the screens.
pub fn years() -> Vec<i32> {
    TABLES.iter().map(|t| t.year).collect()
}

// ─── Arithmetic ─────────────────────────────────────────────────────────────

fn too_large() -> AppError {
    AppError::bad_request("Amount is too large")
}

/// `n / d` rounded half-up (n ≥ 0, d > 0).
fn div_half_up(n: i128, d: i128) -> i128 {
    (2 * n + d) / (2 * d)
}

fn money(cents: i128) -> AppResult<Money> {
    i64::try_from(cents).map(Money::from_cents).map_err(|_| too_large())
}

/// `amount × rate (ppm)`, rounded half-up to the cent.
fn at_rate(amount: Money, rate: i64) -> AppResult<Money> {
    money(div_half_up(i128::from(amount.cents().max(0)) * i128::from(rate.max(0)), i128::from(PPM)))
}

fn sub(a: Money, b: Money) -> AppResult<Money> {
    a.checked_sub(b).ok_or_else(too_large)
}

fn add(items: &[Money]) -> AppResult<Money> {
    items.iter().try_fold(Money::ZERO, |acc, m| acc.checked_add(*m).ok_or_else(too_large))
}

/// The part of `wages` that falls under a yearly limit, given what was already paid this year.
fn under_limit(wages: Money, paid: Money, limit: i64) -> Money {
    let room = (limit - paid.cents()).max(0);
    Money::from_cents(wages.cents().clamp(0, room))
}

/// Worksheet 1A step 2: the annual tentative withholding for an Adjusted Annual Wage Amount, as
/// a numerator over 100 (cents × 100), exact.
fn annual_tentative_x100(adjusted: i128, table: &[Bracket]) -> i128 {
    let row = table.iter().rev().find(|r| adjusted >= i128::from(r.over)).copied().unwrap_or(b(0, 0, 0));
    i128::from(row.base) * 100 + (adjusted - i128::from(row.over)) * i128::from(row.percent)
}

/// Federal income tax to withhold this pay period, per Worksheet 1A, before any optional rounding
/// to the dollar: returned exactly as (numerator, denominator) in cents, and rounded to the cent.
fn federal_withholding_exact(wages: Money, periods: i64, w4: &W4, t: &UsTables) -> (i128, i128) {
    let periods = i128::from(periods);
    let annual = i128::from(wages.cents().max(0)) * periods; // 1c
    let (adjusted, table, credits, extra) = match w4 {
        W4::Since2020 { status, step2_checkbox, credits, other_income, deductions, extra } => {
            let std = match (step2_checkbox, status) {
                (true, _) => 0,
                (false, FilingStatus::MarriedJointly) => t.std_mfj,
                (false, _) => t.std_other,
            };
            let adjusted = annual + i128::from(other_income.cents().max(0))
                - i128::from(deductions.cents().max(0))
                - i128::from(std); // 1e − 1h
            let pair = match status {
                FilingStatus::MarriedJointly => t.mfj,
                FilingStatus::Single => t.single,
                FilingStatus::HeadOfHousehold => t.hoh,
            };
            (adjusted.max(0), pair[usize::from(*step2_checkbox)], credits.cents().max(0), extra.cents().max(0))
        }
        W4::Before2020 { married, allowances, extra } => {
            let adjusted = annual - i128::from(*allowances).max(0) * i128::from(t.allowance); // 1l
            let table = if *married { t.mfj[0] } else { t.single[0] };
            (adjusted.max(0), table, 0, extra.cents().max(0))
        }
    };
    // 2h − 3b, floored at zero (3c), plus 4a — kept over one denominator: periods × 100.
    let tentative = annual_tentative_x100(adjusted, table);
    let net = (tentative - i128::from(credits) * 100).max(0);
    (net + i128::from(extra) * periods * 100, periods * 100)
}

/// Federal income tax withholding for one pay period, rounded half-up to the cent.
pub fn federal_income_tax(wages: Money, periods: i64, w4: &W4, year: i32) -> AppResult<Money> {
    if periods <= 0 {
        return Err(AppError::bad_request("Pay periods per year must be positive"));
    }
    let (n, d) = federal_withholding_exact(wages, periods, w4, tables(year)?);
    money(div_half_up(n, d))
}

/// Every US payroll tax for one employee's pay period.
pub fn compute(i: &UsInputs) -> AppResult<UsFigures> {
    let t = tables(i.year)?;
    if !matches!(i.periods_per_year, 2 | 4 | 12 | 24 | 26 | 52 | 260) {
        return Err(AppError::bad_request("Pay periods per year must be 260, 52, 26, 24, 12, 4 or 2"));
    }
    if [i.gross, i.pretax_income_tax_only, i.pretax_all_taxes].iter().any(|m| m.cents() < 0) {
        return Err(AppError::bad_request("Amounts must not be negative"));
    }
    let pretax = add(&[i.pretax_income_tax_only, i.pretax_all_taxes])?;
    if pretax > i.gross {
        return Err(AppError::bad_request("Pre-tax deductions are more than gross pay"));
    }

    let fica_wages = sub(i.gross, i.pretax_all_taxes)?;
    let income_tax_wages = sub(i.gross, pretax)?;
    let federal_income_tax = if i.exempt_from_withholding {
        Money::ZERO
    } else {
        let (n, d) = federal_withholding_exact(income_tax_wages, i.periods_per_year, &i.w4, t);
        money(div_half_up(n, d))?
    };

    let social_security_wages = under_limit(fica_wages, i.ytd.social_security_wages, t.social_security_wage_base);
    let social_security_employee = at_rate(social_security_wages, t.social_security_rate)?;
    let social_security_employer = social_security_employee;

    let medicare_wages = fica_wages;
    let medicare_employee = at_rate(medicare_wages, t.medicare_rate)?;
    let medicare_employer = medicare_employee;
    // Only the wages that take the year past the threshold, and everything after it.
    let paid = i.ytd.medicare_wages.cents().max(0);
    let over =
        (paid + medicare_wages.cents() - t.additional_medicare_threshold.max(paid)).clamp(0, medicare_wages.cents());
    let additional_medicare = at_rate(Money::from_cents(over), t.additional_medicare_rate)?;

    let futa_wages = under_limit(fica_wages, i.ytd.futa_wages, t.futa_wage_base);
    let futa_rate = t.futa_rate - t.futa_max_credit + i.futa_credit_reduction.max(0);
    let futa = at_rate(futa_wages, futa_rate)?;

    let suta_wages = match i.suta_wage_base {
        Some(base) => under_limit(fica_wages, i.ytd.suta_wages, base.cents()),
        None => fica_wages,
    };
    let suta = at_rate(suta_wages, i.suta_rate)?;

    let employee_taxes = add(&[federal_income_tax, social_security_employee, medicare_employee, additional_medicare])?;
    let net_pay = sub(sub(i.gross, pretax)?, employee_taxes)?;
    let employer_taxes = add(&[social_security_employer, medicare_employer, futa, suta])?;
    let cost_to_employer = add(&[i.gross, employer_taxes])?;

    Ok(UsFigures {
        income_tax_wages,
        federal_income_tax,
        social_security_wages,
        social_security_employee,
        social_security_employer,
        medicare_wages,
        medicare_employee,
        additional_medicare,
        medicare_employer,
        futa_wages,
        futa,
        suta_wages,
        suta,
        employee_taxes,
        net_pay,
        employer_taxes,
        cost_to_employer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn usd(dollars: &str) -> Money {
        Money::parse_decimal(dollars).unwrap_or_else(|| panic!("not money: {dollars}"))
    }

    fn w4(status: FilingStatus, step2_checkbox: bool) -> W4 {
        W4::Since2020 {
            status,
            step2_checkbox,
            credits: Money::ZERO,
            other_income: Money::ZERO,
            deductions: Money::ZERO,
            extra: Money::ZERO,
        }
    }

    fn inputs(gross: &str, periods: i64, w4: W4) -> UsInputs {
        UsInputs {
            year: 2026,
            periods_per_year: periods,
            gross: usd(gross),
            pretax_income_tax_only: Money::ZERO,
            pretax_all_taxes: Money::ZERO,
            w4,
            exempt_from_withholding: false,
            ytd: YearToDate::default(),
            suta_rate: 0,
            suta_wage_base: None,
            futa_credit_reduction: 0,
        }
    }

    /// Every row of the IRS 2026 wage bracket tables: the table is the percentage method at the
    /// middle of each bracket, rounded to the dollar (to the dime for the daily table).
    #[test]
    fn matches_every_irs_2026_wage_bracket_row() {
        let fx: Value =
            serde_json::from_str(include_str!("../tests/fixtures/us_2026_wage_brackets.json")).unwrap_or(Value::Null);
        let rows = fx["rows"].as_array().cloned().unwrap_or_default();
        assert!(rows.len() > 700, "fixture not loaded");
        let cols = [
            (FilingStatus::MarriedJointly, false),
            (FilingStatus::MarriedJointly, true),
            (FilingStatus::HeadOfHousehold, false),
            (FilingStatus::HeadOfHousehold, true),
            (FilingStatus::Single, false),
            (FilingStatus::Single, true),
        ];
        let t = tables(2026).unwrap_or_else(|e| panic!("{e:?}"));
        let mut checked = 0;
        for r in rows {
            let periods = r["periods"].as_i64().unwrap_or(0);
            let from = usd(r["from"].as_str().unwrap_or(""));
            let to = usd(r["to"].as_str().unwrap_or(""));
            let mid = Money::from_cents((from.cents() + to.cents()) / 2);
            // Cents per printed step: a dime for daily, a dollar otherwise.
            let step: i128 = if periods == 260 { 10 } else { 100 };
            for ((status, chk), want) in cols.iter().zip(r["withholding"].as_array().cloned().unwrap_or_default()) {
                let (n, d) = federal_withholding_exact(mid, periods, &w4(*status, *chk), t);
                let got = div_half_up(n, d * step) * step;
                let want = i128::from(usd(want.as_str().unwrap_or("")).cents());
                assert_eq!(got, want, "{periods} periods, wages {mid}, {status:?}, checkbox {chk}");
                checked += 1;
            }
        }
        assert_eq!(checked, 4794);
    }

    #[test]
    fn w4_steps_3_and_4_follow_worksheet_1a() -> AppResult<()> {
        // Biweekly $3,000, single, $2,000 credits, $5,200 other income, $2,600 deductions, $20 extra.
        // 1c 78,000; 1e 83,200; 1h 2,600 + 8,600 = 11,200; 1i 72,000.
        // 2g 5,800 + 22% × (72,000 − 57,900) = 8,902; 2h 342.38461…; 3b 76.92307…; 3c 265.4615…; +20.
        let w = W4::Since2020 {
            status: FilingStatus::Single,
            step2_checkbox: false,
            credits: usd("2000"),
            other_income: usd("5200"),
            deductions: usd("2600"),
            extra: usd("20"),
        };
        assert_eq!(federal_income_tax(usd("3000"), 26, &w, 2026)?, usd("285.46"));
        // Credits larger than the tax leave only the extra withholding.
        let w = W4::Since2020 {
            status: FilingStatus::Single,
            step2_checkbox: false,
            credits: usd("50000"),
            other_income: Money::ZERO,
            deductions: Money::ZERO,
            extra: usd("15"),
        };
        assert_eq!(federal_income_tax(usd("3000"), 26, &w, 2026)?, usd("15"));
        Ok(())
    }

    #[test]
    fn older_w4_uses_allowances() -> AppResult<()> {
        // Monthly $5,000, married, 2 allowances: 60,000 − 8,600 = 51,400 on the MFJ standard table:
        // 2,480 + 12% × (51,400 − 44,100) = 3,356 a year → 279.67 a month.
        let w = W4::Before2020 { married: true, allowances: 2, extra: Money::ZERO };
        assert_eq!(federal_income_tax(usd("5000"), 12, &w, 2026)?, usd("279.67"));
        Ok(())
    }

    #[test]
    fn fica_futa_and_suta_on_an_ordinary_paycheck() -> AppResult<()> {
        let mut i = inputs("2000", 26, w4(FilingStatus::Single, false));
        i.suta_rate = 27_000; // 2.7%
        i.suta_wage_base = Some(usd("9000"));
        let f = compute(&i)?;
        assert_eq!(f.social_security_employee, usd("124"));
        assert_eq!(f.social_security_employer, usd("124"));
        assert_eq!(f.medicare_employee, usd("29"));
        assert_eq!(f.additional_medicare, Money::ZERO);
        assert_eq!(f.futa, usd("12")); // 0.6%
        assert_eq!(f.suta, usd("54"));
        // 52,000 − 8,600 = 43,400 → 1,240 + 12% × 23,500 = 4,060 → 156.15 per period.
        assert_eq!(f.federal_income_tax, usd("156.15"));
        assert_eq!(f.net_pay, usd("1690.85"));
        assert_eq!(f.cost_to_employer, usd("2219"));
        Ok(())
    }

    #[test]
    fn wage_bases_stop_at_the_limits() -> AppResult<()> {
        let mut i = inputs("10000", 12, w4(FilingStatus::MarriedJointly, false));
        i.ytd = YearToDate {
            social_security_wages: usd("180000"),
            medicare_wages: usd("195000"),
            futa_wages: usd("6000"),
            suta_wages: usd("9000"),
        };
        i.suta_rate = 27_000;
        i.suta_wage_base = Some(usd("9000"));
        let f = compute(&i)?;
        assert_eq!(f.social_security_wages, usd("4500")); // 184,500 − 180,000
        assert_eq!(f.social_security_employee, usd("279"));
        assert_eq!(f.medicare_employee, usd("145"));
        assert_eq!(f.additional_medicare, usd("45")); // 0.9% of the 5,000 over 200,000
        assert_eq!(f.medicare_employer, usd("145")); // no employer share of the extra 0.9%
        assert_eq!(f.futa_wages, usd("1000"));
        assert_eq!(f.futa, usd("6"));
        assert_eq!(f.suta, Money::ZERO);
        Ok(())
    }

    #[test]
    fn pretax_deductions_and_exemption() -> AppResult<()> {
        let mut i = inputs("4000", 24, w4(FilingStatus::Single, false));
        i.pretax_income_tax_only = usd("400"); // 401(k): still FICA wages
        i.pretax_all_taxes = usd("100"); // section 125
        i.exempt_from_withholding = true;
        let f = compute(&i)?;
        assert_eq!(f.income_tax_wages, usd("3500"));
        assert_eq!(f.medicare_wages, usd("3900"));
        assert_eq!(f.federal_income_tax, Money::ZERO);
        assert_eq!(f.net_pay, usd("3201.65")); // 3,500 − 241.80 − 56.55
        Ok(())
    }

    #[test]
    fn credit_reduction_raises_futa() -> AppResult<()> {
        let mut i = inputs("1000", 52, w4(FilingStatus::Single, false));
        i.futa_credit_reduction = 3_000; // 0.3%
        assert_eq!(compute(&i)?.futa, usd("9"));
        Ok(())
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(compute(&UsInputs { year: 2019, ..inputs("100", 52, w4(FilingStatus::Single, false)) }).is_err());
        assert!(compute(&inputs("100", 13, w4(FilingStatus::Single, false))).is_err());
        let mut i = inputs("100", 52, w4(FilingStatus::Single, false));
        i.pretax_all_taxes = usd("150");
        assert!(compute(&i).is_err());
    }
}
