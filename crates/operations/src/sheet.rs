//! D37: reads an uploaded CSV or Excel file as rows of text, for the import screen to map.
//!
//! * CSV / TXT: UTF-8 (with or without BOM), UTF-16 with BOM, otherwise Windows-1252 (what
//!   Excel and most bank portals write on Windows). The delimiter is whichever of `,` `;` tab
//!   appears most in the first line.
//! * XLSX / XLSM / XLS / ODS: every worksheet. Dates become `YYYY-MM-DD`; numbers keep the
//!   shortest text that reads back as the same number (so 1234.5 stays "1234.5"), and the import
//!   turns that text into exact cents — money is never computed in floating point.
//!
//! Empty rows are dropped and trailing empty cells trimmed. At most [`MAX_ROWS`] rows per sheet.

use app_core::{AppError, AppResult};
use calamine::{Data, Reader};
use serde::Serialize;
use std::io::Cursor;

pub const MAX_ROWS: usize = 10_000;
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    /// More rows were in the sheet than [`MAX_ROWS`].
    pub truncated: bool,
}

fn unreadable(what: &str) -> AppError {
    AppError::bad_request(format!("Could not read this file as {what}. Save it as CSV or Excel (.xlsx) and try again."))
}

pub fn read(file_name: &str, bytes: &[u8]) -> AppResult<Vec<Sheet>> {
    let ext = file_name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "csv" | "txt" | "tsv" => Ok(vec![read_csv(file_name, bytes)?]),
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" => read_workbook(bytes),
        _ => Err(AppError::bad_request("Choose a CSV or Excel file (.csv, .xlsx, .xls)")),
    }
}

fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode_with_bom_removal(bytes).0.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE.decode_with_bom_removal(bytes).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
    }
}

fn delimiter(text: &str) -> u8 {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    b",;\t".iter().copied().max_by_key(|d| first.bytes().filter(|b| b == d).count()).unwrap_or(b',')
}

fn tidy(mut row: Vec<String>) -> Option<Vec<String>> {
    for c in row.iter_mut() {
        *c = c.trim().to_string();
    }
    while row.last().is_some_and(|c| c.is_empty()) {
        row.pop();
    }
    (!row.is_empty()).then_some(row)
}

fn read_csv(file_name: &str, bytes: &[u8]) -> AppResult<Sheet> {
    let text = decode(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter(&text))
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    let mut truncated = false;
    for record in reader.records() {
        let record = record.map_err(|_| unreadable("CSV"))?;
        if let Some(row) = tidy(record.iter().map(str::to_string).collect()) {
            if rows.len() == MAX_ROWS {
                truncated = true;
                break;
            }
            rows.push(row);
        }
    }
    let name = file_name.rsplit_once('.').map(|(n, _)| n).unwrap_or(file_name).to_string();
    Ok(Sheet { name, rows, truncated })
}

fn cell_text(c: &Data) -> String {
    match c {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) => s.clone(),
        Data::Bool(b) => b.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.0}")
            } else {
                // Shortest round-trip form: the number as it was typed into the cell.
                f.to_string()
            }
        }
        Data::DateTime(d) => match d.as_datetime() {
            Some(dt) if dt.time() == chrono::NaiveTime::MIN => dt.format("%Y-%m-%d").to_string(),
            Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
            None => d.as_f64().to_string(),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
    }
}

fn read_workbook(bytes: &[u8]) -> AppResult<Vec<Sheet>> {
    let mut book =
        calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|_| unreadable("Excel"))?;
    let mut sheets = Vec::new();
    for name in book.sheet_names() {
        let range = book.worksheet_range(&name).map_err(|_| unreadable("Excel"))?;
        let mut rows = Vec::new();
        let mut truncated = false;
        for r in range.rows() {
            if let Some(row) = tidy(r.iter().map(cell_text).collect()) {
                if rows.len() == MAX_ROWS {
                    truncated = true;
                    break;
                }
                rows.push(row);
            }
        }
        sheets.push(Sheet { name, rows, truncated });
    }
    if sheets.is_empty() {
        return Err(unreadable("Excel"));
    }
    Ok(sheets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_with_semicolons_bom_and_quotes() {
        let bytes = b"\xEF\xBB\xBFDate;Details;Amount\n01/10/2026;\"Rent; October\";-50,000.00\n\n;;\n";
        let s = read("stmt.csv", bytes).expect("csv reads");
        assert_eq!(s[0].name, "stmt");
        assert_eq!(
            s[0].rows,
            vec![vec!["Date", "Details", "Amount"], vec!["01/10/2026", "Rent; October", "-50,000.00"]]
        );
    }

    #[test]
    fn windows_1252_text_is_decoded() {
        let s = read("a.csv", b"Caf\xE9,10\n").expect("csv reads");
        assert_eq!(s[0].rows[0][0], "Café");
    }

    #[test]
    fn unknown_extension_is_refused() {
        assert!(read("a.pdf", b"%PDF").is_err());
        assert!(read("a.xlsx", b"not a zip").is_err());
    }

    #[test]
    fn float_cells_keep_their_typed_digits() {
        assert_eq!(cell_text(&Data::Float(1234.5)), "1234.5");
        assert_eq!(cell_text(&Data::Float(18337.13)), "18337.13");
        assert_eq!(cell_text(&Data::Float(-50000.0)), "-50000");
        assert_eq!(cell_text(&Data::Int(7)), "7");
    }
}
