//! D66, step 3 of the US state payroll plan: states whose 2026 withholding method is not one
//! rate or one bracket table (their own deductions that phase out, federal tax subtracted,
//! credits and recaptures, rate tables chosen by the employee). Each state's method is worked
//! out here exactly as its own 2026 publication writes it; the built-in rule in
//! [`crate::us_state_packs`] names the formula (`StateRule::formula`, method FORMULA).
//! Integer cents throughout; every function returns the tax for this pay period, before the
//! certificate's extra withholding.

use crate::us::PPM;
use crate::us_state::{PayContext, StateEmployee};
use app_core::money::Money;
use app_core::{AppError, AppResult};

/// This period's tax for formula `id`.
pub fn compute(id: &str, wages: Money, periods: i64, e: &StateEmployee, pay: &PayContext) -> AppResult<Money> {
    let annual_wages = i128::from(wages.cents().max(0)) * i128::from(periods);
    let p = i128::from(periods);
    let annual = match id {
        "AL-2026" => alabama(annual_wages, e, i128::from(pay.federal_income_tax.cents().max(0)) * p),
        "AR-2026" => arkansas(annual_wages, e),
        "CA-2026" => california(i128::from(wages.cents().max(0)), periods, e),
        "MD-2026" => maryland(i128::from(wages.cents().max(0)), periods, e),
        "ME-2026" => {
            // Maine rounds this period's tax to the whole dollar.
            let cents = per_period(maine(annual_wages, e), periods)?.cents();
            return Ok(Money::from_cents((cents + 50) / 100 * 100));
        }
        "NY-2026" => return new_york(i128::from(wages.cents().max(0)), periods, e),
        "NJ-2026" => return new_jersey(i128::from(wages.cents().max(0)), periods, e),
        "WI-2026" => wisconsin(annual_wages, e),
        "CT-2026" => connecticut(annual_wages, e),
        _ => return Err(AppError::bad_request(format!("Unknown state withholding formula \"{id}\""))),
    };
    per_period(annual, periods)
}

/// Index of a standard pay frequency in the per-period tables: weekly, biweekly, semimonthly,
/// monthly.
fn frequency(periods: i64) -> Option<usize> {
    match periods {
        52 => Some(0),
        26 => Some(1),
        24 => Some(2),
        12 => Some(3),
        _ => None,
    }
}

/// `(upper bound in cents, None = no limit; rate in ppm)`.
type Rows = [(Option<i128>, i128)];

/// Annual tax on `taxable` cents through progressive brackets, in cents × ppm.
fn brackets(taxable: i128, rows: &Rows) -> i128 {
    let mut tax = 0;
    let mut lower = 0;
    for (upper, rate) in rows {
        if taxable <= lower {
            break;
        }
        let top = upper.map_or(taxable, |u| u.min(taxable));
        tax += (top - lower) * rate;
        match upper {
            Some(u) => lower = *u,
            None => break,
        }
    }
    tax
}

/// Annual tax (cents × ppm) ÷ pay periods, half-up to the cent.
fn per_period(annual: i128, periods: i64) -> AppResult<Money> {
    let d = i128::from(PPM) * i128::from(periods);
    let cents = (2 * annual.max(0) + d) / (2 * d);
    i64::try_from(cents).map(Money::from_cents).map_err(|_| AppError::bad_request("Amount is too large"))
}

/// Half-up division of non-negative numbers.
fn div_half_up(n: i128, d: i128) -> i128 {
    (2 * n + d) / (2 * d)
}

fn ceil_div(n: i128, d: i128) -> i128 {
    (n + d - 1) / d
}

const fn usd(dollars: i128) -> i128 {
    dollars * 100
}

/// Alabama, "Withholding Tax Tables and Instructions for Employers and Withholding Agents"
/// (Revised January 2026), formula p.7, standard deduction schedules p.8. Form A4: status "0"
/// (no personal exemption), "S", "MS" (married filing separately), "M", "H"; line 4
/// dependents (`allowances`). Without a status code: married → M, head of household → H, else S.
///   GI = annual wages; taxable = GI − standard deduction (phased out by GI) − annual federal
///   income tax withheld − personal exemption − dependents; 2% / 4% / 5% brackets.
fn alabama(gi: i128, e: &StateEmployee, federal: i128) -> i128 {
    let status = match e.code.as_deref() {
        Some(c @ ("0" | "S" | "MS" | "M" | "H")) => c,
        _ if e.married => "M",
        _ if e.head_of_household => "H",
        _ => "S",
    };
    // (full deduction, phase-out starts above, step size, step amount, floor)
    let (full, above, step, cut, floor) = match status {
        "MS" => (usd(4_250), usd(12_999), usd(250), usd(88), usd(2_500)),
        "M" => (usd(8_500), usd(25_999), usd(500), usd(175), usd(5_000)),
        "H" => (usd(5_200), usd(25_999), usd(500), usd(135), usd(2_500)),
        _ => (usd(3_000), usd(25_999), usd(500), usd(25), usd(2_500)),
    };
    let steps = if gi > above { ceil_div(gi - above, step) } else { 0 };
    let standard = (full - steps * cut).max(floor);
    let personal = match status {
        "0" => 0,
        "S" | "MS" => usd(1_500),
        _ => usd(3_000),
    };
    let per_dependent = if gi <= usd(50_000) {
        usd(1_000)
    } else if gi <= usd(100_000) {
        usd(500)
    } else {
        usd(300)
    };
    let dependents = i128::from(e.allowances.max(0)) * per_dependent;
    let taxable = (gi - standard - federal - personal - dependents).max(0);
    let rows: &Rows = if status == "M" {
        &[(Some(usd(1_000)), 20_000), (Some(usd(6_000)), 40_000), (None, 50_000)]
    } else {
        &[(Some(usd(500)), 20_000), (Some(usd(3_000)), 40_000), (None, 50_000)]
    };
    brackets(taxable, rows)
}

