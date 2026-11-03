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

/// At most this many columns are read from a sheet; cells further right are ignored.
pub const MAX_COLUMNS: u32 = 200;
/// A workbook is a zip of XML files: all of them together may unpack to at most this much.
const MAX_UNZIPPED_BYTES: u64 = 100 * 1024 * 1024;
/// Formats read as a whole grid (ODS) may span at most this many cells, empty ones included.
const MAX_GRID_CELLS: u64 = 5_000_000;

fn too_big() -> AppError {
    AppError::bad_request(
        "This spreadsheet is too large to import. Copy the rows you need into a new sheet, or save it as CSV.",
    )
}

/// XLSX / XLSB / ODS are zip files: refuse one that unpacks to more than [`MAX_UNZIPPED_BYTES`]
/// before any of it is read (a few kilobytes can claim gigabytes).
fn check_zip(bytes: &[u8]) -> AppResult<()> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| unreadable("Excel"))?;
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|_| unreadable("Excel"))?;
        total = total.saturating_add(f.size());
        if total > MAX_UNZIPPED_BYTES {
            return Err(too_big());
        }
    }
    Ok(())
}

/// Rows from cells taken one by one: never more than [`MAX_ROWS`] non-empty rows or
/// [`MAX_COLUMNS`] columns are kept, whatever positions the file claims, so a cell at the far
/// corner of the sheet cannot make the reader allocate the whole grid.
struct Collector {
    rows: std::collections::BTreeMap<u32, Vec<(u32, String)>>,
    truncated: bool,
}

impl Collector {
    fn new() -> Self {
        Collector { rows: std::collections::BTreeMap::new(), truncated: false }
    }

    /// Takes one cell; `Ok(false)` once enough rows have been read. A value beyond
    /// [`MAX_COLUMNS`] refuses the file.
    fn push(&mut self, (row, col): (u32, u32), value: &Data) -> AppResult<bool> {
        let text = cell_text(value);
        if text.trim().is_empty() {
            return Ok(true);
        }
        if col >= MAX_COLUMNS {
            return Err(AppError::bad_request(format!(
                "This sheet has values beyond column {MAX_COLUMNS}. Copy the columns you need into a new sheet and try again."
            )));
        }
        if !self.rows.contains_key(&row) && self.rows.len() == MAX_ROWS {
            self.truncated = true;
            // Rows normally arrive in order: anything after the limit is beyond it.
            if self.rows.last_key_value().is_some_and(|(last, _)| row > *last) {
                return Ok(false);
            }
            self.rows.pop_last();
        }
        self.rows.entry(row).or_default().push((col, text));
        Ok(true)
    }

    fn finish(self, name: String) -> Sheet {
        let first_col = self.rows.values().flatten().map(|(c, _)| *c).min().unwrap_or(0);
        let rows = self
            .rows
            .into_values()
            .filter_map(|cells| {
                let width = cells.iter().map(|(c, _)| c - first_col + 1).max().unwrap_or(0) as usize;
                let mut row = vec![String::new(); width];
                for (c, t) in cells {
                    row[(c - first_col) as usize] = t;
                }
                tidy(row)
            })
            .collect();
        Sheet { name, rows, truncated: self.truncated }
    }
}

/// Reads every sheet of an XLSX / XLSB workbook through its streaming cell reader.
macro_rules! stream_sheets {
    ($book:expr) => {{
        let mut sheets = Vec::new();
        for name in $book.sheet_names() {
            let mut cells = $book.worksheet_cells_reader(&name).map_err(|_| unreadable("Excel"))?;
            let mut c = Collector::new();
            while let Some(cell) = cells.next_cell().map_err(|_| unreadable("Excel"))? {
                if !c.push(cell.get_position(), &Data::from(cell.get_value().clone()))? {
                    break;
                }
            }
            sheets.push(c.finish(name));
        }
        sheets
    }};
}

/// Rows of a sheet read as a whole range (XLS, whose grid is at most 65,536 × 256, and ODS).
fn from_range(name: String, range: &calamine::Range<Data>) -> AppResult<Sheet> {
    let mut c = Collector::new();
    let (r0, c0) = range.start().unwrap_or((0, 0));
    for (r, row) in range.rows().enumerate() {
        for (i, v) in row.iter().enumerate() {
            if !c.push((r0 + r as u32, c0 + i as u32), v)? {
                return Ok(c.finish(name));
            }
        }
    }
    Ok(c.finish(name))
}

