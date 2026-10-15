//! Statutory rates and the per-line payroll arithmetic (reference `payroll-rates.ts`).
//!
//! Rates are configuration values that arrive as JavaScript numbers (settings strings read with
//! `Number()`, request bodies). Each one is held as the exact decimal JavaScript prints for it
//! (`0.08`, `25`, `150000.5`), and every product with an amount is computed exactly in integers
//! and rounded half-up to the cent at the same steps as the reference's `r2` (deviation D4: the
//! reference multiplied binary floats; the two agree except at rare float ties).

use app_core::jsnum::{js_number, js_string, js_value};
use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{json, Map, Value};

// ─── Exact decimal ──────────────────────────────────────────────────────────

/// `m / 10^s`, exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dec {
    m: i128,
    s: u32,
}

/// The largest scale kept; JavaScript numbers in the valid ranges never need more.
const MAX_SCALE: u32 = 24;

impl Dec {
    pub const ZERO: Dec = Dec { m: 0, s: 0 };

    /// The exact value of the decimal text JavaScript prints for `x` (`String(x)`).
    pub fn from_js(x: f64) -> Option<Dec> {
        if !x.is_finite() {
            return None;
        }
        Dec::parse(&js_string(x))
    }

    /// `[-]digits[.digits][e[+-]digits]`.
    pub fn parse(text: &str) -> Option<Dec> {
        let (mantissa, exp) = match text.split_once(['e', 'E']) {
            Some((m, e)) => (m, e.parse::<i32>().ok()?),
            None => (text, 0),
        };
        let (neg, digits) = match mantissa.strip_prefix('-') {
            Some(d) => (true, d),
            None => (false, mantissa),
        };
        let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
        if (int.is_empty() && frac.is_empty()) || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut m: i128 = format!("{int}{frac}").parse().ok()?;
        let mut scale = i32::try_from(frac.len()).ok()? - exp;
        while scale < 0 {
            m = m.checked_mul(10)?;
            scale += 1;
        }
        let mut s = u32::try_from(scale).ok()?;
        while s > MAX_SCALE {
            // Far below a cent of a cent; only reachable with absurd configuration values.
            m /= 10;
            s -= 1;
        }
        Some(Dec { m: if neg { -m } else { m }, s })
    }

    pub fn is_negative(self) -> bool {
        self.m < 0
    }

    fn pow10(n: u32) -> Option<i128> {
        10i128.checked_pow(n)
    }

    /// The value as whole cents, rounded half-up (`r2` of a rupee amount).
    pub fn to_cents(self) -> Option<Money> {
        let scaled = self.m.checked_mul(100)?;
        let cents = div_half_up(scaled, Dec::pow10(self.s)?)?;
        i64::try_from(cents).ok().map(Money::from_cents)
    }

    /// The value as an integer if it has no fraction.
    pub fn to_integer(self) -> Option<i64> {
        let p = Dec::pow10(self.s)?;
        (self.m % p == 0).then(|| i64::try_from(self.m / p).ok()).flatten()
    }

    /// `r2(amount × self)`.
    pub fn times(self, amount: Money, mode: Rounding) -> Option<Money> {
        let product = i128::from(amount.cents()).checked_mul(self.m)?;
        let cents = div_round(product, Dec::pow10(self.s)?, mode)?;
        i64::try_from(cents).ok().map(Money::from_cents)
    }

    /// The JavaScript number this decimal came from, for JSON output.
    pub fn to_json(self) -> Value {
        js_number(&self.to_string()).map(js_value).unwrap_or(Value::Null)
    }
}

impl std::fmt::Display for Dec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sign = if self.m < 0 { "-" } else { "" };
        let digits = self.m.unsigned_abs().to_string();
        let s = self.s as usize;
        if s == 0 {
            return write!(f, "{sign}{digits}");
        }
        let padded = format!("{digits:0>width$}", width = s + 1);
        let (int, frac) = padded.split_at(padded.len() - s);
        write!(f, "{sign}{int}.{frac}")
    }
}

/// How an exact half is rounded. Production always uses `HalfUp` (deviation D4); `HalfDown`
/// exists so tests can prove a difference from the reference comes only from an exact tie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    HalfUp,
    HalfDown,
}

/// `n / d` rounded to an integer (`d` > 0), ties per `mode`, away from / toward zero.
fn div_round(n: i128, d: i128, mode: Rounding) -> Option<i128> {
    let q = n / d;
    let r = n % d;
    let twice = r.checked_abs()?.checked_mul(2)?;
    let up = match mode {
        Rounding::HalfUp => twice >= d,
        Rounding::HalfDown => twice > d,
    };
    Some(if up { q + n.signum() } else { q })
}