/// Arkansas, "Withholding Tax Formula Method, Effective 01/01/2026" (revised for H.B. 1001,
/// 3.7% top rate retroactive to 1 January 2026), pp.2–4. AR4EC line 3 total exemptions
/// (`allowances`); the low-income tables (line 5) are not offered.
///   net taxable = annual wages − $2,470; below $100,001 it is taken at the middle of its $100
///   band (truncated to $100 + $50), from $100,001 in whole dollars; annual tax = rate × net
///   taxable − the band's adjustment, rounded to the dollar; less $29 per exemption.
fn arkansas(annual: i128, e: &StateEmployee) -> i128 {
    let net = (annual - usd(2_470)).max(0);
    let dollars = net / 100;
    let income = if dollars < 100_001 { dollars / 100 * 100 + 50 } else { dollars };
    // (rate in ppm, adjustment in cents)
    let (rate, minus): (i128, i128) = match dollars {
        0..=5_599 => (0, 0),
        5_600..=11_199 => (20_000, 11_198),
        11_200..=15_999 => (30_000, 22_397),
        16_000..=26_399 => (34_000, 28_797),
        26_400..=94_700 => (37_000, 36_716),
        // $100 bands with an adjustment $10 smaller each, from 369.90 to 89.90.
        94_701..=97_600 => (37_000, 36_990 - (dollars - 94_701) / 100 * 1_000),
        _ => (37_000, 7_990),
    };
    let tax_cents_ppm = (usd(income) * rate - minus * i128::from(PPM)).max(0);
    let rounded_dollars = div_half_up(tax_cents_ppm, usd(1) * i128::from(PPM));
    let credit = 29 * i128::from(e.allowances.max(0));
    usd((rounded_dollars - credit).max(0)) * i128::from(PPM)
}

/// California, "California Withholding Schedules for 2026 – Method B (Exact Calculation
/// Method)" (EDD), with its annualized Examples E and F. DE 4: status Single or Married with
/// two or more incomes / Married (one income) / Head of household; line 1a regular allowances
/// (`allowances`, credited), line 1b estimated-deduction allowances (`second_allowances`,
/// $1,000 deductions, never credited).
///   1. No tax when this period's wages are at or under the low-income exemption (Table 1).
///   2. Annual wages − $1,000 × estimated-deduction allowances − the standard deduction (Table
///      3) = taxable income; 3. tax from the annual rate table (Tables 5–7); 4. less $168.30 per
///      regular allowance (Table 4), not below zero; ÷ pay periods.
///
/// The high low-income and standard-deduction column is for married with 2 or more
/// allowances (all allowances counted) and for head of household.
fn california(wages: i128, periods: i64, e: &StateEmployee) -> i128 {
    let total_allowances = e.allowances.max(0) + e.second_allowances.max(0);
    let high = e.head_of_household && !e.married || e.married && total_allowances >= 2;
    let p = i128::from(periods);
    let exemption = match (frequency(periods), high) {
        (Some(i), false) => usd([363, 727, 787, 1_575][i]),
        (Some(i), true) => usd([727, 1_454, 1_575, 3_149][i]),
        (None, false) => usd(18_896) / p,
        (None, true) => usd(37_791) / p,
    };
    if wages <= exemption {
        return 0;
    }
    let standard = if high { usd(11_412) } else { usd(5_706) };
    let taxable = (wages * p - i128::from(e.second_allowances.max(0)) * usd(1_000) - standard).max(0);
    let rates = [11_000, 22_000, 44_000, 66_000, 88_000, 102_300, 113_300, 124_300, 135_300, 146_300];
    let uppers: [i128; 9] = if e.married {
        [22_158, 52_528, 82_904, 115_084, 145_448, 742_958, 891_542, 1_000_000, 1_485_906]
    } else if e.head_of_household {
        [22_173, 52_530, 67_716, 83_805, 98_990, 505_208, 606_251, 1_000_000, 1_010_417]
    } else {
        [11_079, 26_264, 41_452, 57_542, 72_724, 371_479, 445_771, 742_953, 1_000_000]
    };
    let rows: Vec<(Option<i128>, i128)> =
        rates.iter().enumerate().map(|(i, r)| (uppers.get(i).map(|u| usd(*u)), *r)).collect();
    let credit = i128::from(e.allowances.max(0)) * 16_830;
    (brackets(taxable, &rows) - credit * i128::from(PPM)).max(0)
}