/// The ODS reader expands a sheet into a full grid, so its size is worked out from
/// `content.xml` first: rows from the first to the last with a value, times the widest row.
fn check_ods_grid(bytes: &[u8]) -> AppResult<()> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| unreadable("ODS"))?;
    let mut xml = String::new();
    zip.by_name("content.xml")
        .map_err(|_| unreadable("ODS"))?
        .take(MAX_UNZIPPED_BYTES)
        .read_to_string(&mut xml)
        .map_err(|_| unreadable("ODS"))?;
    if ods_grid_cells(&xml).into_iter().any(|cells| cells > MAX_GRID_CELLS) {
        return Err(too_big());
    }
    Ok(())
}

fn attr_number(tag: &str, attr: &str) -> u64 {
    tag.find(&format!("{attr}=\""))
        .map(|i| &tag[i + attr.len() + 2..])
        .and_then(|rest| rest.split('"').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}

/// For each table in an ODS `content.xml`: the cells its grid would hold.
fn ods_grid_cells(xml: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let (mut row_pos, mut first, mut last, mut width) = (0u64, None::<u64>, 0u64, 0u64);
    let (mut rows_rep, mut col, mut row_width) = (1u64, 0u64, 0u64);
    let (mut cell_end, mut cell_content, mut in_cell) = (0u64, false, false);
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        let after = &rest[i + 1..];
        let Some(j) = after.find('>') else { break };
        let tag = &after[..j];
        rest = &after[j + 1..];
        let closed = tag.ends_with('/');
        let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        match (name, tag) {
            ("table:table", _) => (row_pos, first, last, width) = (0, None, 0, 0),
            (_, "/table:table") => {
                if let Some(f) = first {
                    out.push((last - f + 1).saturating_mul(width));
                }
            }
            ("table:table-row", _) => {
                rows_rep = attr_number(tag, "table:number-rows-repeated").max(1);
                (col, row_width) = (0, 0);
                if closed {
                    row_pos = row_pos.saturating_add(rows_rep);
                }
            }
            (_, "/table:table-row") => {
                if row_width > 0 {
                    first = first.or(Some(row_pos));
                    last = row_pos.saturating_add(rows_rep - 1);
                    width = width.max(row_width);
                }
                row_pos = row_pos.saturating_add(rows_rep);
            }
            ("table:table-cell" | "table:covered-table-cell", _) => {
                col = col.saturating_add(attr_number(tag, "table:number-columns-repeated"));
                let valued = tag.contains("office:value-type=");
                if closed {
                    if valued {
                        row_width = col;
                    }
                } else {
                    (in_cell, cell_end, cell_content) = (true, col, valued);
                }
            }
            ("text:p", _) if in_cell => cell_content = true,
            (_, "/table:table-cell" | "/table:covered-table-cell") => {
                if cell_content {
                    row_width = cell_end;
                }
                in_cell = false;
            }
            _ => {}
        }
    }
    out
}

