//! D69, step 6 of the US state payroll plan: local (city / county) taxes.
//!
//! A company keeps a list of localities (`us_localities`): each is a percentage of local wages
//! (resident and nonresident rates, an employer rate, an annual exemption per allowance), a flat
//! amount per month above a monthly wage threshold (Colorado occupational privilege taxes), a flat
//! amount per pay period with a yearly cap (Pennsylvania Local Services Tax), or a built-in
//! formula (New York City, Yonkers, Philadelphia, the Portland Metro SHS and Multnomah County
//! PFA taxes). Built-in entries ([`CATALOG`]) are copied into the company's list when it adds
//! them; anything else it enters itself (Ohio cities, Pennsylvania EIT by PSD, Kentucky, …).
//! Each employee lists the localities withheld for, as a resident or not, with their local
//! allowances. Localities sharing a `group` (e.g. Pennsylvania EIT: the higher of the resident
//! and the work-place nonresident rate) withhold only the largest employee amount.
//! Amounts are half-up to the cent, except the Pennsylvania LST, which the law rounds down.

use crate::us_leave::{Contribution, Ytd};
use crate::us_state::{PayContext, StateEmployee};
use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const PPM: i128 = 1_000_000;

/// One locality as the company keeps it (and as a pay line snapshots it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locality {
    pub code: String,
    pub name: String,
    pub state: String,
    /// PERCENT | MONTHLY_FLAT | WEEKLY_FLAT | PERIOD_FLAT | FORMULA.
    pub method: String,
    pub formula: Option<String>,
    /// ppm.
    pub resident_rate: i64,
    pub nonresident_rate: i64,
    pub employer_rate: i64,
    /// Annual exemption per local allowance / second allowance (cents), PERCENT only.
    pub exemption: i64,
    pub second_exemption: i64,
    /// Flat amounts (cents): per month (MONTHLY_FLAT) or per year (PERIOD_FLAT, spread over the
    /// pay periods and rounded down).
    pub flat_employee: i64,
    pub flat_employer: i64,
    /// MONTHLY_FLAT: monthly wages at or above which the tax is due (cents).
    pub monthly_threshold: i64,
    /// Yearly maximum employee amount (cents).
    pub employee_year_cap: Option<i64>,
    pub includes_401k: bool,
    pub includes_125: bool,
    pub group: Option<String>,
    pub account_id: Option<String>,
    pub source: String,
    pub verified: bool,
}

impl Locality {
    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code, "name": self.name, "state": self.state, "method": self.method,
            "formula": self.formula, "residentRate": self.resident_rate, "nonresidentRate": self.nonresident_rate,
            "employerRate": self.employer_rate, "exemption": self.exemption, "secondExemption": self.second_exemption,
            "flatEmployee": self.flat_employee, "flatEmployer": self.flat_employer,
            "monthlyThreshold": self.monthly_threshold, "employeeYearCap": self.employee_year_cap,
            "includes401k": self.includes_401k, "includes125": self.includes_125, "group": self.group,
            "accountId": self.account_id, "source": self.source, "verified": self.verified,
        })
    }

    pub fn from_json(v: &Value) -> Option<Locality> {
        let i = |k: &str| v[k].as_i64().unwrap_or(0);
        Some(Locality {
            code: v["code"].as_str()?.to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            state: v["state"].as_str().unwrap_or_default().to_string(),
            method: v["method"].as_str().unwrap_or("PERCENT").to_string(),
            formula: v["formula"].as_str().map(str::to_string),
            resident_rate: i("residentRate"),
            nonresident_rate: i("nonresidentRate"),
            employer_rate: i("employerRate"),
            exemption: i("exemption"),
            second_exemption: i("secondExemption"),
            flat_employee: i("flatEmployee"),
            flat_employer: i("flatEmployer"),
            monthly_threshold: i("monthlyThreshold"),
            employee_year_cap: v["employeeYearCap"].as_i64(),
            includes_401k: v["includes401k"].as_bool().unwrap_or(true),
            includes_125: v["includes125"].as_bool().unwrap_or(false),
            group: v["group"].as_str().map(str::to_string),
            account_id: v["accountId"].as_str().map(str::to_string),
            source: v["source"].as_str().unwrap_or_default().to_string(),
            verified: v["verified"].as_bool().unwrap_or(true),
        })
    }
}

/// One locality an employee is withheld for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assignment {
    pub code: String,
    pub resident: bool,
    pub allowances: i64,
    pub second_allowances: i64,
    pub extra: i64,
    pub exempt: bool,
}

impl Assignment {
    pub fn to_json(&self) -> Value {
        json!({"code": self.code, "resident": self.resident, "allowances": self.allowances,
               "secondAllowances": self.second_allowances, "extra": self.extra, "exempt": self.exempt})
    }

    pub fn from_json(v: &Value) -> Option<Assignment> {
        Some(Assignment {
            code: v["code"].as_str()?.to_string(),
            resident: v["resident"].as_bool().unwrap_or(false),
            allowances: v["allowances"].as_i64().unwrap_or(0),
            second_allowances: v["secondAllowances"].as_i64().unwrap_or(0),
            extra: v["extra"].as_i64().unwrap_or(0),
            exempt: v["exempt"].as_bool().unwrap_or(false),
        })
    }
}

/// What a pay line uses for one locality: the locality (rates resolved for the pay date) and
/// the employee's assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub locality: Locality,
    pub assignment: Assignment,
}

impl Plan {
    pub fn to_json(&self) -> Value {
        json!({"locality": self.locality.to_json(), "assignment": self.assignment.to_json()})
    }

    pub fn from_json(v: &Value) -> Option<Plan> {
        Some(Plan {
            locality: Locality::from_json(&v["locality"])?,
            assignment: Assignment::from_json(&v["assignment"])?,
        })
    }
}

fn base(code: &str, name: &str, state: &str, method: &str, source: &str) -> Locality {
    Locality {
        code: code.into(),
        name: name.into(),
        state: state.into(),
        method: method.into(),
        formula: None,
        resident_rate: 0,
        nonresident_rate: 0,
        employer_rate: 0,
        exemption: 0,
        second_exemption: 0,
        flat_employee: 0,
        flat_employer: 0,
        monthly_threshold: 0,
        employee_year_cap: None,
        includes_401k: true,
        includes_125: false,
        group: None,
        account_id: None,
        source: source.into(),
        verified: true,
    }
}