/// Maryland local income tax: county code → (chart rate in ppm, or progressive rates for single
/// and joint). The Employer Withholding Guide (Revised December 2025) withholds county tax with
/// the state tax using the chart whose rate equals or slightly exceeds the county's (Carroll and
/// Charles 3.03% → 3.05%, Cecil 2.74% → 2.75%, Harford 3.06% → 3.10%, Washington 2.95% → 3.00%);
/// 2026 county rates from the Comptroller's 2026 state and local withholding memo. Anne Arundel
/// and Frederick have progressive rates and no chart; they are applied as enacted.
fn maryland_local(code: Option<&str>, joint: bool) -> Vec<(Option<i128>, i128)> {
    let flat = |rate: i128| vec![(None, rate)];
    match code.unwrap_or("") {
        "ALLE" | "BALT" | "BCITY" | "CALV" | "CARO" | "HOWA" | "MONT" | "PRIN" | "QUEE" | "STMA" | "SOME" | "WICO" => {
            flat(32_000)
        }
        "CARR" | "CHAR" => flat(30_500),
        "CECI" => flat(27_500),
        "GARR" => flat(26_500),
        "HARF" => flat(31_000),
        "TALB" => flat(24_000),
        "WASH" => flat(30_000),
        // Worcester, and the special nonresident rate for someone living outside Maryland.
        "WORC" | "NR" => flat(22_500),
        "ANNE" if joint => vec![(Some(usd(75_000)), 27_000), (Some(usd(480_000)), 29_400), (None, 32_000)],
        "ANNE" => vec![(Some(usd(50_000)), 27_000), (Some(usd(400_000)), 29_400), (None, 32_000)],
        "FRED" if joint => {
            vec![
                (Some(usd(25_000)), 22_500),
                (Some(usd(100_000)), 27_500),
                (Some(usd(250_000)), 29_600),
                (None, 32_000),
            ]
        }
        "FRED" => {
            vec![(Some(usd(25_000)), 22_500), (Some(usd(50_000)), 27_500), (Some(usd(150_000)), 29_600), (None, 32_000)]
        }
        // Dorchester, Kent, and a county not given (the highest local rate).
        _ => flat(33_000),
    }
}

/// Maryland, Employer Withholding Guide effective January 2026 (Revised December 2025), the
/// percentage formula on pp.5–6 and the annual charts: taxable = wages − $3,400 − $3,200 per
/// MW507 exemption; state rates from 4.75% (the lower brackets are not used for withholding)
/// plus the county rate; married filing jointly and head of household use the joint schedule.
/// Nothing is withheld on wages under $5,000 a year ($96 a week, …). County: `code`.
fn maryland(wages: i128, periods: i64, e: &StateEmployee) -> i128 {
    let floor = match frequency(periods) {
        Some(i) => usd([96, 192, 208, 417][i]),
        None => usd(5_000) / i128::from(periods),
    };
    if wages < floor {
        return 0;
    }
    let joint = e.married || e.head_of_household;
    let taxable = (wages * i128::from(periods) - usd(3_400) - i128::from(e.allowances.max(0)) * usd(3_200)).max(0);
    let state: Vec<(Option<i128>, i128)> = if joint {
        vec![
            (Some(usd(150_000)), 47_500),
            (Some(usd(175_000)), 50_000),
            (Some(usd(225_000)), 52_500),
            (Some(usd(300_000)), 55_000),
            (Some(usd(600_000)), 57_500),
            (Some(usd(1_200_000)), 62_500),
            (None, 65_000),
        ]
    } else {
        vec![
            (Some(usd(100_000)), 47_500),
            (Some(usd(125_000)), 50_000),
            (Some(usd(150_000)), 52_500),
            (Some(usd(250_000)), 55_000),
            (Some(usd(500_000)), 57_500),
            (Some(usd(1_000_000)), 62_500),
            (None, 65_000),
        ]
    };
    brackets(taxable, &state) + brackets(taxable, &maryland_local(e.code.as_deref(), joint))
}

/// Maine, "2026 Withholding Tables for Individual Income Tax" (Revised August 2026, increased
/// standard deduction and the 2% surcharge, for 2026 from 1 January), percentage method p.6:
/// annual wages − $5,300 per W-4ME allowance − the standard deduction ($12,850 single, $28,550
/// married, phased out between $102,250–$177,250 / $204,550–$354,550 of annual wages); 5.8% /
/// 6.75% / 7.15%, plus 2% of annual income over $1,000,000 / $1,500,000. Head of household uses
/// the single schedule. Rounded to the whole dollar per period by the caller.
fn maine(annual: i128, e: &StateEmployee) -> i128 {
    let (full, from, to) =
        if e.married { (usd(28_550), usd(204_550), usd(354_550)) } else { (usd(12_850), usd(102_250), usd(177_250)) };
    let standard = if annual <= from {
        full
    } else if annual >= to {
        0
    } else {
        div_half_up(full * (to - annual), to - from)
    };
    let income = (annual - i128::from(e.allowances.max(0)) * usd(5_300) - standard).max(0);
    let rows: &Rows = if e.married {
        &[(Some(usd(54_850)), 58_000), (Some(usd(129_750)), 67_500), (None, 71_500)]
    } else {
        &[(Some(usd(27_400)), 58_000), (Some(usd(64_850)), 67_500), (None, 71_500)]
    };
    let surcharge_over = if e.married { usd(1_500_000) } else { usd(1_000_000) };
    brackets(income, rows) + (income - surcharge_over).max(0) * 20_000
}