fn div_half_up(n: i128, d: i128) -> Option<i128> {
    div_round(n, d, Rounding::HalfUp)
}

// ─── Rates ──────────────────────────────────────────────────────────────────

/// `[upper bound (None = no limit), rate]`, progressive and monthly.
pub type PayeSlab = (Option<Dec>, Dec);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rates {
    pub epf_employee_rate: Dec,
    pub epf_employer_rate: Dec,
    pub etf_rate: Dec,
    pub stamp_duty: Dec,
    pub employer_welfare: Dec,
    pub days_in_month: Dec,
    pub paye_slabs: Vec<PayeSlab>,
}

/// The settings keys each rate lives under, in the reference's order.
pub const RATE_KEYS: &[(&str, &str)] = &[
    ("epfEmployeeRate", "payroll.epfEmployeeRate"),
    ("epfEmployerRate", "payroll.epfEmployerRate"),
    ("etfRate", "payroll.etfRate"),
    ("stampDuty", "payroll.stampDuty"),
    ("employerWelfare", "payroll.employerWelfare"),
    ("daysInMonth", "payroll.daysInMonth"),
    ("payeSlabs", "payroll.payeSlabs"),
];

fn dec(text: &str) -> Dec {
    Dec::parse(text).unwrap_or(Dec::ZERO)
}

/// From the company's April 2026 salary sheet (reference `DEFAULT_RATES`).
pub fn default_rates() -> Rates {
    Rates {
        epf_employee_rate: dec("0.08"),
        epf_employer_rate: dec("0.12"),
        etf_rate: dec("0.03"),
        stamp_duty: dec("25"),
        employer_welfare: dec("150"),
        days_in_month: dec("30"),
        paye_slabs: vec![
            (Some(dec("250000")), dec("0")),
            (Some(dec("500000")), dec("0.06")),
            (Some(dec("750000")), dec("0.12")),
            (None, dec("0.18")),
        ],
    }
}

/// Reference `isValidSlabs`: a non-empty array of `[upper, rate]` pairs, rates 0–1, upper
/// bounds positive and ascending, and only the last one may be null.
pub fn slabs_from_json(v: &Value) -> Option<Vec<PayeSlab>> {
    let items = v.as_array().filter(|a| !a.is_empty())?;
    let mut prev = 0.0;
    let mut out = Vec::with_capacity(items.len());
    for (i, s) in items.iter().enumerate() {
        let pair = s.as_array().filter(|p| p.len() == 2)?;
        let rate = pair[1].as_f64().filter(|r| (0.0..=1.0).contains(r))?;
        let last = i == items.len() - 1;
        let upper = match &pair[0] {
            Value::Null if last => None,
            Value::Number(n) => {
                let u = n.as_f64()?;
                if u <= prev {
                    return None;
                }
                prev = u;
                Some(Dec::from_js(u)?)
            }
            _ => return None,
        };
        out.push((upper, Dec::from_js(rate)?));
    }
    Some(out)
}

pub fn slabs_to_json(slabs: &[PayeSlab]) -> Value {
    Value::Array(slabs.iter().map(|(u, r)| json!([u.map(Dec::to_json), r.to_json()])).collect())
}

/// Reference `ratesFromSettings`: each key falls back to its default on its own.
pub fn rates_from_settings(settings: &Map<String, Value>) -> Rates {
    let d = default_rates();
    let num = |key: &str, default: Dec| -> Dec {
        let raw = settings.get(key).and_then(Value::as_str).unwrap_or("");
        if raw.is_empty() {
            return default;
        }
        js_number(raw).and_then(Dec::from_js).unwrap_or(default)
    };
    let paye_slabs = settings
        .get("payroll.payeSlabs")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| slabs_from_json(&v))
        .unwrap_or(d.paye_slabs.clone());
    Rates {
        epf_employee_rate: num("payroll.epfEmployeeRate", d.epf_employee_rate),
        epf_employer_rate: num("payroll.epfEmployerRate", d.epf_employer_rate),
        etf_rate: num("payroll.etfRate", d.etf_rate),
        stamp_duty: num("payroll.stampDuty", d.stamp_duty),
        employer_welfare: num("payroll.employerWelfare", d.employer_welfare),
        days_in_month: num("payroll.daysInMonth", d.days_in_month),
        paye_slabs,
    }
}