fn formula(code: &str, name: &str, state: &str, id: &str, source: &str) -> Locality {
    Locality { formula: Some(id.into()), ..base(code, name, state, "FORMULA", source) }
}

fn percent(code: &str, name: &str, state: &str, resident: i64, nonresident: i64, source: &str) -> Locality {
    Locality { resident_rate: resident, nonresident_rate: nonresident, ..base(code, name, state, "PERCENT", source) }
}

fn monthly(
    code: &str,
    name: &str,
    state: &str,
    employee: i64,
    employer: i64,
    threshold: i64,
    source: &str,
) -> Locality {
    Locality {
        flat_employee: employee,
        flat_employer: employer,
        monthly_threshold: threshold,
        ..base(code, name, state, "MONTHLY_FLAT", source)
    }
}

/// Built-in localities the company can add (2026; `verified` false where the city's own page
/// could only be read through a search excerpt — the screen asks the company to check it).
pub fn catalog() -> Vec<Locality> {
    let mut out = vec![
        formula("NYC", "New York City", "NY", "NYC-2026", "NYS-50-T-NYC (1/26), Method II tables II-A to II-D; New York City residents only"),
        formula("YONKERS", "Yonkers (resident surcharge)", "NY", "YONKERS-RES-2026", "NYS-50-T-Y (1/26): 16.75% of the New York State withholding; Yonkers residents"),
        formula("YONKERS-NR", "Yonkers (nonresident earnings tax)", "NY", "YONKERS-NR-2026", "NYS-50-T-Y (1/26), Method VII: 0.50% of wages for work in Yonkers less the table's exclusion"),
        Locality {
            includes_125: true,
            ..formula("PHL", "Philadelphia Wage Tax", "PA", "PHL-2026", "City of Philadelphia, Wage Tax (employers): 3.74% / 3.43% to 30 June 2026, then 3.735% / 3.425%; 401(k) and section 125 amounts taxable")
        },
        formula("METRO-SHS", "Portland Metro SHS income tax", "OR", "METRO-SHS-2026", "Metro Supportive Housing Services employer handbook (12/30/25): 1% over $200,000 after the Oregon federal-tax deduction and $5,820"),
        formula("MULT-PFA", "Multnomah County Preschool for All", "OR", "MULT-PFA-2026", "Multnomah County PFA employer handbook (12/30/25): 1.5% over $200,000, 3% over $400,000 after the federal-tax deduction and $5,820"),
        Locality {
            employee_year_cap: Some(5_200),
            ..base("PA-LST", "Pennsylvania Local Services Tax (enter your work place's rate)", "PA", "PERIOD_FLAT", "DCED Local Services Tax: up to $52 a year, divided by the pay periods and rounded down; exempt under $12,000 when the tax is over $10")
        },
        Locality {
            group: Some("PA-EIT".into()),
            ..percent("PA-EIT", "Pennsylvania EIT (enter the PSD's rates)", "PA", 0, 0, "Act 32: withhold the higher of the resident rate (home PSD) and the nonresident rate (work PSD), from DCED Municipal Statistics")
        },
        monthly("GV-OPT", "Greenwood Village occupational privilege tax", "CO", 200, 200, 25_000, "City of Greenwood Village, Occupational Privilege Tax: $2 + $2 a month when $250 or more is earned in the month"),
        monthly("GLENDALE-OPT", "Glendale (CO) occupational privilege tax", "CO", 500, 500, 75_001, "City of Glendale, Occupational Privilege Tax: $5 + $5 a month when more than $750 is earned in the month"),
        Locality {
            verified: false,
            ..monthly("DENVER-OPT", "Denver occupational privilege tax", "CO", 575, 400, 50_000, "Denver Tax Guide Topic 61: employee $5.75 + employer $4.00 a month when $500 or more is earned in Denver in the month")
        },
        Locality {
            verified: false,
            includes_401k: false,
            ..percent("KCMO", "Kansas City (MO) earnings tax", "MO", 10_000, 10_000, "Kansas City Tax FAQs: 1% (nonresidents on work in Kansas City); cafeteria plan and deferred compensation excluded")
        },
        Locality {
            verified: false,
            includes_401k: false,
            ..percent("STL", "St. Louis City earnings tax", "MO", 10_000, 10_000, "City of St. Louis, earnings tax: 1% (nonresidents on work in the City); sections 125–129 and deferred compensation excluded")
        },
        Locality {
            verified: false,
            ..percent("OR-TRANSIT", "Oregon statewide transit tax", "OR", 1_000, 1_000, "Oregon DOR, 2026 payroll updates: employee 0.1% (residents, and nonresidents for work in Oregon)")
        },
        Locality {
            verified: false,
            employer_rate: 8_237,
            ..base("TRIMET", "TriMet transit tax (employer)", "OR", "PERCENT", "Oregon DOR, 2026 Combined Payroll Tax Report instructions: employer 0.8237%")
        },
        Locality {
            verified: false,
            employer_rate: 8_000,
            ..base("LTD", "Lane Transit District tax (employer)", "OR", "PERCENT", "Oregon DOR, 2026 Combined Payroll Tax Report instructions: employer 0.80%")
        },
    ];
    out.extend(indiana());
    out.extend(michigan());
    out.extend(others());
    out
}