/// New York State, NYS-50-T-NYS (1/26), Method II (exact calculation) with its per-period
/// tables II-A to II-D (pp.17 and 19) and Method III (p.22). IT-2104: Single or head of
/// household / Married / Married but withhold at the higher single rate (= single).
///   net wages = wages − deduction allowance (Table B) − exemptions × Table C;
///   tax = (net − column 3) × column 4, half-up to the cent, + column 5 (as printed);
///   annualized net wages at or over $1,077,550 single / $2,155,350 married: Method III,
///   net × the top rate. Weekly, biweekly, semimonthly and monthly payrolls.
fn new_york(wages: i128, periods: i64, e: &StateEmployee) -> AppResult<Money> {
    let Some(f) = frequency(periods) else {
        return Err(AppError::bad_request(
            "New York withholding needs a weekly, biweekly, semimonthly or monthly payroll",
        ));
    };
    let deduction = if e.married { [15_290, 30_580, 33_125, 66_250][f] } else { [14_230, 28_460, 30_835, 61_670][f] };
    let exemption = [1_925, 3_850, 4_165, 8_330][f];
    let net = wages - deduction - i128::from(e.allowances.max(0)) * exemption;
    if net <= 0 {
        return Ok(Money::ZERO);
    }
    let annualized = net * i128::from(periods);
    let top = if e.married { usd(2_155_350) } else { usd(1_077_550) };
    let tax = if annualized >= top {
        let rate = if annualized < usd(5_000_000) {
            104_500
        } else if annualized < usd(25_000_000) {
            111_000
        } else {
            117_000
        };
        div_half_up(net * rate, i128::from(PPM))
    } else {
        let (col3, rates, col5): (&[i128], &[i128], &[i128]) = if e.married {
            (
                &[
                    [0, 163, 225, 267, 1_551, 1_862, 2_070, 3_032, 4_068, 6_215, 7_177, 20_722],
                    [0, 327, 450, 535, 3_102, 3_723, 4_140, 6_063, 8_137, 12_431, 14_354, 41_444],
                    [0, 354, 488, 579, 3_360, 4_033, 4_485, 6_569, 8_815, 13_467, 15_550, 44_898],
                    [0, 708, 975, 1_158, 6_721, 8_067, 8_971, 13_138, 17_629, 26_933, 31_100, 89_796],
                ][f],
                &[39_000, 44_000, 51_500, 54_000, 59_000, 65_700, 70_700, 80_100, 64_000, 134_900, 73_500, 76_500],
                &[
                    [0, 638, 908, 1_127, 8_058, 9_890, 11_260, 18_054, 26_362, 40_104, 53_077, 152_633],
                    [0, 1_277, 1_815, 2_254, 16_115, 19_781, 22_519, 36_108, 52_723, 80_208, 106_154, 305_265],
                    [0, 1_383, 1_967, 2_442, 17_458, 21_429, 24_396, 39_117, 57_117, 86_892, 115_000, 330_704],
                    [0, 2_767, 3_933, 4_883, 34_917, 42_858, 48_792, 78_233, 114_233, 173_783, 230_000, 661_408],
                ][f],
            )
        } else {
            (
                &[
                    [0, 163, 225, 267, 1_551, 1_862, 2_070, 3_032, 4_142, 5_104],
                    [0, 327, 450, 535, 3_102, 3_723, 4_140, 6_063, 8_285, 10_208],
                    [0, 354, 488, 579, 3_360, 4_033, 4_485, 6_569, 8_975, 11_058],
                    [0, 708, 975, 1_158, 6_721, 8_067, 8_971, 13_138, 17_950, 22_117],
                ][f],
                &[39_000, 44_000, 51_500, 54_000, 59_000, 70_300, 75_300, 64_000, 114_400, 73_500],
                &[
                    [0, 638, 908, 1_127, 8_058, 9_890, 11_358, 18_602, 25_710, 36_713],
                    [0, 1_277, 1_815, 2_254, 16_115, 19_781, 22_715, 37_204, 51_419, 73_427],
                    [0, 1_383, 1_967, 2_442, 17_458, 21_429, 24_608, 40_304, 55_704, 79_546],
                    [0, 2_767, 3_933, 4_883, 34_917, 42_858, 49_217, 80_608, 111_408, 159_092],
                ][f],
            )
        };
        let line = col3.iter().rposition(|c| net >= usd(*c)).unwrap_or(0);
        col5[line] + div_half_up((net - usd(col3[line])) * rates[line], i128::from(PPM))
    };
    i64::try_from(tax).map(Money::from_cents).map_err(|_| AppError::bad_request("Amount is too large"))
}

/// Wisconsin, Publication W-166 (1/26), "Alternate Method of Withholding Wisconsin Income Tax"
/// pp.26–27 (rates unchanged for 2026, Withholding Tax Update 2026-1): annual wages less a
/// deduction that phases out (single $6,702 − 12% of wages over $17,780, none from $73,630;
/// married $9,461 − 20% over $25,727, none from $73,032), less $400 per WT-4 exemption; one
/// schedule 3.54% / 4.65% / 5.30% / 7.65%. Head of household uses the single deduction.
fn wisconsin(annual: i128, e: &StateEmployee) -> i128 {
    // Deduction in cents × 100 (12% / 20% of cents).
    let deduction = if e.married {
        if annual < usd(25_727) {
            usd(9_461) * 100
        } else if annual < usd(73_032) {
            usd(9_461) * 100 - 20 * (annual - usd(25_727))
        } else {
            0
        }
    } else if annual < usd(17_780) {
        usd(6_702) * 100
    } else if annual < usd(73_630) {
        usd(6_702) * 100 - 12 * (annual - usd(17_780))
    } else {
        0
    };
    let net = (annual * 100 - deduction.max(0) - i128::from(e.allowances.max(0)) * usd(400) * 100).max(0);
    let rows: &Rows = &[
        (Some(usd(12_760) * 100), 35_400),
        (Some(usd(25_520) * 100), 46_500),
        (Some(usd(280_950) * 100), 53_000),
        (None, 76_500),
    ];
    brackets(net, rows) / 100
}