impl Rates {
    pub fn to_json(&self) -> Value {
        json!({
            "epfEmployeeRate": self.epf_employee_rate.to_json(),
            "epfEmployerRate": self.epf_employer_rate.to_json(),
            "etfRate": self.etf_rate.to_json(),
            "stampDuty": self.stamp_duty.to_json(),
            "employerWelfare": self.employer_welfare.to_json(),
            "daysInMonth": self.days_in_month.to_json(),
            "payeSlabs": slabs_to_json(&self.paye_slabs),
        })
    }

    /// A run's `ratesSnapshot` as stored. Unknown or broken values fall back to the defaults.
    pub fn from_json(v: &Value) -> Rates {
        let d = default_rates();
        let num = |k: &str, default: Dec| v.get(k).and_then(Value::as_f64).and_then(Dec::from_js).unwrap_or(default);
        Rates {
            epf_employee_rate: num("epfEmployeeRate", d.epf_employee_rate),
            epf_employer_rate: num("epfEmployerRate", d.epf_employer_rate),
            etf_rate: num("etfRate", d.etf_rate),
            stamp_duty: num("stampDuty", d.stamp_duty),
            employer_welfare: num("employerWelfare", d.employer_welfare),
            days_in_month: num("daysInMonth", d.days_in_month),
            paye_slabs: v.get("payeSlabs").and_then(slabs_from_json).unwrap_or(d.paye_slabs),
        }
    }

    /// `rates.daysInMonth || 30` as the integer stored on a line.
    pub fn line_days_in_month(&self) -> i64 {
        match self.days_in_month.to_integer() {
            Some(0) | None => 30,
            Some(n) => n,
        }
    }
}

// ─── Line arithmetic ────────────────────────────────────────────────────────

/// The editable inputs of a line. `days_worked` is in hundredths of a day.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInputs {
    pub epf_applicable: bool,
    pub days_in_month: i64,
    pub days_worked: i64,
    pub basic: Money,
    pub bra2005: Money,
    pub bra2016: Money,
    pub vehicle_allowance: Money,
    pub other_allowance: Money,
    pub project_bonus: Money,
    pub arrears: Money,
    pub medical: Money,
    pub other_earnings: Money,
    pub no_pay_amount: Money,
    pub paye_override: Option<Money>,
    pub welfare: Money,
    pub loan_recovery: Money,
    pub salary_advance: Money,
    pub phone_bill: Money,
    pub other_deductions: Money,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineFigures {
    pub basic_for_month: Money,
    pub earnings_total: Money,
    pub gross_salary: Money,
    pub epf_base: Money,
    pub epf_employee: Money,
    pub paye: Money,
    pub stamp_duty: Money,
    pub deductions_total: Money,
    pub net_salary: Money,
    pub epf_employer: Money,
    pub etf: Money,
    pub employer_welfare: Money,
    pub cost_to_company: Money,
}

fn too_large() -> AppError {
    AppError::bad_request("Amount is too large")
}

fn sum(items: &[Money]) -> AppResult<Money> {
    items.iter().try_fold(Money::ZERO, |acc, m| acc.checked_add(*m).ok_or_else(too_large))
}

/// Reference `calcPaye`: each slab taxes only the income inside it; rounded once at the end.
pub fn calc_paye(taxable: Money, slabs: &[PayeSlab]) -> AppResult<Money> {
    calc_paye_with(taxable, slabs, Rounding::HalfUp)
}

pub fn calc_paye_with(taxable: Money, slabs: &[PayeSlab], mode: Rounding) -> AppResult<Money> {
    let scale = slabs.iter().map(|(_, r)| r.s).max().unwrap_or(0);
    let unit = Dec::pow10(scale).ok_or_else(too_large)?;
    let mut tax: i128 = 0; // cents × 10^scale
    let mut lower = Money::ZERO;
    for (upper, rate) in slabs {
        if taxable <= lower {
            break;
        }
        let upper_cents = match upper {
            Some(u) => Some(u.to_cents().ok_or_else(too_large)?),
            None => None,
        };
        let top = match upper_cents {
            None => taxable,
            Some(u) => u.min(taxable),
        };
        let band = i128::from(top.cents() - lower.cents());
        let rate_scaled =
            rate.m.checked_mul(Dec::pow10(scale - rate.s).ok_or_else(too_large)?).ok_or_else(too_large)?;
        tax = tax.checked_add(band.checked_mul(rate_scaled).ok_or_else(too_large)?).ok_or_else(too_large)?;
        match upper_cents {
            None => break,
            Some(u) => lower = u,
        }
    }
    let cents = div_round(tax.max(0), unit, mode).ok_or_else(too_large)?;
    i64::try_from(cents).map(Money::from_cents).map_err(|_| too_large())
}