/// Indiana county tax, Departmental Notice #1 (R47, effective 1 October 2026): the county of
/// residence on 1 January (out-of-state residents: the county of principal work), on the state
/// base: wages less $1,000 per personal exemption and $1,500 per dependent (count an adopted
/// child's $3,000 as two dependents). Boone was 1.70% before 1 October 2026.
fn indiana() -> Vec<Locality> {
    const COUNTIES: &[(&str, &str, i64)] = &[
        ("01", "Adams", 16_000),
        ("02", "Allen", 15_900),
        ("03", "Bartholomew", 17_500),
        ("04", "Benton", 17_900),
        ("05", "Blackford", 25_000),
        ("06", "Boone", 17_100),
        ("07", "Brown", 25_234),
        ("08", "Carroll", 24_733),
        ("09", "Cass", 29_500),
        ("10", "Clark", 20_000),
        ("11", "Clay", 23_500),
        ("12", "Clinton", 26_500),
        ("13", "Crawford", 16_500),
        ("14", "Daviess", 15_000),
        ("15", "Dearborn", 14_000),
        ("16", "Decatur", 24_500),
        ("17", "DeKalb", 21_300),
        ("18", "Delaware", 15_000),
        ("19", "Dubois", 12_000),
        ("20", "Elkhart", 20_000),
        ("21", "Fayette", 28_200),
        ("22", "Floyd", 18_900),
        ("23", "Fountain", 21_000),
        ("24", "Franklin", 17_000),
        ("25", "Fulton", 28_800),
        ("26", "Gibson", 13_000),
        ("27", "Grant", 27_500),
        ("28", "Greene", 23_500),
        ("29", "Hamilton", 11_000),
        ("30", "Hancock", 19_400),
        ("31", "Harrison", 10_000),
        ("32", "Hendricks", 17_000),
        ("33", "Henry", 20_200),
        ("34", "Howard", 23_500),
        ("35", "Huntington", 19_500),
        ("36", "Jackson", 21_000),
        ("37", "Jasper", 28_640),
        ("38", "Jay", 25_000),
        ("39", "Jefferson", 10_300),
        ("40", "Jennings", 25_000),
        ("41", "Johnson", 14_000),
        ("42", "Knox", 17_000),
        ("43", "Kosciusko", 10_000),
        ("44", "LaGrange", 16_500),
        ("45", "Lake", 15_000),
        ("46", "LaPorte", 14_500),
        ("47", "Lawrence", 17_500),
        ("48", "Madison", 22_500),
        ("49", "Marion", 20_200),
        ("50", "Marshall", 12_500),
        ("51", "Martin", 25_000),
        ("52", "Miami", 25_400),
        ("53", "Monroe", 21_400),
        ("54", "Montgomery", 26_500),
        ("55", "Morgan", 27_200),
        ("56", "Newton", 10_000),
        ("57", "Noble", 17_500),
        ("58", "Ohio", 20_000),
        ("59", "Orange", 17_500),
        ("60", "Owen", 25_000),
        ("61", "Parke", 26_500),
        ("62", "Perry", 14_000),
        ("63", "Pike", 12_000),
        ("64", "Porter", 5_000),
        ("65", "Posey", 14_500),
        ("66", "Pulaski", 28_500),
        ("67", "Putnam", 23_000),
        ("68", "Randolph", 30_000),
        ("69", "Ripley", 23_800),
        ("70", "Rush", 21_500),
        ("71", "St. Joseph", 17_500),
        ("72", "Scott", 21_600),
        ("73", "Shelby", 17_000),
        ("74", "Spencer", 8_000),
        ("75", "Starke", 17_100),
        ("76", "Steuben", 19_900),
        ("77", "Sullivan", 17_000),
        ("78", "Switzerland", 14_500),
        ("79", "Tippecanoe", 12_800),
        ("80", "Tipton", 26_000),
        ("81", "Union", 27_500),
        ("82", "Vanderburgh", 12_500),
        ("83", "Vermillion", 15_000),
        ("84", "Vigo", 20_000),
        ("85", "Wabash", 29_000),
        ("86", "Warren", 21_200),
        ("87", "Warrick", 10_000),
        ("88", "Washington", 20_000),
        ("89", "Wayne", 12_500),
        ("90", "Wells", 21_000),
        ("91", "White", 23_200),
        ("92", "Whitley", 16_829),
    ];
    COUNTIES
        .iter()
        .map(|(code, county, rate)| Locality {
            exemption: 100_000,
            second_exemption: 150_000,
            ..percent(
                &format!("IN-{code}"),
                &format!("Indiana: {county} County"),
                "IN",
                *rate,
                *rate,
                "Indiana DOR Departmental Notice #1 (R47, effective 1 October 2026)",
            )
        })
        .collect()
}

/// Michigan city income taxes (Act 284 of 1964; each city's own 2025-2026 forms): resident and
/// nonresident rates and the yearly exemption per allowance. Not built in: Lansing and Muskegon
/// Heights (no official figures found).
fn michigan() -> Vec<Locality> {
    // (code, city, resident, nonresident, exemption dollars, verified)
    const CITIES: &[(&str, &str, i64, i64, i64, bool)] = &[
        ("MI-DETROIT", "Detroit", 24_000, 12_000, 600, true),
        ("MI-ALBION", "Albion", 10_000, 5_000, 600, true),
        ("MI-BATTLE-CREEK", "Battle Creek", 10_000, 5_000, 750, true),
        ("MI-BENTON-HARBOR", "Benton Harbor", 10_000, 5_000, 750, true),
        ("MI-BIG-RAPIDS", "Big Rapids", 10_000, 5_000, 600, true),
        ("MI-EAST-LANSING", "East Lansing", 10_000, 5_000, 600, true),
        ("MI-FLINT", "Flint", 10_000, 5_000, 600, true),
        ("MI-GRAND-RAPIDS", "Grand Rapids", 15_000, 7_500, 600, true),
        ("MI-GRAYLING", "Grayling", 10_000, 5_000, 3_000, true),
        ("MI-HAMTRAMCK", "Hamtramck", 10_000, 5_000, 600, true),
        ("MI-HIGHLAND-PARK", "Highland Park", 20_000, 10_000, 600, true),
        ("MI-HUDSON", "Hudson", 10_000, 5_000, 1_000, true),
        ("MI-IONIA", "Ionia", 10_000, 5_000, 700, true),
        ("MI-JACKSON", "Jackson", 10_000, 5_000, 600, true),
        ("MI-LAPEER", "Lapeer", 10_000, 5_000, 600, true),
        ("MI-MUSKEGON", "Muskegon", 10_000, 5_000, 600, true),
        ("MI-PONTIAC", "Pontiac", 10_000, 5_000, 600, true),
        ("MI-PORT-HURON", "Port Huron", 10_000, 5_000, 600, true),
        ("MI-PORTLAND", "Portland", 10_000, 5_000, 1_000, true),
        ("MI-SAGINAW", "Saginaw", 15_000, 7_500, 750, false),
        ("MI-SPRINGFIELD", "Springfield", 10_000, 5_000, 750, false),
        ("MI-WALKER", "Walker", 10_000, 5_000, 600, true),
    ];
    CITIES
        .iter()
        .map(|(code, city, resident, nonresident, exemption, verified)| Locality {
            exemption: exemption * 100,
            verified: *verified,
            ..percent(
                code,
                &format!("{city} (MI) city income tax"),
                "MI",
                *resident,
                *nonresident,
                if *code == "MI-DETROIT" {
                    "Michigan Treasury Form 5469, 2026 City of Detroit Income Tax Withholding Guide: 2.4% / 1.2%, $600 per exemption"
                } else {
                    "City income tax forms and withholding guide (Act 284 of 1964); nonresidents only on work in the city, none under 25%"
                },
            )
        })
        .collect()
}