/// Connecticut, TPG-211 "2026 Withholding Calculation Rules" (Rev. 12/25, unchanged from 2025)
/// Tables A–E: S = annual wages; T = S − personal exemption (Table A); tax = initial tax on T
/// (Table B) + 2% phase-out add-back (Table C) + recapture (Table D), × (1 − personal credit
/// decimal, Table E), ÷ pay periods. CT-W4 line 1 withholding code A, B, C, D, F (`code`); E =
/// no withholding; no CT-W4 on file = 6.99% of wages with no exemption (IP 2026(1) p.11). Every
/// band is "more than X, up to and including Y". The state's wage-bracket tables apply this
/// formula to each wage row's midpoint.
fn connecticut(annual: i128, e: &StateEmployee) -> i128 {
    let code = match e.code.as_deref() {
        Some(c @ ("A" | "B" | "C" | "D" | "E" | "F")) => c,
        _ => return annual * 69_900,
    };
    if code == "E" {
        return 0;
    }
    let steps = |over: i128, size: i128| if annual > usd(over) { ceil_div(annual - usd(over), usd(size)) } else { 0 };
    // Table A.
    let exemption = match code {
        "A" => usd(12_000) - steps(24_000, 1_000) * usd(1_000),
        "B" => usd(19_000) - steps(38_000, 1_000) * usd(1_000),
        "C" => usd(24_000) - steps(48_000, 1_000) * usd(1_000),
        "F" => usd(15_000) - steps(30_000, 1_000) * usd(1_000),
        _ => 0,
    }
    .max(0);
    let taxable = (annual - exemption).max(0);
    if taxable == 0 {
        return 0;
    }
    // Table B (scale: A / D / F 1, B 1.6, C 2 of the A brackets).
    let bounds: [i128; 6] = match code {
        "B" => [16_000, 80_000, 160_000, 320_000, 400_000, 800_000],
        "C" => [20_000, 100_000, 200_000, 400_000, 500_000, 1_000_000],
        _ => [10_000, 50_000, 100_000, 200_000, 250_000, 500_000],
    };
    let rates = [20_000, 45_000, 55_000, 60_000, 65_000, 69_000, 69_900];
    let rows: Vec<(Option<i128>, i128)> =
        rates.iter().enumerate().map(|(i, r)| (bounds.get(i).map(|b| usd(*b)), *r)).collect();
    let initial = brackets(taxable, &rows);
    // Table C.
    let add_back = match code {
        "B" => (40 * steps(78_500, 4_000)).min(400),
        "C" => (50 * steps(100_500, 5_000)).min(500),
        "F" => (25 * steps(56_500, 5_000)).min(250),
        _ => (25 * steps(50_250, 2_500)).min(250),
    };
    // Table D: three rising steps with plateaus.
    let (k, step) = match code {
        "B" => (8, 8_000),
        "C" => (10, 10_000),
        _ => (5, 5_000),
    };
    let t = |x: i128| x * k / 5; // thresholds of the A table scaled for B (×1.6) and C (×2)
    let recapture = if annual <= usd(t(105_000)) {
        0
    } else if annual <= usd(t(200_000)) {
        (k * 5 * steps(t(105_000), step)).min(k * 50)
    } else if annual <= usd(t(500_000)) {
        let (first, each, cap) = match code {
            "B" => (540, 140, 4_600),
            "C" => (680, 180, 5_900),
            _ => (340, 90, 2_950),
        };
        (first + each * (steps(t(200_000), step) - 1)).min(cap)
    } else {
        let (first, each, cap) = match code {
            "B" => (4_680, 80, 5_320),
            "C" => (6_000, 100, 6_800),
            _ => (3_000, 50, 3_400),
        };
        (first + each * (steps(t(500_000), step) - 1)).min(cap)
    };
    // Table E, in hundredths.
    let decimal = if code == "D" {
        0
    } else {
        let b: [i128; 8] = match code {
            "A" => [12_000, 15_000, 18_500, 20_000, 21_500, 25_000, 27_000, 48_000],
            "B" => [19_000, 24_000, 27_500, 34_000, 35_500, 44_000, 46_000, 74_000],
            "C" => [24_000, 30_000, 33_500, 40_000, 41_500, 50_000, 52_000, 96_000],
            _ => [15_000, 18_800, 22_300, 25_000, 26_500, 31_300, 33_300, 60_000],
        };
        let band = |from: i128| ceil_div(annual - usd(from), usd(500)) - 1;
        let s = annual;
        if s <= usd(b[1]) {
            75
        } else if s <= usd(b[2]) {
            70 - 5 * band(b[1])
        } else if s <= usd(b[3]) {
            35
        } else if s <= usd(b[4]) {
            30 - 5 * band(b[3])
        } else if s <= usd(b[5]) {
            15
        } else if s <= usd(b[6]) {
            14 - band(b[5])
        } else if s <= usd(b[7]) {
            10
        } else if s <= usd(b[7] + 4_500) {
            9 - band(b[7])
        } else {
            0
        }
    };
    (initial + usd(add_back + recapture) * i128::from(PPM)) * (100 - decimal) / 100
}