fn read_workbook(bytes: &[u8]) -> AppResult<Vec<Sheet>> {
    if bytes.starts_with(b"PK") {
        check_zip(bytes)?;
    }
    let book = calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|_| unreadable("Excel"))?;
    let sheets = match book {
        calamine::Sheets::Xlsx(mut x) => stream_sheets!(x),
        calamine::Sheets::Xlsb(mut x) => stream_sheets!(x),
        calamine::Sheets::Ods(mut o) => {
            check_ods_grid(bytes)?;
            let mut sheets = Vec::new();
            for name in o.sheet_names() {
                let range = o.worksheet_range(&name).map_err(|_| unreadable("ODS"))?;
                sheets.push(from_range(name, &range)?);
            }
            sheets
        }
        calamine::Sheets::Xls(mut x) => {
            let mut sheets = Vec::new();
            for name in x.sheet_names() {
                let range = x.worksheet_range(&name).map_err(|_| unreadable("Excel"))?;
                sheets.push(from_range(name, &range)?);
            }
            sheets
        }
    };
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

    /// A minimal XLSX holding `cells` (reference, inline value) on one sheet.
    fn xlsx(cells: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: &str| {
            z.start_file(name, opt).expect("entry");
            z.write_all(body.as_bytes()).expect("write");
        };
        add(
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        );
        add(
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        );
        add(
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        );
        add(
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        );
        let mut rows = String::new();
        for (r, v) in cells {
            let row: String = r.chars().filter(char::is_ascii_digit).collect();
            rows.push_str(&format!(r#"<row r="{row}"><c r="{r}"><v>{v}</v></c></row>"#));
        }
        add(
            "xl/worksheets/sheet1.xml",
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#
            ),
        );
        z.finish().expect("zip").into_inner()
    }

    #[test]
    fn a_small_workbook_reads() {
        let s = read("a.xlsx", &xlsx(&[("A1", "1234.5"), ("C1", "7"), ("B3", "-50")])).expect("reads");
        assert_eq!(s[0].name, "Sheet1");
        assert_eq!(s[0].rows, vec![vec!["1234.5", "", "7"], vec!["", "-50"]]);
        assert!(!s[0].truncated);
    }

    #[test]
    fn a_cell_in_the_far_corner_is_refused_not_allocated() {
        // A1 and XFD1048576: as a dense grid that is 17 billion cells.
        let bytes = xlsx(&[("A1", "1"), ("XFD1048576", "2")]);
        assert!(bytes.len() < 4096);
        match read("corner.xlsx", &bytes) {
            Err(AppError::BadRequest(_)) => {}
            other => panic!("expected a 400, got {other:?}"),
        }
        // Far down but within the columns: read, only two rows kept.
        let s = read("deep.xlsx", &xlsx(&[("A1", "1"), ("B1048576", "2")])).expect("reads");
        assert_eq!(s[0].rows, vec![vec!["1"], vec!["", "2"]]);
    }

    #[test]
    fn a_workbook_that_unpacks_to_too_much_is_refused() {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("xl/sharedStrings.xml", zip::write::SimpleFileOptions::default()).expect("entry");
        let chunk = vec![b' '; 1024 * 1024];
        for _ in 0..101 {
            z.write_all(&chunk).expect("write");
        }
        let bytes = z.finish().expect("zip").into_inner();
        assert!(bytes.len() < MAX_FILE_BYTES);
        match read("bomb.xlsx", &bytes) {
            Err(AppError::BadRequest(m)) => assert!(format!("{m:?}").contains("too large"), "{m:?}"),
            other => panic!("expected a 400, got {other:?}"),
        }
    }

    #[test]
    fn ods_grid_size_is_worked_out_before_reading() {
        let small = r#"<table:table table:name="S"><table:table-row><table:table-cell office:value-type="float"><text:p>1</text:p></table:table-cell><table:table-cell table:number-columns-repeated="1000"/></table:table-row><table:table-row table:number-rows-repeated="1048570"><table:table-cell/></table:table-row></table:table>"#;
        assert_eq!(ods_grid_cells(small), vec![1]);
        let huge = r#"<table:table table:name="S"><table:table-row table:number-rows-repeated="1000000"><table:table-cell table:number-columns-repeated="1000" office:value-type="string"><text:p>x</text:p></table:table-cell></table:table-row></table:table>"#;
        assert_eq!(ods_grid_cells(huge), vec![1_000_000_000]);
        let corner = r#"<table:table table:name="S"><table:table-row><table:table-cell office:value-type="float"/></table:table-row><table:table-row table:number-rows-repeated="1000000"><table:table-cell/></table:table-row><table:table-row><table:table-cell table:number-columns-repeated="16000"/><table:table-cell office:value-type="float"/></table:table-row></table:table>"#;
        assert!(ods_grid_cells(corner)[0] > MAX_GRID_CELLS);
    }

    #[test]
    fn float_cells_keep_their_typed_digits() {
        assert_eq!(cell_text(&Data::Float(1234.5)), "1234.5");
        assert_eq!(cell_text(&Data::Float(18337.13)), "18337.13");
        assert_eq!(cell_text(&Data::Float(-50000.0)), "-50000");
        assert_eq!(cell_text(&Data::Int(7)), "7");
    }
}