/// Ohio cities (work-place withholding; qualifying wages = Medicare wages less section 125,
/// ORC 718.01(R)), Kentucky, Alabama occupational taxes and West Virginia city service fees.
fn others() -> Vec<Locality> {
    let mut out = Vec::new();
    for (code, city, rate) in [
        ("OH-COLUMBUS", "Columbus", 25_000),
        ("OH-CLEVELAND", "Cleveland", 25_000),
        ("OH-CINCINNATI", "Cincinnati", 18_000),
        ("OH-TOLEDO", "Toledo", 25_000),
        ("OH-AKRON", "Akron", 25_000),
        ("OH-DAYTON", "Dayton", 25_000),
        ("OH-PARMA", "Parma", 25_000),
        ("OH-CANTON", "Canton", 25_000),
        ("OH-YOUNGSTOWN", "Youngstown", 27_500),
        ("OH-LORAIN", "Lorain", 25_000),
    ] {
        out.push(percent(
            code,
            &format!("{city} (OH) municipal income tax"),
            "OH",
            rate,
            rate,
            "Ohio Department of Taxation, The Finder municipal rate table (2026); withheld for the city where the work is done",
        ));
    }
    let gross = |l: Locality| Locality { includes_401k: true, includes_125: true, ..l };
    out.push(gross(percent(
        "KY-LOUISVILLE",
        "Louisville Metro (KY) occupational tax",
        "KY",
        22_000,
        14_500,
        "Louisville Metro Revenue Commission: residents 2.2% (Metro 1.25% + TARC 0.20% + schools 0.75%), nonresidents 1.45%",
    )));
    out.push(gross(percent(
        "KY-LEXINGTON",
        "Lexington-Fayette (KY) occupational license fee",
        "KY",
        22_500,
        22_500,
        "Lexington-Fayette Urban County Government: 2.25% of gross compensation before pre-tax deductions",
    )));
    out.push(gross(percent(
        "KY-FAYETTE-SCHOOLS",
        "Fayette County Public Schools (KY) occupational tax",
        "KY",
        5_000,
        0,
        "Fayette County Public Schools: 0.5%, residents only",
    )));
    for (code, city, rate) in [
        ("AL-BIRMINGHAM", "Birmingham", 10_000),
        ("AL-BESSEMER", "Bessemer", 10_000),
        ("AL-GADSDEN", "Gadsden", 20_000),
        ("AL-AUBURN", "Auburn", 10_000),
        ("AL-OPELIKA", "Opelika", 10_000),
        ("AL-LEEDS", "Leeds", 10_000),
        ("AL-MIDFIELD", "Midfield", 10_000),
        ("AL-IRONDALE", "Irondale", 5_000),
        ("AL-MACON", "Macon County", 10_000),
    ] {
        out.push(gross(percent(
            code,
            &format!("{city} (AL) occupational tax"),
            "AL",
            rate,
            rate,
            "City ordinance and occupational tax forms: gross pay for work in the city, no deductions",
        )));
    }
    for (code, city, weekly, source) in [
        ("WV-CHARLESTON", "Charleston", 300, "City of Charleston, City Service Fee regulations: $3.00 a week"),
        ("WV-HUNTINGTON", "Huntington", 500, "City of Huntington, Article 772: $5.00 a week"),
        (
            "WV-WHEELING",
            "Wheeling",
            200,
            "City of Wheeling regulations: $2.00 a week (employees on the payroll 30+ days a year)",
        ),
        ("WV-PARKERSBURG", "Parkersburg", 250, "City of Parkersburg, Ordinance 780.04: $2.50 a week"),
        ("WV-MORGANTOWN", "Morgantown", 300, "City of Morgantown letter to employers: $3.00 a week"),
        (
            "WV-FAIRMONT",
            "Fairmont (nonresidents only)",
            200,
            "City of Fairmont regulations: $2.00 a week, nonresident employees only",
        ),
    ] {
        out.push(Locality {
            flat_employee: weekly,
            ..base(code, &format!("{city} (WV) city service fee"), "WV", "WEEKLY_FLAT", source)
        });
    }
    out
}

#[cfg(test)]
mod catalog_tests {
    use super::*;

    fn usd(s: &str) -> Money {
        Money::parse_decimal(s).unwrap_or_else(|| panic!("{s}"))
    }

    fn pay(code: &str, a: Assignment, gross: &str, periods: i64) -> i64 {
        let l = catalog().into_iter().find(|l| l.code == code).expect("locality");
        let p = Plan { locality: l, assignment: a };
        contribution(
            &p,
            usd(gross),
            Money::ZERO,
            Money::ZERO,
            periods,
            "2026-11-06",
            &PayContext::default(),
            false,
            Ytd::default(),
        )
        .expect("contribution")
        .employee
    }