/// New Jersey rate tables A–E for one pay frequency: `(lower bound in dollars, base in cents)`
/// per row, with the table's rates.
fn nj_table(letter: &str, f: usize) -> (&'static [i128], &'static [i128], &'static [i128]) {
    const RA: &[i128] = &[15_000, 20_000, 39_000, 61_000, 70_000, 99_000, 118_000];
    const RB: &[i128] = &[15_000, 20_000, 27_000, 39_000, 61_000, 70_000, 99_000, 118_000];
    const RC: &[i128] = &[15_000, 23_000, 28_000, 35_000, 56_000, 66_000, 99_000, 118_000];
    const RD: &[i128] = &[15_000, 27_000, 34_000, 43_000, 56_000, 65_000, 99_000, 118_000];
    const RE: &[i128] = &[15_000, 20_000, 58_000, 65_000, 99_000, 118_000];
    const LA: [&[i128]; 4] = [
        &[0, 385, 673, 769, 1_442, 9_615, 19_231],
        &[0, 769, 1_346, 1_538, 2_885, 19_231, 38_462],
        &[0, 833, 1_458, 1_667, 3_125, 20_833, 41_667],
        &[0, 1_667, 2_917, 3_333, 6_250, 41_667, 83_333],
    ];
    const BA: [&[i128]; 4] = [
        &[0, 577, 1_154, 1_529, 5_635, 62_846, 158_038],
        &[0, 1_200, 2_300, 3_100, 11_300, 125_700, 316_100],
        &[0, 1_300, 2_500, 3_300, 12_200, 136_200, 342_400],
        &[0, 2_500, 5_000, 6_600, 24_400, 272_300, 684_800],
    ];
    const LB: [&[i128]; 4] = [
        &[0, 385, 962, 1_346, 1_538, 2_885, 9_615, 19_231],
        &[0, 769, 1_923, 2_692, 3_077, 5_769, 19_231, 38_462],
        &[0, 833, 2_083, 2_917, 3_333, 6_250, 20_833, 41_667],
        &[0, 1_667, 4_167, 5_833, 6_667, 12_500, 41_667, 83_333],
    ];
    const BB: [&[i128]; 4] = [
        &[0, 577, 1_731, 2_769, 3_519, 11_731, 58_846, 154_038],
        &[0, 1_200, 3_500, 5_500, 7_000, 23_500, 117_700, 308_100],
        &[0, 1_250, 3_750, 5_999, 7_625, 25_419, 127_500, 333_800],
        &[0, 2_500, 7_500, 12_000, 15_300, 50_800, 255_000, 667_500],
    ];
    // C and D share their bounds.
    const LC: [&[i128]; 4] = [
        &[0, 385, 769, 962, 1_154, 2_885, 9_615, 19_231],
        &[0, 769, 1_538, 1_923, 2_308, 5_769, 19_231, 38_462],
        &[0, 833, 1_667, 2_083, 2_500, 6_250, 20_833, 41_667],
        &[0, 1_667, 3_333, 4_167, 5_000, 12_500, 41_667, 83_333],
    ];
    const BC: [&[i128]; 4] = [
        &[0, 577, 1_462, 2_000, 2_673, 12_365, 56_788, 151_981],
        &[0, 1_154, 2_923, 4_000, 5_346, 24_731, 113_577, 303_962],
        &[0, 1_250, 3_167, 4_333, 5_792, 26_792, 123_042, 329_292],
        &[0, 2_500, 6_333, 8_667, 11_583, 53_585, 246_083, 658_583],
    ];
    const BD: [&[i128]; 4] = [
        &[0, 577, 1_615, 2_269, 3_096, 12_788, 56_538, 151_731],
        &[0, 1_154, 3_231, 4_538, 6_192, 25_577, 113_077, 303_462],
        &[0, 1_250, 3_500, 4_917, 6_708, 27_708, 122_500, 328_750],
        &[0, 2_500, 7_000, 9_833, 13_417, 55_417, 245_000, 657_500],
    ];
    const LE: [&[i128]; 4] = [
        &[0, 385, 673, 1_923, 9_615, 19_231],
        &[0, 769, 1_346, 3_846, 19_231, 38_462],
        &[0, 833, 1_458, 4_167, 20_833, 41_667],
        &[0, 1_667, 2_917, 8_333, 41_667, 83_333],
    ];
    const BE: [&[i128]; 4] = [
        &[0, 577, 1_154, 8_404, 58_404, 153_596],
        &[0, 1_200, 2_300, 16_800, 116_800, 307_200],
        &[0, 1_300, 2_500, 18_200, 126_500, 332_800],
        &[0, 2_500, 5_000, 36_400, 253_100, 665_600],
    ];
    match letter {
        "B" => (LB[f], BB[f], RB),
        "C" => (LC[f], BC[f], RC),
        "D" => (LC[f], BD[f], RD),
        "E" => (LE[f], BE[f], RE),
        _ => (LA[f], BA[f], RA),
    }
}

