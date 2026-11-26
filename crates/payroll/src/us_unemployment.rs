//! D68, step 5 of the US state payroll plan: each state's 2026 unemployment (SUTA) figures as
//! defaults. When a company registers a state, the screen fills in that state's wage base and
//! new-employer rate and any employer charges billed on the same wages besides the rate (for
//! example California's ETT), so a new company is right from its first pay run; it replaces
//! them with the figures on its own rate notice. Nothing here changes a registration by itself.
//!
//! `verified` is false where the state's own page could only be read through a search excerpt;
//! the screen then asks the company to check the figure. Employee unemployment contributions
//! (Alaska, New Jersey, Pennsylvania) are programs in [`crate::us_leave`] (kind UI).

use serde_json::{json, Value};

/// A charge billed on the state's unemployment wages besides the assigned rate.
#[derive(Debug, Clone, Copy)]
pub struct Surcharge {
    pub name: &'static str,
    /// ppm.
    pub rate: i64,
    pub note: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct StateUi {
    pub state: &'static str,
    /// Dollars per employee per calendar year.
    pub wage_base: i64,
    /// ppm; None where the state sets new employers' rates by industry or did not publish one.
    pub new_employer_rate: Option<i64>,
    pub verified: bool,
    pub surcharges: &'static [Surcharge],
    pub note: &'static str,
    pub source: &'static str,
}

const fn s(state: &'static str, wage_base: i64, new_employer_rate: Option<i64>, source: &'static str) -> StateUi {
    StateUi { state, wage_base, new_employer_rate, verified: true, surcharges: &[], note: "", source }
}

/// The 2026 figures (each state's own publication, named in `source`).
pub const STATES: &[StateUi] = &[
    StateUi { note: "Includes the 0.06% Employment Security Enhancement Assessment.", ..s("AL", 8_000, Some(27_000), "Alabama Department of Workforce, Employers FAQ") },
    StateUi {
        note: "Employees also pay 0.5% on the same wages (built in as Alaska employee UI).",
        ..s("AK", 54_200, Some(10_000), "Alaska DOLWD, 2026 Unemployment Insurance Tax Rates and 2026 Industry Rates")
    },
    s("AZ", 8_000, Some(20_000), "Arizona DES, UIT-0603A Tax Rate Chart for calendar year 2026"),
    s("AR", 7_000, Some(20_000), "Arkansas Division of Reemployment, 2026 Tax Rates and Employer Handbook"),
    StateUi {
        surcharges: &[Surcharge { name: "Employment Training Tax (ETT)", rate: 1_000, note: "On the same $7,000; shown on the rate notice (DE 2088)" }],
        ..s("CA", 7_000, Some(34_000), "California EDD, Rates and Withholding (2026)")
    },
    StateUi {
        note: "Combined rate incl. the support and solvency surcharges; heavy construction 6.285%.",
        ..s("CO", 30_600, Some(30_500), "Colorado CDLE, Introductory Rates and Premiums (2026)")
    },
    StateUi { note: "The 1.0% fund solvency tax is added to charged rates; check that your notice's rate includes it.", ..s("CT", 27_000, Some(19_000), "Connecticut DOL, Unemployment Tax Rate for Calendar Year 2026") },
    StateUi {
        surcharges: &[
            Surcharge { name: "Operations and technology assessment", rate: 2_000, note: "19 Del. C. § 3350B, 2024–2026" },
            Surcharge { name: "Training tax", rate: 1_100, note: "19 Del. C. § 3401" },
        ],
        ..s("DE", 14_500, Some(10_000), "Delaware DOL, Employer Services merit rate table; 19 Del. C. §§ 3350B, 3401")
    },
    StateUi {
        surcharges: &[Surcharge { name: "Administrative funding assessment", rate: 2_000, note: "D.C. Code § 51-103(m), paid separately" }],
        ..s("DC", 9_000, Some(27_000), "DC DOES employer notice for 2026; D.C. Code § 51-103")
    },
    s("FL", 7_000, Some(27_000), "Florida Department of Revenue, Reemployment Tax Rate Information"),
    StateUi { note: "Total rate incl. the 0.06% administrative assessment (through 2026).", ..s("GA", 9_500, Some(27_000), "Georgia DOL, UI Tax Reporting and Liability Information for Employers") },
    StateUi {
        surcharges: &[Surcharge { name: "Employment and training assessment", rate: 100, note: "Listed as its own rate; check your notice" }],
        ..s("HI", 64_500, Some(24_000), "Hawaii DLIR, Tax Rate Schedule (Schedule C) for 2026")
    },
    StateUi { note: "Standard rate incl. 0.030% workforce development.", ..s("ID", 58_300, Some(10_000), "Idaho DOL, Rate Class Array 2026 and Administrative Order 682") },
    StateUi { note: "NAICS 56 new employers 3.45%.", ..s("IL", 14_250, Some(33_500), "Illinois IDES, EA-50 2026 Contribution Rates") },
    s("IN", 9_500, Some(25_000), "Indiana DWD, New Employer Premium Rate"),
    StateUi { note: "Construction 5.4%.", ..s("IA", 20_400, Some(10_000), "Iowa Workforce Development, UI Employer Handbook 2026") },
    StateUi { note: "Construction 5.55%.", ..s("KS", 15_100, Some(17_500), "Kansas DOL, Employer Services rate table (2026)") },
    StateUi { note: "New construction employers: the highest rate on the schedule.", ..s("KY", 12_000, Some(27_000), "Kentucky Office of Unemployment Insurance, 2026 Employer Guide") },
    StateUi { note: "New-employer rates by industry; enter yours.", ..s("LA", 7_000, None, "Louisiana Workforce Commission, 2026 UI Contribution Rate Table") },
    StateUi { note: "Combined rate incl. CSSF 0.14% and UPAF 0.17%.", ..s("ME", 12_000, Some(25_400), "Maine DOL, Annual Rate Category Array, Rate Year 2026") },
    StateUi { note: "The 2026 new-employer rate is on your notice.", ..s("MD", 8_500, None, "Maryland DOL, Tax Rates and Quarterly Reporting") },
    StateUi {
        surcharges: &[
            Surcharge { name: "EMAC (employer medical assistance contribution)", rate: 3_400, note: "0.12% / 0.24% / 0.34% in years 4 / 5 / 6+ of business; check your notice" },
            Surcharge { name: "Workforce Training Fund", rate: 560, note: "M.G.L. c.151A § 14L" },
        ],
        note: "New construction employers 6.08%; the COVID recovery assessment (not for new employers) is on your notice.",
        ..s("MA", 15_000, Some(24_200), "Massachusetts DUA, 2026 UI & COVID-19 Recovery Assessment Rate Schedule; EMAC")
    },
    StateUi { verified: false, note: "Construction 5.0%.", ..s("MI", 9_500, Some(27_000), "Michigan UIA, Unemployment Insurance Taxes (2026)") },
    StateUi {
        surcharges: &[Surcharge { name: "Workforce development fee", rate: 1_000, note: "Minn. Stat. 116L.20" }],
        note: "New-employer rates by industry; enter yours.",
        ..s("MN", 44_000, None, "Minnesota DEED, Update on 2026 UI Rate Factors")
    },
    StateUi { note: "1.0% / 1.1% / 1.2% in years 1 / 2 / 3+.", ..s("MS", 14_000, Some(10_000), "Mississippi MDES, Unemployment Tax Rates and Employer FAQs") },
    s("MO", 9_000, Some(23_760), "Missouri DES, Tax Rates (2026)"),
    StateUi {
        surcharges: &[Surcharge { name: "Administrative Fund Tax", rate: 1_800, note: "0.13% for eligible classes 01–02" }],
        note: "New-employer rates by industry (1.00%–2.00%); enter yours.",
        ..s("MT", 47_300, None, "Montana DLI, UI Contribution Taxable Wage Base & Rates for 2026")
    },
    StateUi { note: "Construction 5.40%.", ..s("NE", 9_000, Some(12_500), "Nebraska DOL, 2026 Guide to Nebraska's UI Combined Tax Rates") },
    StateUi {
        surcharges: &[Surcharge { name: "Career Enhancement Program", rate: 500, note: "Not for employers at 5.4%" }],
        ..s("NV", 43_700, Some(29_500), "Nevada DETR, UI Information for Employers (2026)")
    },
    StateUi { note: "Net rate for Q1–Q2 2026; New Hampshire sets rates each quarter.", ..s("NH", 14_000, Some(17_000), "New Hampshire NHES, Tax Rate Chart (2026)") },
    StateUi {
        note: "Employees also pay 0.425% on the same wages (built in as New Jersey employee UI).",
        ..s("NJ", 44_800, Some(28_000), "New Jersey DOL, Rate information and Table C (2026)")
    },
    StateUi { note: "New employers: industry average or 1%, whichever is higher.", ..s("NM", 34_800, None, "New Mexico DWS, UI Tax Information (2026)") },
    StateUi { note: "Includes the 0.075% Re-employment Service Fund.", ..s("NY", 17_600, Some(41_000), "New York DOL, Unemployment Insurance Rate Information") },
    s("NC", 34_200, Some(10_000), "North Carolina DES, Tax Rate Information"),
    StateUi { note: "Construction 9.67%.", ..s("ND", 46_600, Some(10_000), "Job Service North Dakota, 2026 UI Tax Rate Schedules") },
    StateUi { note: "Includes the 0.15% technology fee; construction 5.85%.", ..s("OH", 9_000, Some(28_500), "Ohio ODJFS, 2026 Rates") },
    s("OK", 25_000, Some(15_000), "Oklahoma OESC, Important Numbers for 2026"),
    s("OR", 56_700, Some(24_000), "Oregon OED, 2026 tax rate press release (18 Nov 2025)"),
    StateUi {
        note: "Construction 10.5924%. Employees also pay 0.07% of all wages (built in as Pennsylvania employee UC).",
        ..s("PA", 10_000, Some(38_220), "Pennsylvania L&I, UC Solvency Measures in Effect for Calendar Year 2026")
    },
    StateUi { verified: false, note: "Includes the 0.21% Job Development Assessment.", ..s("RI", 30_800, Some(12_100), "Rhode Island DLT, 2026 Tax Rates for UI and TDI") },
    StateUi { verified: false, note: "The 2026 new-employer rate is on your notice.", ..s("SC", 14_000, None, "SC Code § 41-27-380; SC DEW, 2026 Tax Rate Cuts") },
    StateUi {
        surcharges: &[
            Surcharge { name: "Investment fee", rate: 5_500, note: "0.55% for new employers; 0.00%–0.53% by reserve ratio later" },
            Surcharge { name: "Administrative fee", rate: 800, note: "SDCL 61-5-29" },
        ],
        note: "1.20% in year 1, 1.00% in years 2–3; construction 6.00% / 3.00%.",
        ..s("SD", 15_000, Some(12_000), "South Dakota DLR, Reemployment Assistance tax FAQ")
    },
    s("TN", 7_000, Some(27_000), "Tennessee TDLWD, UI Tax Rates"),
    StateUi { verified: false, note: "Or the industry average if higher.", ..s("TX", 9_000, Some(27_000), "Texas Workforce Commission, Your 2026 Tax Rates") },
    StateUi { verified: false, note: "New-employer rates by industry; enter yours.", ..s("UT", 50_700, None, "Utah DWS, Tax Rates FAQ") },
    StateUi { note: "Out-of-state construction: NAICS 236 1.9%, 237 4.0%, 238 2.7%.", ..s("VT", 15_400, Some(10_000), "Vermont DOL, Unemployment Tax Rates (2026)") },
    StateUi { verified: false, note: "Plus any add-ons on your notice.", ..s("VA", 8_000, Some(25_000), "Virginia Employment Commission") },
    StateUi { note: "New employers: 115% of the industry average, at least 1%; enter yours.", ..s("WA", 78_200, None, "Washington ESD, How we determine tax rates (2026)") },
    StateUi {
        note: "SB 1053 cut rates by 7% and added an automation and administration fee from 1 July 2026; check your notice.",
        ..s("WV", 9_500, Some(27_000), "WorkForce West Virginia, Employer Handbook")
    },
    StateUi { note: "3.25% when payroll is $500,000 or more; construction 2.50% / 2.70%.", ..s("WI", 14_000, Some(30_500), "Wisconsin DWD, 2026 Tax Rate Schedule for Employers") },
    StateUi { note: "New-employer rates by industry; enter yours.", ..s("WY", 33_800, None, "Wyoming DWS, Unemployment Taxable Wage Base") },
];

pub fn for_state(state: &str) -> Option<&'static StateUi> {
    STATES.iter().find(|x| x.state == state)
}