    #[test]
    fn official_examples() {
        // Indiana DN #1: weekly $800 with 5 personal, 4 dependent and 2 adopted ($3,000 = 2 x $1,500)
        // exemptions: $326.92 off, 473.08 x 1% (a 1% county) = 4.73.
        let one_percent =
            Locality { resident_rate: 10_000, ..catalog().into_iter().find(|l| l.code == "IN-43").expect("Kosciusko") };
        let p = Plan {
            locality: one_percent,
            assignment: Assignment { resident: true, allowances: 5, second_allowances: 8, ..Default::default() },
        };
        let c = contribution(
            &p,
            usd("800"),
            Money::ZERO,
            Money::ZERO,
            52,
            "2026-11-06",
            &PayContext::default(),
            false,
            Ytd::default(),
        )
        .expect("contribution");
        assert_eq!(c.employee, 473);
        // Detroit Form 5469: $200 a week, 3 exemptions: 165.38 x 2.4% = 3.97.
        assert_eq!(
            pay("MI-DETROIT", Assignment { resident: true, allowances: 3, ..Default::default() }, "200", 52),
            397
        );
        // Walker: $110 a week, 3 exemptions: 75.38 x 1% = 0.75.
        assert_eq!(pay("MI-WALKER", Assignment { resident: true, allowances: 3, ..Default::default() }, "110", 52), 75);
        // Fairmont's own per-paycheck amounts for $2 a week: 4.00 / 4.33 / 8.67.
        assert_eq!(pay("WV-FAIRMONT", Assignment::default(), "1000", 26), 400);
        assert_eq!(pay("WV-FAIRMONT", Assignment::default(), "1000", 24), 433);
        assert_eq!(pay("WV-FAIRMONT", Assignment::default(), "1000", 12), 867);
        // Leeds: "$3,000 ... you would withhold $30".
        assert_eq!(pay("AL-LEEDS", Assignment::default(), "3000", 12), 3_000);
        // Louisville: nonresident 1.45%, resident 2.2%.
        assert_eq!(pay("KY-LOUISVILLE", Assignment::default(), "2000", 26), 2_900);
        assert_eq!(pay("KY-LOUISVILLE", Assignment { resident: true, ..Default::default() }, "2000", 26), 4_400);
    }

    #[test]
    fn codes_are_unique_and_valid() {
        let all = catalog();
        let mut seen = std::collections::BTreeSet::new();
        for l in &all {
            assert!(seen.insert(l.code.clone()), "{} twice", l.code);
            assert!(
                l.code.len() <= 20 && l.code.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-'),
                "{}",
                l.code
            );
        }
        assert_eq!(all.iter().filter(|l| l.state == "IN").count(), 92);
    }
}

fn at(wages: i128, ppm: i64) -> i128 {
    (2 * wages.max(0) * i128::from(ppm) + PPM) / (2 * PPM)
}

fn frequency(periods: i64) -> Option<usize> {
    match periods {
        52 => Some(0),
        26 => Some(1),
        24 => Some(2),
        12 => Some(3),
        _ => None,
    }
}

fn need_frequency(name: &str, periods: i64) -> AppResult<usize> {
    frequency(periods).ok_or_else(|| {
        AppError::bad_request(format!("{name} needs a weekly, biweekly, semimonthly or monthly payroll"))
    })
}

/// New York City, NYS-50-T-NYC (1/26) Method II: wages − Table A (deduction + exemptions);
/// (net − column 3) × column 4, half-up, + column 5 (the single and married tables are alike).
fn nyc(wages: i128, periods: i64, married: bool, allowances: i64) -> AppResult<i128> {
    let f = need_frequency("New York City withholding", periods)?;
    let deduction = if married { [10_575, 21_150, 22_915, 45_830][f] } else { [9_615, 19_230, 20_835, 41_670][f] };
    let net = wages - deduction - i128::from(allowances.max(0)) * [1_925, 3_850, 4_165, 8_330][f];
    if net <= 0 {
        return Ok(0);
    }
    let col3: [i128; 6] = [
        [0, 154, 167, 288, 481, 1_154],
        [0, 308, 334, 577, 962, 2_308],
        [0, 333, 362, 625, 1_042, 2_500],
        [0, 667, 725, 1_250, 2_083, 5_000],
    ][f];
    let col5: [i128; 6] = [
        [0, 315, 354, 746, 1_506, 4_300],
        [0, 631, 708, 1_492, 3_012, 8_600],
        [0, 683, 767, 1_617, 3_263, 9_317],
        [0, 1_367, 1_533, 3_233, 6_525, 18_633],
    ][f];
    let rates = [20_500, 28_000, 32_500, 39_500, 41_500, 42_500];
    let line = col3.iter().rposition(|c| net >= c * 100).unwrap_or(0);
    Ok(col5[line] + at(net - col3[line] * 100, rates[line]))
}

/// Yonkers nonresident earnings tax, NYS-50-T-Y (1/26) Method VII: 0.50% of wages less the
/// exclusion for the wage line.
fn yonkers_nonresident(wages: i128, periods: i64) -> AppResult<i128> {
    let f = need_frequency("Yonkers withholding", periods)?;
    let (breaks, exclusions): ([i128; 4], [i128; 4]) = [
        ([77, 192, 385, 577], [58, 38, 19, 0]),
        ([154, 385, 769, 1_154], [115, 77, 38, 0]),
        ([167, 417, 833, 1_250], [125, 83, 42, 0]),
        ([333, 833, 1_667, 2_500], [250, 167, 83, 0]),
    ][f];
    if wages < breaks[0] * 100 {
        return Ok(0);
    }
    let line = breaks.iter().rposition(|b| wages >= b * 100).unwrap_or(0);
    Ok(at(wages - exclusions[line] * 100, 5_000))
}

/// The Oregon federal-tax deduction of the Metro / Multnomah formulas: annual federal income
/// tax withheld, at most $8,750, phased out from $250,000 of annual wages to nothing at $290,000.
fn oregon_federal_deduction(annual_wages: i128, annual_federal: i128) -> i128 {
    let cap = match annual_wages {
        w if w < 25_000_000 => 875_000,
        w if w < 26_000_000 => 700_000,
        w if w < 27_000_000 => 525_000,
        w if w < 28_000_000 => 350_000,
        w if w < 29_000_000 => 175_000,
        _ => 0,
    };
    annual_federal.clamp(0, cap)
}

/// Annual cents → this period, half-up.
fn per_period(annual: i128, periods: i64) -> i128 {
    let d = i128::from(periods.max(1));
    (2 * annual.max(0) + d) / (2 * d)
}