/// Reference `computeLine`: basic prorated by days; EPF on basic + BRA; PAYE on gross; stamp
/// duty and employer welfare flat when there is any gross pay.
pub fn compute_line(i: &LineInputs, rates: &Rates) -> AppResult<LineFigures> {
    compute_line_with(i, rates, Rounding::HalfUp)
}

pub fn compute_line_with(i: &LineInputs, rates: &Rates, mode: Rounding) -> AppResult<LineFigures> {
    let days = i.days_worked.clamp(0, i.days_in_month.max(0) * 100);
    let basic_for_month = if i.days_in_month > 0 {
        let n = i128::from(i.basic.cents()) * i128::from(days);
        let d = i128::from(i.days_in_month) * 100;
        Money::from_cents(i64::try_from(div_round(n, d, mode).ok_or_else(too_large)?).map_err(|_| too_large())?)
    } else {
        Money::ZERO
    };
    let earnings_total = sum(&[
        basic_for_month,
        i.bra2005,
        i.bra2016,
        i.vehicle_allowance,
        i.other_allowance,
        i.project_bonus,
        i.arrears,
        i.medical,
        i.other_earnings,
    ])?;
    let gross_salary = earnings_total.checked_sub(i.no_pay_amount).ok_or_else(too_large)?.max(Money::ZERO);

    let epf_base = if i.epf_applicable { sum(&[basic_for_month, i.bra2005, i.bra2016])? } else { Money::ZERO };
    let epf_employee = rates.epf_employee_rate.times(epf_base, mode).ok_or_else(too_large)?;
    let epf_employer = rates.epf_employer_rate.times(epf_base, mode).ok_or_else(too_large)?;
    let etf = rates.etf_rate.times(epf_base, mode).ok_or_else(too_large)?;

    let paye = match i.paye_override {
        Some(p) => p,
        None => calc_paye_with(gross_salary, &rates.paye_slabs, mode)?,
    };
    let positive = gross_salary > Money::ZERO;
    let stamp_duty = if positive { rates.stamp_duty.to_cents().ok_or_else(too_large)? } else { Money::ZERO };
    let employer_welfare =
        if positive { rates.employer_welfare.to_cents().ok_or_else(too_large)? } else { Money::ZERO };

    let deductions_total = sum(&[
        epf_employee,
        paye,
        stamp_duty,
        i.welfare,
        i.loan_recovery,
        i.salary_advance,
        i.phone_bill,
        i.other_deductions,
    ])?;
    let net_salary = gross_salary.checked_sub(deductions_total).ok_or_else(too_large)?;
    let cost_to_company = sum(&[gross_salary, epf_employer, etf, employer_welfare])?;

    Ok(LineFigures {
        basic_for_month,
        earnings_total,
        gross_salary,
        epf_base,
        epf_employee,
        paye,
        stamp_duty,
        deductions_total,
        net_salary,
        epf_employer,
        etf,
        employer_welfare,
        cost_to_company,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/reference_payroll.json")).unwrap_or(Value::Null)
    }

    fn m(v: &Value) -> Money {
        Money::from_json(v).unwrap_or_else(|| panic!("not money: {v}"))
    }

    fn rate_sets(fx: &Value) -> Vec<Rates> {
        fx["rateSets"].as_array().cloned().unwrap_or_default().iter().map(Rates::from_json).collect()
    }

    fn inputs(v: &Value) -> LineInputs {
        LineInputs {
            epf_applicable: v["epfApplicable"].as_bool().unwrap_or(false),
            days_in_month: v["daysInMonth"].as_i64().unwrap_or(0),
            days_worked: m(&v["daysWorked"]).cents(),
            basic: m(&v["basic"]),
            bra2005: m(&v["bra2005"]),
            bra2016: m(&v["bra2016"]),
            vehicle_allowance: m(&v["vehicleAllowance"]),
            other_allowance: m(&v["otherAllowance"]),
            project_bonus: m(&v["projectBonus"]),
            arrears: m(&v["arrears"]),
            medical: m(&v["medical"]),
            other_earnings: m(&v["otherEarnings"]),
            no_pay_amount: m(&v["noPayAmount"]),
            paye_override: (!v["payeOverride"].is_null()).then(|| m(&v["payeOverride"])),
            welfare: m(&v["welfare"]),
            loan_recovery: m(&v["loanRecovery"]),
            salary_advance: m(&v["salaryAdvance"]),
            phone_bill: m(&v["phoneBill"]),
            other_deductions: m(&v["otherDeductions"]),
        }
    }

    fn figures_json(f: &LineFigures) -> Value {
        json!({
            "basicForMonth": f.basic_for_month.to_json_number(),
            "earningsTotal": f.earnings_total.to_json_number(),
            "grossSalary": f.gross_salary.to_json_number(),
            "epfBase": f.epf_base.to_json_number(),
            "epfEmployee": f.epf_employee.to_json_number(),
            "paye": f.paye.to_json_number(),
            "stampDuty": f.stamp_duty.to_json_number(),
            "deductionsTotal": f.deductions_total.to_json_number(),
            "netSalary": f.net_salary.to_json_number(),
            "epfEmployer": f.epf_employer.to_json_number(),
            "etf": f.etf.to_json_number(),
            "employerWelfare": f.employer_welfare.to_json_number(),
            "costToCompany": f.cost_to_company.to_json_number(),
        })
    }

    #[test]
    fn april_2026_regression_row() -> AppResult<()> {
        let fx = fixture();
        let f = compute_line(&inputs(&fx["lines"][0]["input"]), &default_rates())?;
        assert_eq!(f.gross_salary, Money::from_cents(8_385_000));
        assert_eq!(f.epf_employee, Money::from_cents(352_000));
        assert_eq!(f.epf_employer, Money::from_cents(528_000));
        assert_eq!(f.etf, Money::from_cents(132_000));
        assert_eq!(f.net_salary, Money::from_cents(8_000_500));
        Ok(())
    }

    #[test]
    fn every_line_matches_the_reference_compute_line() -> AppResult<()> {
        let fx = fixture();
        let sets = rate_sets(&fx);
        let lines = fx["lines"].as_array().cloned().unwrap_or_default();
        assert!(lines.len() > 400);
        let (mut same, mut ties) = (0, 0);
        for l in lines {
            let set = &sets[l["rates"].as_u64().unwrap_or(0) as usize];
            let got = figures_json(&compute_line(&inputs(&l["input"]), set)?);
            if got == l["output"] {
                same += 1;
                continue;
            }
            // D4: the reference's floats rounded an exact half-cent down here.
            let down = figures_json(&compute_line_with(&inputs(&l["input"]), set, Rounding::HalfDown)?);
            assert_eq!(down, l["output"], "not an exact tie: input {}", l["input"]);
            ties += 1;
        }
        assert!(same > 300, "{same} identical, {ties} exact ties");
        Ok(())
    }

    #[test]
    fn paye_matches_the_reference_calc_paye() -> AppResult<()> {
        let fx = fixture();
        let sets = rate_sets(&fx);
        for c in fx["paye"].as_array().cloned().unwrap_or_default() {
            let set = &sets[c["rates"].as_u64().unwrap_or(0) as usize];
            let got = calc_paye(m(&c["taxable"]), &set.paye_slabs)?;
            if got.to_json_number() != c["paye"] {
                let down = calc_paye_with(m(&c["taxable"]), &set.paye_slabs, Rounding::HalfDown)?;
                assert_eq!(
                    down.to_json_number(),
                    c["paye"],
                    "not an exact tie: taxable {} set {}",
                    c["taxable"],
                    c["rates"]
                );
            }
        }
        Ok(())
    }

    #[test]
    fn slab_validation_matches_the_reference() {
        let fx = fixture();
        for c in fx["slabs"].as_array().cloned().unwrap_or_default() {
            assert_eq!(slabs_from_json(&c["slabs"]).is_some(), c["valid"].as_bool().unwrap_or(false), "{}", c["slabs"]);
        }
    }

    #[test]
    fn rates_read_from_settings_fall_back_per_key() {
        let mut s = Map::new();
        s.insert("payroll.epfEmployeeRate".into(), json!("0.085"));
        s.insert("payroll.stampDuty".into(), json!("abc"));
        s.insert("payroll.etfRate".into(), json!(""));
        s.insert("payroll.payeSlabs".into(), json!("[[100,2]]"));
        let r = rates_from_settings(&s);
        assert_eq!(r.epf_employee_rate.to_string(), "0.085");
        assert_eq!(r, Rates { epf_employee_rate: r.epf_employee_rate, ..default_rates() });
        assert_eq!(
            default_rates().to_json(),
            json!({"epfEmployeeRate":0.08,"epfEmployerRate":0.12,"etfRate":0.03,"stampDuty":25,"employerWelfare":150,
                   "daysInMonth":30,"payeSlabs":[[250000,0],[500000,0.06],[750000,0.12],[null,0.18]]})
        );
        assert_eq!(Dec::parse("1.5e-7").map(|d| d.to_string()), Some("0.00000015".into()));
        assert_eq!(Dec::parse("2.5e3").map(|d| d.to_string()), Some("2500".into()));
    }
}