/// The defaults, for the registration screen (rates in percent, amounts in dollars).
pub fn catalog() -> Value {
    Value::Array(
        STATES
            .iter()
            .map(|x| {
                json!({
                    "state": x.state,
                    "wageBase": x.wage_base,
                    "newEmployerRate": x.new_employer_rate.map(crate::us_setup::ppm_to_percent),
                    "verified": x.verified,
                    "surcharges": x.surcharges.iter().map(|c| json!({
                        "name": c.name, "rate": crate::us_setup::ppm_to_percent(c.rate), "note": c.note,
                    })).collect::<Vec<_>>(),
                    "note": x.note,
                    "source": x.source,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_once_with_sane_figures() {
        let mut seen = std::collections::BTreeSet::new();
        for x in STATES {
            assert!(seen.insert(x.state), "{} twice", x.state);
            assert!(crate::us_setup::STATES.contains(&x.state), "{}", x.state);
            assert!((7_000..=100_000).contains(&x.wage_base), "{}", x.state);
            assert!(x.new_employer_rate.is_none_or(|r| (1_000..=110_000).contains(&r)), "{}", x.state);
            assert!(x.surcharges.iter().all(|c| (1..=10_000).contains(&c.rate)), "{}", x.state);
            assert!(!x.source.is_empty());
        }
        // Every state and DC.
        assert_eq!(STATES.len(), 51);
    }
}