/// One locality's contribution on a paycheck.
#[allow(clippy::too_many_arguments)]
pub fn contribution(
    p: &Plan,
    gross: Money,
    pretax_401k: Money,
    pretax_125: Money,
    periods: i64,
    pay_date: &str,
    pay: &PayContext,
    married: bool,
    ytd: Ytd,
) -> AppResult<Contribution> {
    let l = &p.locality;
    let a = &p.assignment;
    let mut wages = i128::from(gross.cents());
    if !l.includes_401k {
        wages -= i128::from(pretax_401k.cents());
    }
    if !l.includes_125 {
        wages -= i128::from(pretax_125.cents());
    }
    let wages = wages.max(0);
    let periods_i = i128::from(periods.max(1));
    let (mut employee, mut employer): (i128, i128) = match l.method.as_str() {
        "PERCENT" => {
            // The year's exemptions / pay periods, half-up (Indiana DN #1, Detroit Form 5469).
            let year = i128::from(a.allowances.max(0)) * i128::from(l.exemption)
                + i128::from(a.second_allowances.max(0)) * i128::from(l.second_exemption);
            let off = (2 * year + periods_i) / (2 * periods_i);
            let rate = if a.resident { l.resident_rate } else { l.nonresident_rate };
            (at(wages - off, rate), at(wages, l.employer_rate))
        }
        "MONTHLY_FLAT" => {
            // Due once a month: on the first pay date of the month, when the month's wages
            // (this paycheck × periods ÷ 12) reach the threshold.
            let day: i64 = pay_date.get(8..10).and_then(|d| d.parse().ok()).unwrap_or(1);
            let first_of_month = match periods {
                52 => day <= 7,
                26 => day <= 14,
                24 => day <= 15,
                _ => true,
            };
            let monthly_wages = wages * periods_i / 12;
            if first_of_month && monthly_wages >= i128::from(l.monthly_threshold) {
                (i128::from(l.flat_employee), i128::from(l.flat_employer))
            } else {
                (0, 0)
            }
        }
        "WEEKLY_FLAT" => {
            // A weekly fee: weekly amount x 52 / pay periods, half-up (West Virginia city fees).
            let per = |weekly: i64| (2 * i128::from(weekly) * 52 + periods_i) / (2 * periods_i);
            (per(l.flat_employee), per(l.flat_employer))
        }
        "PERIOD_FLAT" => {
            // Spread over the pay periods, rounded down to the cent (the PA LST rule).
            (i128::from(l.flat_employee) / periods_i, i128::from(l.flat_employer) / periods_i)
        }
        _ => {
            let annual = wages * periods_i;
            let federal = i128::from(pay.federal_income_tax.cents().max(0)) * periods_i;
            match l.formula.as_deref().unwrap_or_default() {
                "NYC-2026" if a.resident => (nyc(wages, periods, married, a.allowances)?, 0),
                "NYC-2026" => (0, 0),
                "YONKERS-RES-2026" if a.resident => {
                    let cert = StateEmployee { married, allowances: a.allowances, ..Default::default() };
                    let nys = crate::us_state_formulas::compute(
                        "NY-2026",
                        Money::from_cents(i64::try_from(wages).unwrap_or(i64::MAX)),
                        periods,
                        &cert,
                        pay,
                    )?;
                    (at(i128::from(nys.cents()), 167_500), 0)
                }
                "YONKERS-RES-2026" => (0, 0),
                "YONKERS-NR-2026" => (yonkers_nonresident(wages, periods)?, 0),
                "PHL-2026" => {
                    let new = pay_date >= "2026-07-01";
                    let rate = match (a.resident, new) {
                        (true, false) => 37_400,
                        (true, true) => 37_350,
                        (false, false) => 34_300,
                        (false, true) => 34_250,
                    };
                    (at(wages, rate), 0)
                }
                "METRO-SHS-2026" => {
                    let taxable = annual - oregon_federal_deduction(annual, federal) - 582_000 - 20_000_000;
                    (per_period(at(taxable, 10_000), periods), 0)
                }
                "MULT-PFA-2026" => {
                    let taxable = annual - oregon_federal_deduction(annual, federal) - 582_000;
                    let low = (taxable.min(40_000_000) - 20_000_000).max(0);
                    let high = (taxable - 40_000_000).max(0);
                    (per_period(at(low, 15_000) + at(high, 30_000), periods), 0)
                }
                other => return Err(AppError::bad_request(format!("Unknown local tax formula \"{other}\""))),
            }
        }
    };
    if a.exempt {
        employee = 0;
    } else if employee > 0 || l.method != "MONTHLY_FLAT" {
        employee += i128::from(a.extra.max(0));
    }
    if let Some(cap) = l.employee_year_cap {
        employee = employee.min((i128::from(cap) - i128::from(ytd.employee)).max(0));
    }
    employer = employer.max(0);
    let cents = |x: i128| i64::try_from(x).map_err(|_| AppError::bad_request("Amount is too large"));
    Ok(Contribution {
        id: l.code.clone(),
        state: l.state.clone(),
        name: l.name.clone(),
        wages: cents(wages)?,
        employer_wages: if employer > 0 { cents(wages)? } else { 0 },
        employee: cents(employee)?,
        employer: cents(employer)?,
    })
}

/// Every locality on a paycheck; within a group only the largest employee amount is kept.
#[allow(clippy::too_many_arguments)]
pub fn contributions(
    plans: &[Plan],
    gross: Money,
    pretax_401k: Money,
    pretax_125: Money,
    periods: i64,
    pay_date: &str,
    pay: &PayContext,
    married: bool,
    ytd: &BTreeMap<String, Ytd>,
) -> AppResult<Vec<Contribution>> {
    let mut out: Vec<(Option<String>, Contribution)> = Vec::new();
    for p in plans {
        let y = ytd.get(&p.locality.code).copied().unwrap_or_default();
        let c = contribution(p, gross, pretax_401k, pretax_125, periods, pay_date, pay, married, y)?;
        if c.employee == 0 && c.employer == 0 {
            continue;
        }
        match &p.locality.group {
            Some(g) => match out.iter_mut().find(|(og, _)| og.as_ref() == Some(g)) {
                Some((_, kept)) if kept.employee >= c.employee => {}
                Some(slot) => slot.1 = c,
                None => out.push((Some(g.clone()), c)),
            },
            None => out.push((None, c)),
        }
    }
    Ok(out.into_iter().map(|(_, c)| c).collect())
}