/// New Jersey, NJ-WT (September 2025) p.24 and "Tables for Percentage Method of Withholding"
/// (wages paid on and after 1 October 2020, current for 2026), per pay period: wages − allowances
/// × the period's allowance value ($19.20 weekly, $38.40 biweekly, $41.60 semimonthly, $83.30
/// monthly); tax = row base + (wages − row lower bound) × rate, half-up to the cent. NJ-W4 line 3
/// rate letter (`code`) when given; otherwise Rate A for single / married filing separately and
/// Rate B for married filing jointly / head of household.
fn new_jersey(wages: i128, periods: i64, e: &StateEmployee) -> AppResult<Money> {
    let Some(f) = frequency(periods) else {
        return Err(AppError::bad_request(
            "New Jersey withholding needs a weekly, biweekly, semimonthly or monthly payroll",
        ));
    };
    let letter = match e.code.as_deref() {
        Some(c @ ("A" | "B" | "C" | "D" | "E")) => c,
        _ if e.married || e.head_of_household => "B",
        _ => "A",
    };
    let net = wages - i128::from(e.allowances.max(0)) * [1_920, 3_840, 4_160, 8_330][f];
    if net <= 0 {
        return Ok(Money::ZERO);
    }
    let (lower, base, rates) = nj_table(letter, f);
    let line = lower.iter().rposition(|l| net > usd(*l)).unwrap_or(0);
    let tax = base[line] + div_half_up((net - usd(lower[line])) * rates[line], i128::from(PPM));
    i64::try_from(tax).map(Money::from_cents).map_err(|_| AppError::bad_request("Amount is too large"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd_m(s: &str) -> Money {
        Money::parse_decimal(s).expect("amount")
    }

    fn tax(id: &str, wages: &str, periods: i64, e: &StateEmployee, federal: &str) -> Money {
        let pay = PayContext { federal_income_tax: usd_m(federal), fica: Money::ZERO };
        compute(id, usd_m(wages), periods, e, &pay).expect("tax")
    }

    #[test]
    fn alabama_examples() {
        // Booklet p.7: M-2, weekly $850, federal withholding $35.19 → 29.59.
        let m2 = StateEmployee { married: true, allowances: 2, ..Default::default() };
        assert_eq!(tax("AL-2026", "850", 52, &m2, "35.19"), usd_m("29.59"));
        // Standard deduction steps: single $26,000 → $2,975; $26,500 → $2,950; $35,500 → $2,500.
        let s = StateEmployee::default();
        // Computed: single, $26,000 a year (monthly), no federal tax: 26,000 − 2,975 − 1,500 =
        // 21,525 → 10 + 100 + 5% × 18,525 = 1,036.25 → monthly 86.35.
        assert_eq!(tax("AL-2026", "2166.67", 12, &s, "0"), usd_m("86.35"));
        // "0" (no exemption) and MS (married filing separately) use the single brackets.
        let zero = StateEmployee { code: Some("0".into()), ..Default::default() };
        assert!(tax("AL-2026", "2000", 26, &zero, "100") > tax("AL-2026", "2000", 26, &s, "100"));
    }

    #[test]
    fn arkansas_examples() {
        // Formula p.4: monthly $2,127, 2 exemptions → 36.50.
        let two = StateEmployee { allowances: 2, ..Default::default() };
        assert_eq!(tax("AR-2026", "2127", 12, &two, "0"), usd_m("36.50"));
        // Computed: $120,000 a year, 1 exemption: 117,530 × 3.7% − 79.90 = 4,268.71 → $4,269
        // − $29 = 4,240 → monthly 353.33.
        let one = StateEmployee { allowances: 1, ..Default::default() };
        assert_eq!(tax("AR-2026", "10000", 12, &one, "0"), usd_m("353.33"));
        // Under the $5,600 band nothing is withheld.
        assert_eq!(tax("AR-2026", "300", 26, &StateEmployee::default(), "0"), Money::ZERO);
    }

    #[test]
    fn california_examples() {
        let married = |allowances: i64, estimated: i64| StateEmployee {
            married: true,
            allowances,
            second_allowances: estimated,
            ..Default::default()
        };
        // Method B: A weekly $210 single → 0 (low-income exemption); B biweekly $1,600 married,
        // 3 allowances (1 estimated) → 2.38; E semimonthly $2,400 married, 4 → 4.13; F $57,000
        // a year monthly, married, 4 → 7.17.
        let single1 = StateEmployee { allowances: 1, ..Default::default() };
        assert_eq!(tax("CA-2026", "210", 52, &single1, "0"), Money::ZERO);
        assert_eq!(tax("CA-2026", "1600", 26, &married(2, 1), "0"), usd_m("2.38"));
        assert_eq!(tax("CA-2026", "2400", 24, &married(4, 0), "0"), usd_m("4.13"));
        assert_eq!(tax("CA-2026", "4750", 12, &married(4, 0), "0"), usd_m("7.17"));
        // C (monthly $5,100, married, 5) and D (weekly $950, head of household, 3) are worked with
        // the rounded per-period tables (0.82 and 1.69); annualized they are 0.84 and 1.67.
        assert_eq!(tax("CA-2026", "5100", 12, &married(5, 0), "0"), usd_m("0.84"));
        let hoh = StateEmployee { head_of_household: true, allowances: 3, ..Default::default() };
        assert_eq!(tax("CA-2026", "950", 52, &hoh, "0"), usd_m("1.67"));
    }

    #[test]
    fn new_york_examples() {
        let s = |allowances: i64| StateEmployee { allowances, ..Default::default() };
        let m = |allowances: i64| StateEmployee { married: true, allowances, ..Default::default() };
        // NYS-50-T-NYS (1/26) pp.16 and 18.
        assert_eq!(tax("NY-2026", "400", 52, &s(3), "0"), usd_m("8.01"));
        assert_eq!(tax("NY-2026", "50000", 12, &s(3), "0"), usd_m("3576.63"));
        assert_eq!(tax("NY-2026", "400", 52, &m(4), "0"), usd_m("6.69"));
        assert_eq!(tax("NY-2026", "50000", 12, &m(3), "0"), usd_m("3622.09"));
        // Example 2s print 12.43 for 165 × .0753 = 12.4245 and 4.15 for 58.80 × .0707 = 4.157;
        // half-up to the cent gives one cent less / more (258.51 → 258.50, 248.11 → 248.12).
        assert_eq!(tax("NY-2026", "5000", 24, &s(1), "0"), usd_m("258.50"));
        assert_eq!(tax("NY-2026", "5000", 24, &m(3), "0"), usd_m("248.12"));
        // Method III: monthly $100,000 single → 99,383.30 × 10.45% = 10,385.55.
        assert_eq!(tax("NY-2026", "100000", 12, &s(0), "0"), usd_m("10385.55"));
        let pay = PayContext::default();
        assert!(compute("NY-2026", usd_m("1000"), 4, &s(0), &pay).is_err(), "quarterly has no table");
    }

    #[test]
    fn maryland_examples() {
        let county = |code: &str| StateEmployee { allowances: 2, code: Some(code.into()), ..Default::default() };
        // Computed from the 3.20% chart: biweekly $2,000, single, 2 exemptions, Montgomery →
        // (52,000 − 3,400 − 6,400) × 7.95% ÷ 26 = 129.03; a nonresident (2.25%) → 113.62.
        assert_eq!(tax("MD-2026", "2000", 26, &county("MONT"), "0"), usd_m("129.03"));
        assert_eq!(tax("MD-2026", "2000", 26, &county("NR"), "0"), usd_m("113.62"));
        // No county given: the highest local rate (3.30%).
        let none = StateEmployee { allowances: 2, ..Default::default() };
        assert_eq!(tax("MD-2026", "2000", 26, &none, "0"), usd_m("130.66"));
        // Under $192 biweekly nothing is withheld.
        assert_eq!(tax("MD-2026", "191", 26, &none, "0"), Money::ZERO);
    }

    #[test]
    fn maine_examples() {
        let s2 = StateEmployee { allowances: 2, ..Default::default() };
        // 2026 tables (Revised August 2026) p.7.
        assert_eq!(tax("ME-2026", "300", 52, &s2, "0"), Money::ZERO);
        assert_eq!(tax("ME-2026", "1000", 52, &s2, "0"), usd_m("32"));
        let m2 = StateEmployee { married: true, allowances: 2, ..Default::default() };
        assert_eq!(tax("ME-2026", "4500", 52, &m2, "0"), usd_m("256"));
        // Computed, the 2% surcharge: single, $100,000 a month → 89,280.30 ÷ 12 → $7,440.
        assert_eq!(tax("ME-2026", "100000", 12, &StateEmployee::default(), "0"), usd_m("7440"));
    }

    #[test]
    fn wisconsin_examples() {
        // W-166 pp.26–27.
        let s = |allowances: i64| StateEmployee { allowances, ..Default::default() };
        assert_eq!(tax("WI-2026", "350", 52, &s(1), "0"), usd_m("7.59"));
        assert_eq!(tax("WI-2026", "500", 52, &s(3), "0"), usd_m("14.34"));
        let m3 = StateEmployee { married: true, allowances: 3, ..Default::default() };
        assert_eq!(tax("WI-2026", "1000", 26, &m3, "0"), usd_m("22.08"));
    }

    #[test]
    fn connecticut_examples() {
        let code = |c: &str| StateEmployee { code: Some(c.into()), ..Default::default() };
        // IP 2026(1) examples 8–10 come from the wage-bracket tables, which are this formula on
        // each row's midpoint: $692–701 weekly code F → 17.65; $990–1,000 weekly code A →
        // 39.97; $2,269–2,307 biweekly code B → 78.82.
        assert_eq!(tax("CT-2026", "696.50", 52, &code("F"), "0"), usd_m("17.65"));
        assert_eq!(tax("CT-2026", "995", 52, &code("A"), "0"), usd_m("39.97"));
        assert_eq!(tax("CT-2026", "2288", 26, &code("B"), "0"), usd_m("78.82"));
        // On the exact wages instead of the row midpoint.
        assert_eq!(tax("CT-2026", "1000", 52, &code("A"), "0"), usd_m("40.24"));
        // Code E withholds nothing; no CT-W4 on file: 6.99% with no exemption.
        assert_eq!(tax("CT-2026", "1000", 52, &code("E"), "0"), Money::ZERO);
        assert_eq!(tax("CT-2026", "1000", 52, &StateEmployee::default(), "0"), usd_m("69.90"));
        // High wages reach the add-back and recapture: code A, $600,000 a year (monthly) →
        // 31,250 + 6.99% × 100,000 + 250 + 3,400 = 41,890 → 3,490.83.
        assert_eq!(tax("CT-2026", "50000", 12, &code("A"), "0"), usd_m("3490.83"));
    }

    #[test]
    fn new_jersey_examples() {
        // NJ-WT p.25 cases worked with the current (October 2020) weekly tables.
        let a1 = StateEmployee { allowances: 1, ..Default::default() };
        assert_eq!(tax("NJ-2026", "300", 52, &a1, "0"), usd_m("4.21"));
        assert_eq!(tax("NJ-2026", "700", 52, &a1, "0"), usd_m("11.84"));
        assert_eq!(tax("NJ-2026", "1200", 52, &a1, "0"), usd_m("40.41"));
        let b3 = StateEmployee { married: true, allowances: 3, ..Default::default() };
        assert_eq!(tax("NJ-2026", "375", 52, &b3, "0"), usd_m("4.76"));
        assert_eq!(tax("NJ-2026", "950", 52, &b3, "0"), usd_m("15.92"));
        assert_eq!(tax("NJ-2026", "1400", 52, &b3, "0"), usd_m("27.58"));
        // A chosen rate letter wins over the filing status.
        let e = StateEmployee { code: Some("E".into()), ..a1.clone() };
        assert_ne!(tax("NJ-2026", "3000", 52, &e, "0"), tax("NJ-2026", "3000", 52, &a1, "0"));
    }
}
