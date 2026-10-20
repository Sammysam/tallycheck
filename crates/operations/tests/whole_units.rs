//! D29: a currency without decimals. Its own test binary, because the whole-unit switch is
//! process-wide and would otherwise change the other tests' arithmetic mid-run.

use app_core::money::{self, Money};
use operations::tax;

fn m(s: &str) -> Money {
    Money::parse_decimal(s).unwrap_or_default()
}

#[test]
fn yen_amounts_line_totals_and_tax_are_whole_units() -> app_core::AppResult<()> {
    money::set_currency("JPY");
    assert!(money::whole_units());
    // 2.5 × ¥999 = ¥2,497.5 → ¥2,498.
    assert_eq!(Money::line_amount(m("2.5"), m("999")), Some(m("2498")));
    // 8.875% of ¥1,000 = ¥88.75 → ¥89; 10% included in ¥1,100 = ¥100.
    assert_eq!(tax::tax_of(m("1000"), 88_750, false)?, m("89"));
    assert_eq!(tax::split(m("1100"), 100_000, true)?, (m("1000"), m("100"), m("1100")));
    // Every part of a group is whole too, and the parts still add up to the tax in the price.
    let parts = tax::split_parts(m("1077"), &vec![("a".into(), 60_000, false), ("b".into(), 17_500, false)], true)?;
    assert!(parts.iter().all(|p| p.amount.fits_currency()));
    assert_eq!(parts.iter().map(|p| p.amount.cents()).sum::<i64>(), 7_700, "1077 holds 77 of tax on a net of 1000");
    assert!(!m("10.5").fits_currency());

    money::set_currency("LKR");
    assert!(!money::whole_units());
    assert_eq!(Money::line_amount(m("2.5"), m("999")), Some(m("2497.5")));
    Ok(())
}