// ─── Storage ────────────────────────────────────────────────────────────────

fn row_to_locality(data: &str) -> AppResult<Locality> {
    let v: Value = serde_json::from_str(data).map_err(|_| AppError::internal("locality data"))?;
    Locality::from_json(&v).ok_or_else(|| AppError::internal("locality data"))
}

/// The company's localities, by code.
pub async fn list(conn: &mut sqlx::SqliteConnection) -> AppResult<Vec<Locality>> {
    let rows: Vec<(String,)> =
        sqlx::query_as(r#"SELECT "data" FROM "us_localities" ORDER BY "code""#).fetch_all(&mut *conn).await?;
    rows.iter().map(|(d,)| row_to_locality(d)).collect()
}

pub async fn get(conn: &mut sqlx::SqliteConnection, code: &str) -> AppResult<Option<Locality>> {
    let row: Option<(String,)> = sqlx::query_as(r#"SELECT "data" FROM "us_localities" WHERE "code" = ?1"#)
        .bind(code)
        .fetch_optional(&mut *conn)
        .await?;
    row.map(|(d,)| row_to_locality(&d)).transpose()
}

/// The localities screen: the company's list and the built-in ones it can add.
pub async fn overview(db: &database::Db) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    let mine = list(&mut c).await?;
    Ok(json!({
        "localities": mine.iter().map(Locality::to_json).collect::<Vec<_>>(),
        "catalog": catalog().iter().map(Locality::to_json).collect::<Vec<_>>(),
    }))
}

/// Saves a locality. A built-in formula is kept from the catalog (the company can't make one up).
pub async fn put(db: &database::Db, mut l: Locality) -> AppResult<Value> {
    if l.method == "FORMULA" {
        let built = catalog()
            .into_iter()
            .find(|c| c.formula.is_some() && c.formula == l.formula)
            .ok_or_else(|| AppError::bad_request("Only the built-in localities use a formula"))?;
        l.formula = built.formula;
    } else {
        l.formula = None;
    }
    let mut tx = db.begin().await?;
    if let Some(id) = &l.account_id {
        let a = ledger::accounts::find_one(&mut tx, id).await?;
        if !a.is_active || a.account_type != ledger::accounts::AccountType::Liability || a.currency.is_some() {
            return Err(AppError::bad_request(format!(
                "The local tax account must be an active liability account in the home currency; {} {} is not",
                a.code, a.name
            )));
        }
    }
    sqlx::query(
        r#"INSERT INTO "us_localities" ("code","name","state","data") VALUES (?1,?2,?3,?4)
           ON CONFLICT ("code") DO UPDATE SET "name" = excluded."name", "state" = excluded."state",
             "data" = excluded."data", "updatedAt" = strftime('%Y-%m-%dT%H:%M:%fZ','now')"#,
    )
    .bind(&l.code)
    .bind(&l.name)
    .bind(&l.state)
    .bind(l.to_json().to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(l.to_json())
}

/// Removes a locality no employee is withheld for.
pub async fn delete(db: &database::Db, code: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let used: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM "employee_us_tax" t, json_each(t."localTaxes") j WHERE json_extract(j."value", '$.code') = ?1"#,
    )
    .bind(code)
    .fetch_one(&mut *tx)
    .await?;
    if used > 0 {
        return Err(AppError::bad_request(format!(
            "{code} is withheld for {used} employee(s) — take it off their US tax details first"
        )));
    }
    let done = sqlx::query(r#"DELETE FROM "us_localities" WHERE "code" = ?1"#).bind(code).execute(&mut *tx).await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Locality {code} not found")));
    }
    tx.commit().await?;
    Ok(())
}

/// The plans for an employee's localities on a pay date (localities the company has since
/// removed are skipped).
pub async fn plans_for(conn: &mut sqlx::SqliteConnection, assignments: &[Assignment]) -> AppResult<Vec<Plan>> {
    let mut out = Vec::new();
    for a in assignments {
        if let Some(l) = get(conn, &a.code).await? {
            out.push(Plan { locality: l, assignment: a.clone() });
        }
    }
    Ok(out)
}

/// What the employee's other posted / paid runs paid in `year` counted for each locality.
pub async fn ytd(
    conn: &mut sqlx::SqliteConnection,
    employee_id: &str,
    year: i32,
    except_run: &str,
) -> AppResult<BTreeMap<String, Ytd>> {
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        r#"SELECT json_extract(j."value", '$.id'), COALESCE(SUM(json_extract(j."value", '$.wages')),0),
                  COALESCE(SUM(json_extract(j."value", '$.employerWages')),0),
                  COALESCE(SUM(json_extract(j."value", '$.employee')),0)
           FROM "us_payroll_lines" l JOIN "us_payroll_runs" r ON r."id" = l."runId", json_each(l."local") j
           WHERE l."employeeId" = ?1 AND r."status" IN ('POSTED','PAID') AND r."id" <> ?2
             AND CAST(substr(r."payDate", 1, 4) AS INTEGER) = ?3
           GROUP BY json_extract(j."value", '$.id')"#,
    )
    .bind(employee_id)
    .bind(except_run)
    .bind(year)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, wages, employer_wages, employee)| (id, Ytd { wages, employer_wages, employee }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(s: &str) -> Money {
        Money::parse_decimal(s).unwrap_or_else(|| panic!("{s}"))
    }

    fn find(code: &str) -> Locality {
        catalog().into_iter().find(|l| l.code == code).expect("locality")
    }

    fn run(
        l: Locality,
        a: Assignment,
        gross: &str,
        periods: i64,
        date: &str,
        married: bool,
        federal: &str,
    ) -> (i64, i64) {
        let p = Plan { locality: l, assignment: a };
        let pay = PayContext { federal_income_tax: usd(federal), fica: Money::ZERO, fica_ytd: Money::ZERO };
        let c = contribution(&p, usd(gross), Money::ZERO, Money::ZERO, periods, date, &pay, married, Ytd::default())
            .expect("contribution");
        (c.employee, c.employer)
    }

    fn resident(allowances: i64) -> Assignment {
        Assignment { code: String::new(), resident: true, allowances, ..Default::default() }
    }

    #[test]
    fn new_york_city_examples() {
        // NYS-50-T-NYC (1/26) p.27.
        let nyc = || find("NYC");
        assert_eq!(run(nyc(), resident(3), "400", 52, "2026-11-06", false, "0").0, 611);
        assert_eq!(run(nyc(), resident(1), "5000", 24, "2026-11-06", false, "0").0, 18_880);
        assert_eq!(run(nyc(), resident(3), "50000", 12, "2026-11-06", false, "0").0, 207_050);
        assert_eq!(run(nyc(), resident(4), "400", 52, "2026-11-06", true, "0").0, 517);
        assert_eq!(run(nyc(), resident(3), "5000", 24, "2026-11-06", true, "0").0, 18_437);
        assert_eq!(run(nyc(), resident(3), "50000", 12, "2026-11-06", true, "0").0, 206_873);
        // Nonresidents owe nothing.
        assert_eq!(
            run(nyc(), Assignment { allowances: 3, ..Default::default() }, "400", 52, "2026-11-06", false, "0").0,
            0
        );
    }

    #[test]
    fn yonkers_examples() {
        // NYS-50-T-Y (1/26): 16.75% of the NYS amount — weekly $400 single 3 → 8.01 → 1.34; monthly
        // $50,000 married 3 → 3,622.09 → 606.70.
        assert_eq!(run(find("YONKERS"), resident(3), "400", 52, "2026-11-06", false, "0").0, 134);
        assert_eq!(run(find("YONKERS"), resident(3), "50000", 12, "2026-11-06", true, "0").0, 60_670);
        // Nonresident: weekly $200 → (200 − 38) × 0.5% = 0.81; semimonthly $400 → 1.38; weekly $75 → 0.
        let nr = Assignment::default;
        assert_eq!(run(find("YONKERS-NR"), nr(), "200", 52, "2026-11-06", false, "0").0, 81);
        assert_eq!(run(find("YONKERS-NR"), nr(), "400", 24, "2026-11-06", false, "0").0, 138);
        assert_eq!(run(find("YONKERS-NR"), nr(), "75", 52, "2026-11-06", false, "0").0, 0);
    }

    #[test]
    fn philadelphia_and_oregon() {
        // Philadelphia: resident 3.74% before July 2026, 3.735% after; nonresident 3.43% / 3.425%.
        assert_eq!(run(find("PHL"), resident(0), "2000", 26, "2026-03-06", false, "0").0, 7_480);
        assert_eq!(run(find("PHL"), resident(0), "2000", 26, "2026-11-06", false, "0").0, 7_470);
        assert_eq!(run(find("PHL"), Assignment::default(), "2000", 26, "2026-11-06", false, "0").0, 6_850);
        // Metro SHS: $300,000 a year, federal deduction 0 → 94,180 × 1% = 941.80 → monthly 78.48.
        assert_eq!(run(find("METRO-SHS"), resident(0), "25000", 12, "2026-11-06", false, "5000").0, 7_848);
        // Multnomah PFA: $300,000 → 1,412.70 a year; $450,000 → 4,325.40 a year (monthly 117.73, 360.45).
        assert_eq!(run(find("MULT-PFA"), resident(0), "25000", 12, "2026-11-06", false, "5000").0, 11_773);
        assert_eq!(run(find("MULT-PFA"), resident(0), "37500", 12, "2026-11-06", false, "9000").0, 36_045);
        // Under $200,000 a year neither applies.
        assert_eq!(run(find("METRO-SHS"), resident(0), "10000", 12, "2026-11-06", false, "1500").0, 0);
    }

    #[test]
    fn flat_taxes() {
        // Greenwood Village: $2 + $2 on the first pay date of the month when $250+ is earned.
        assert_eq!(run(find("GV-OPT"), Assignment::default(), "2000", 26, "2026-11-06", false, "0"), (200, 200));
        assert_eq!(run(find("GV-OPT"), Assignment::default(), "2000", 26, "2026-11-20", false, "0"), (0, 0));
        assert_eq!(run(find("GV-OPT"), Assignment::default(), "100", 26, "2026-11-06", false, "0"), (0, 0));
        // PA LST $52 a year, biweekly: 52 / 26 = 2.00; weekly 52 / 52 = 1.00; $47 monthly → 3.91 (down).
        let lst = |amount: i64| Locality { flat_employee: amount, ..find("PA-LST") };
        assert_eq!(run(lst(5_200), Assignment::default(), "2000", 26, "2026-11-06", false, "0").0, 200);
        assert_eq!(run(lst(4_700), Assignment::default(), "2000", 12, "2026-11-06", false, "0").0, 391);
    }

    #[test]
    fn pennsylvania_eit_takes_the_higher_rate() {
        let eit = |code: &str, resident: i64, nonresident: i64| Locality {
            code: code.into(),
            resident_rate: resident,
            nonresident_rate: nonresident,
            ..find("PA-EIT")
        };
        let plans = vec![
            Plan {
                locality: eit("HOME", 10_000, 10_000),
                assignment: Assignment { code: "HOME".into(), resident: true, ..Default::default() },
            },
            Plan {
                locality: eit("WORK", 15_000, 12_000),
                assignment: Assignment { code: "WORK".into(), resident: false, ..Default::default() },
            },
        ];
        let c = contributions(
            &plans,
            usd("2000"),
            Money::ZERO,
            Money::ZERO,
            26,
            "2026-11-06",
            &PayContext::default(),
            false,
            &BTreeMap::new(),
        )
        .expect("contributions");
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].id.as_str(), c[0].employee), ("WORK", 2_400));
    }

    #[test]
    fn catalog_round_trips() {
        for l in catalog() {
            assert_eq!(Locality::from_json(&l.to_json()).as_ref(), Some(&l), "{}", l.code);
        }
    }
}
