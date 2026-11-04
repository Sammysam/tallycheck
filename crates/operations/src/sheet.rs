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

/// Rows of a sheet read as a whole range (XLS and ODS, both checked first: [`check_xls`],
/// [`check_ods_grid`]).
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

/// An old Excel (.xls) file is checked before calamine opens it. calamine 0.38 offers no way to
/// learn a sheet's size first: opening the file already expands every sheet into a full grid
/// (values and formulas), reserves room for the cell count its DIMENSIONS record claims, and
/// follows the compound file's sector chains without stopping at a loop. A few kilobytes could
/// claim gigabytes, so [`xls_guard`] walks the same structures the way calamine does, without
/// expanding anything, and the file is refused when calamine would allocate more than
/// [`MAX_GRID_CELLS`] cells in all, keep values beyond [`MAX_COLUMNS`], or meet a malformed
/// structure that would make it loop or panic. Rows are not refused: an .xls holds at most
/// 65,536, and as for the other formats only the first [`MAX_ROWS`] are kept.
fn check_xls(bytes: &[u8]) -> AppResult<()> {
    match xls_guard::check(bytes) {
        Ok(()) => Ok(()),
        Err(xls_guard::Problem::Unreadable) => Err(unreadable("Excel")),
        Err(xls_guard::Problem::TooBig) => Err(too_big()),
        Err(xls_guard::Problem::TooWide) => Err(AppError::bad_request(format!(
            "This sheet has values beyond column {MAX_COLUMNS}. Copy the columns you need into a new sheet and try again."
        ))),
    }
}

/// The OLE compound file signature that every .xls starts with.
const OLE_SIGNATURE: &[u8; 8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

/// Mirrors what calamine 0.38 (`src/cfb.rs`, `src/xls.rs`) reads from an .xls, counting instead
/// of allocating. Where the two could see the file differently (malformed files only), the
/// difference ends in a refusal or in harmless zero bytes.
mod xls_guard {
    use super::{MAX_COLUMNS, MAX_GRID_CELLS};

    #[derive(Debug, PartialEq, Eq)]
    pub enum Problem {
        Unreadable,
        TooBig,
        TooWide,
    }

    type R<T> = Result<T, Problem>;

    const RESERVED_SECTORS: u32 = 0xFFFF_FFFA;
    const DIFSECT: u32 = 0xFFFF_FFFC;
    const ENDOFCHAIN: u32 = 0xFFFF_FFFE;

    fn u16_at(b: &[u8], i: usize) -> R<u16> {
        b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or(Problem::Unreadable)
    }

    fn u32_at(b: &[u8], i: usize) -> R<u32> {
        b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or(Problem::Unreadable)
    }

    fn u32s(b: &[u8]) -> R<impl Iterator<Item = u32> + '_> {
        if !b.len().is_multiple_of(4) {
            // calamine asserts on this (a panic).
            return Err(Problem::Unreadable);
        }
        Ok(b.as_chunks::<4>().0.iter().map(|s| u32::from_le_bytes(*s)))
    }

    struct Sectors<'a> {
        data: &'a [u8],
        size: usize,
        maxid: u32,
        /// The mini stream: a sector past its end is refused (calamine would read it from
        /// wherever the file reader happens to be).
        strict: bool,
    }

    impl Sectors<'_> {
        fn get(&self, id: u32) -> R<&[u8]> {
            if self.maxid < id {
                return Err(Problem::Unreadable);
            }
            let start = id as usize * self.size;
            let end = start + self.size;
            if end > self.data.len() && self.strict {
                return Err(Problem::Unreadable);
            }
            Ok(self.data.get(start..end.min(self.data.len())).unwrap_or(&[]))
        }

        /// A sector chain as calamine reads it, except that a chain visiting more sectors than
        /// the file has (a loop) is refused instead of being followed until `len` bytes.
        fn chain(&self, mut id: u32, fats: &[u32], len: usize) -> R<Vec<u8>> {
            let mut out = Vec::new();
            let mut steps: u64 = 0;
            while id != ENDOFCHAIN {
                steps += 1;
                if steps > u64::from(self.maxid) + 2 {
                    return Err(Problem::Unreadable);
                }
                let sector = self.get(id)?;
                let take = sector.len().min(len - out.len());
                out.extend_from_slice(&sector[..take]);
                if out.len() >= len {
                    break;
                }
                id = *fats.get(id as usize).ok_or(Problem::Unreadable)?;
            }
            Ok(out)
        }
    }

    struct Dir {
        name: String,
        start: u32,
        len: usize,
    }

    /// The `Workbook` (or older `Book`) stream, read like calamine's `Cfb::new` + `get_stream`.
    fn workbook_stream(bytes: &[u8]) -> R<Vec<u8>> {
        if bytes.len() < 512 || &bytes[..8] != super::OLE_SIGNATURE {
            return Err(Problem::Unreadable);
        }
        let version = u16_at(bytes, 26)?;
        let (size, header_len) = match u16_at(bytes, 30)? {
            0x0009 => (512usize, 512usize),
            0x000C => (4096, 4096),
            _ => return Err(Problem::Unreadable),
        };
        if bytes.len() < header_len || u16_at(bytes, 32)? != 0x0006 {
            return Err(Problem::Unreadable);
        }
        let dir_start = u32_at(bytes, 48)?;
        let mini_fat_start = u32_at(bytes, 60)?;
        let mini_fat_len = u32_at(bytes, 64)? as usize;
        let difat_start = u32_at(bytes, 68)?;
        let mut difat: Vec<u32> = u32s(&bytes[76..512])?.collect();
        let maxid = bytes.len() / size;
        let sectors = Sectors { data: &bytes[header_len..], size, maxid: maxid as u32, strict: false };

        let mut sector_id = difat_start;
        let mut difat_sectors = 0usize;
        while sector_id < RESERVED_SECTORS {
            difat_sectors += 1;
            if difat_sectors > maxid.saturating_add(1) {
                return Err(Problem::Unreadable);
            }
            difat.extend(u32s(sectors.get(sector_id)?)?);
            sector_id = difat.pop().ok_or(Problem::Unreadable)?;
        }
        // A file cannot hold more FAT sectors than sectors; calamine would load each listed one,
        // repeats included.
        let fat_ids: Vec<u32> = difat.into_iter().filter(|id| *id < DIFSECT).collect();
        if fat_ids.len() > maxid.saturating_add(1) {
            return Err(Problem::Unreadable);
        }
        let mut fats = Vec::new();
        for id in fat_ids {
            fats.extend(u32s(sectors.get(id)?)?);
        }

        let dir_bytes = sectors.chain(dir_start, &fats, usize::MAX)?;
        let mut dirs = Vec::new();
        for c in dir_bytes.as_chunks::<128>().0 {
            let mut name = encoding_rs::UTF_16LE.decode(&c[..64]).0.into_owned();
            if let Some(l) = name.as_bytes().iter().position(|b| *b == 0) {
                name.truncate(l);
            }
            let start = u32_at(c, 116)?;
            let len = if size == 512 {
                u32_at(c, 120)? as usize
            } else {
                usize::try_from(u64::from(u32_at(c, 120)?) | (u64::from(u32_at(c, 124)?) << 32))
                    .map_err(|_| Problem::Unreadable)?
            };
            dirs.push(Dir { name, start, len });
        }
        if dirs.is_empty() || (version != 3 && dirs[0].start == ENDOFCHAIN) {
            return Err(Problem::Unreadable);
        }
        let (mini_fats, ministream) = if mini_fat_len > 0 {
            let ministream = sectors.chain(dirs[0].start, &fats, dirs[0].len)?;
            let minifat = sectors.chain(mini_fat_start, &fats, mini_fat_len.saturating_mul(size))?;
            let mini_fats: Vec<u32> = u32s(&minifat)?.collect();
            (mini_fats, ministream)
        } else {
            (Vec::new(), Vec::new())
        };
        let mini = Sectors { data: &ministream, size: 64, maxid: (maxid * 8) as u32, strict: true };

        for wanted in ["Workbook", "Book"] {
            if let Some(d) = dirs.iter().find(|d| d.name.as_bytes().eq_ignore_ascii_case(wanted.as_bytes())) {
                // Any failure here is a refusal: calamine would fall back to "Book" and the two
                // readers could then look at different streams.
                return if d.len < 4096 {
                    mini.chain(d.start, &mini_fats, d.len)
                } else {
                    sectors.chain(d.start, &fats, d.len)
                };
            }
        }
        Err(Problem::Unreadable)
    }

    /// BIFF records: (type, data). Continue records come out as records of their own; nothing
    /// below looks at them, as calamine merges them into the record before.
    fn records(mut s: &[u8]) -> impl Iterator<Item = R<(u16, &[u8])>> {
        std::iter::from_fn(move || {
            if s.is_empty() {
                return None;
            }
            if s.len() < 4 {
                s = &[];
                return Some(Err(Problem::Unreadable));
            }
            let typ = u16::from_le_bytes([s[0], s[1]]);
            let len = u16::from_le_bytes([s[2], s[3]]) as usize;
            if s.len() < len + 4 {
                s = &[];
                return Some(Err(Problem::Unreadable));
            }
            let data = &s[4..len + 4];
            s = &s[len + 4..];
            Some(Ok((typ, data)))
        })
    }

    /// The smallest rectangle around a set of cells, as `Range::from_sparse` builds it.
    #[derive(Default)]
    struct Bounds(Option<(u32, u32, u32, u32)>);

    impl Bounds {
        fn add(&mut self, row: u32, col: u32) {
            self.0 = Some(match self.0 {
                None => (row, row, col, col),
                Some((r0, r1, c0, c1)) => (r0.min(row), r1.max(row), c0.min(col), c1.max(col)),
            });
        }

        fn add_pos(&mut self, (row, col): (u32, u32)) {
            self.add(row, col);
        }

        fn cells(&self) -> u64 {
            self.0.map_or(0, |(r0, r1, c0, c1)| u64::from(r1 - r0 + 1) * u64::from(c1 - c0 + 1))
        }

        fn last_col(&self) -> Option<u32> {
            self.0.map(|(_, _, _, c1)| c1)
        }
    }

    fn pos(data: &[u8], min_len: usize) -> R<(u32, u32)> {
        if data.len() < min_len {
            return Err(Problem::Unreadable);
        }
        Ok((u32::from(u16_at(data, 0)?), u32::from(u16_at(data, 2)?)))
    }

    /// The cells calamine would allocate for the sheet substream at `pos`, and the bytes read.
    fn sheet(stream: &[u8], at: usize) -> R<(u64, usize)> {
        let sub = stream.get(at..).ok_or(Problem::Unreadable)?;
        let (mut values, mut formulas) = (Bounds::default(), Bounds::default());
        let (mut fmla_pos, mut seen_bof, mut depth, mut read) = ((0u32, 0u32), false, 0usize, 0usize);
        for record in records(sub) {
            let (typ, data) = record?;
            read += data.len() + 4;
            if depth > 0 {
                match typ {
                    0x0809 => depth += 1,
                    0x000A => depth -= 1,
                    _ => {}
                }
                continue;
            }
            match typ {
                0x0809 if seen_bof => depth += 1,
                0x0809 => seen_bof = true,
                0x0200 => {
                    let (rf, rl, mut cf, cl) = match data.len() {
                        10 => (
                            u32::from(u16_at(data, 0)?),
                            u32::from(u16_at(data, 2)?),
                            u32::from(u16_at(data, 4)?),
                            u32::from(u16_at(data, 6)?),
                        ),
                        14 | 16 => (
                            u32_at(data, 0)?,
                            u32_at(data, 4)?,
                            u32::from(u16_at(data, 8)?),
                            u32::from(u16_at(data, 10)?),
                        ),
                        _ => return Err(Problem::Unreadable),
                    };
                    if 0xFF < cf || cl < cf {
                        cf = 0;
                    }
                    let ((r0, c0), (r1, c1)) =
                        if 1 <= rl && 1 <= cl { ((rf, cf), (rl - 1, cl - 1)) } else { ((rf, cf), (rf, cf)) };
                    // calamine subtracts these without checking: an end before the start would
                    // overflow (a panic, or a reservation of billions of cells).
                    if r1 < r0 || c1 < c0 {
                        return Err(Problem::Unreadable);
                    }
                    if u64::from(r1 - r0 + 1) * u64::from(c1 - c0 + 1) > MAX_GRID_CELLS {
                        return Err(Problem::TooBig);
                    }
                }
                0x0203 => values.add_pos(pos(data, 14)?),
                0x0204 | 0x00D6 => values.add_pos(pos(data, 6)?),
                0x0205 => values.add_pos(pos(data, 8)?),
                0x027E | 0x00FD => values.add_pos(pos(data, 10)?),
                0x0207 => values.add_pos(fmla_pos),
                0x00BD => {
                    let (row, first) = pos(data, 6)?;
                    let last = u32::from(u16_at(data, data.len() - 2)?);
                    if last < first || data.len() != 6 + 6 * (last - first + 1) as usize {
                        return Err(Problem::Unreadable);
                    }
                    values.add(row, first);
                    values.add(row, last);
                }
                0x0006 => {
                    fmla_pos = pos(data, 20)?;
                    values.add_pos(fmla_pos);
                    formulas.add_pos(fmla_pos);
                }
                0x000A => break,
                _ => {}
            }
        }
        if values.last_col().is_some_and(|c| c >= MAX_COLUMNS) {
            return Err(Problem::TooWide);
        }
        Ok((values.cells().saturating_add(formulas.cells()), read))
    }

    pub fn check(bytes: &[u8]) -> R<()> {
        let stream = workbook_stream(bytes)?;
        let mut sheets = Vec::new();
        for record in records(&stream) {
            let (typ, data) = record?;
            match typ {
                0x0085 => {
                    if data.len() < 6 {
                        return Err(Problem::Unreadable);
                    }
                    sheets.push(u32_at(data, 0)? as usize);
                }
                0x000A => break,
                _ => {}
            }
        }
        let (mut cells, mut read) = (0u64, 0usize);
        for at in sheets {
            let (c, r) = sheet(&stream, at)?;
            cells = cells.saturating_add(c);
            read = read.saturating_add(r);
            if cells > MAX_GRID_CELLS {
                return Err(Problem::TooBig);
            }
            // Sheets are separate parts of the stream; many sheets pointing at one long part
            // would make the reader go over it again and again.
            if read > stream.len() {
                return Err(Problem::TooBig);
            }
        }
        Ok(())
    }
}

fn read_workbook(bytes: &[u8]) -> AppResult<Vec<Sheet>> {
    if bytes.starts_with(b"PK") {
        check_zip(bytes)?;
    }
    if bytes.starts_with(OLE_SIGNATURE) {
        check_xls(bytes)?;
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

    fn record(typ: u16, data: &[u8]) -> Vec<u8> {
        let mut r = typ.to_le_bytes().to_vec();
        r.extend((data.len() as u16).to_le_bytes());
        r.extend(data);
        r
    }

    fn number(row: u16, col: u16, v: f64) -> Vec<u8> {
        let mut d = row.to_le_bytes().to_vec();
        d.extend(col.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        d.extend(v.to_le_bytes());
        record(0x0203, &d)
    }

    /// A minimal BIFF8 .xls (one compound-file stream "Workbook", one sheet "Sheet1") holding
    /// `sheet_records` after the sheet's BOF. With `cyclic`, the stream's last sector points
    /// back to its first and the stream claims a gigabyte.
    fn xls(sheet_records: &[Vec<u8>], cyclic: bool) -> Vec<u8> {
        let bof = |dt: u16| {
            let mut d = 0x0600u16.to_le_bytes().to_vec();
            d.extend(dt.to_le_bytes());
            d.extend([0u8; 12]);
            record(0x0809, &d)
        };
        let boundsheet = |at: u32| {
            let mut d = at.to_le_bytes().to_vec();
            d.extend([0, 0, 6, 0]);
            d.extend(b"Sheet1");
            record(0x0085, &d)
        };
        let globals_len = (bof(5).len() + boundsheet(0).len() + 4) as u32;
        let mut stream = bof(5);
        stream.extend(boundsheet(globals_len));
        stream.extend(record(0x000A, &[]));
        stream.extend(bof(0x10));
        for r in sheet_records {
            stream.extend(r);
        }
        stream.extend(record(0x000A, &[]));
        let stream_len = stream.len().max(4096);
        stream.resize(stream_len.div_ceil(512) * 512, 0);
        let n = (stream.len() / 512) as u32;

        const END: u32 = 0xFFFF_FFFE;
        let mut file = vec![0u8; 512];
        file[..8].copy_from_slice(OLE_SIGNATURE);
        let mut put = |at: usize, v: u32| file[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(24, 0x0003_003E); // minor version 0x3E, major version 3
        put(28, 0x0009_FFFE); // byte order, sector shift 9
        put(32, 0x0006); // mini sector shift 6
        put(44, 1); // one FAT sector
        put(48, 1); // directory in sector 1
        put(56, 4096); // mini stream cutoff
        put(60, END);
        put(68, END);
        for i in 0..109 {
            put(76 + 4 * i, if i == 0 { 0 } else { u32::MAX });
        }
        let mut fat = vec![u32::MAX; 128];
        fat[0] = 0xFFFF_FFFD;
        fat[1] = END;
        for i in 0..n {
            fat[2 + i as usize] = if i + 1 == n {
                if cyclic {
                    2
                } else {
                    END
                }
            } else {
                3 + i
            };
        }
        file.extend(fat.iter().flat_map(|v| v.to_le_bytes()));
        let entry = |name: &str, typ: u8, start: u32, len: u32| {
            let mut e = vec![0u8; 128];
            for (i, u) in name.encode_utf16().enumerate() {
                e[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
            }
            e[64..66].copy_from_slice(&((name.len() as u16 + 1) * 2).to_le_bytes());
            e[66] = typ;
            e[116..120].copy_from_slice(&start.to_le_bytes());
            e[120..124].copy_from_slice(&len.to_le_bytes());
            e
        };
        file.extend(entry("Root Entry", 5, END, 0));
        file.extend(entry("Workbook", 2, 2, if cyclic { 1 << 30 } else { stream_len as u32 }));
        file.extend([0u8; 256]);
        file.extend(stream);
        file
    }

    fn refused_with(file: &str, bytes: &[u8], words: &str) {
        match read(file, bytes) {
            Err(AppError::BadRequest(m)) => assert!(format!("{m:?}").contains(words), "{m:?}"),
            other => panic!("expected a 400, got {other:?}"),
        }
    }

    #[test]
    fn a_small_xls_reads() {
        let bytes = xls(&[number(0, 0, 1234.5), number(1, 2, 7.0)], false);
        assert_eq!(xls_guard::check(&bytes), Ok(()));
        let s = read("old.xls", &bytes).expect("reads");
        assert_eq!(s[0].name, "Sheet1");
        assert_eq!(s[0].rows, vec![vec!["1234.5"], vec!["", "", "7"]]);
    }

    #[test]
    fn an_xls_whose_grid_is_too_large_is_refused_before_reading() {
        // A1 and the last row at column 200: a 65,536 × 200 grid from a 5 KB file.
        let bytes = xls(&[number(0, 0, 1.0), number(u16::MAX, 199, 2.0)], false);
        assert!(bytes.len() < 8192);
        assert_eq!(xls_guard::check(&bytes), Err(xls_guard::Problem::TooBig));
        refused_with("corner.xls", &bytes, "too large");
        // Within the limit (65,536 × 50 cells): read, only two rows kept.
        let s = read("deep.xls", &xls(&[number(0, 0, 1.0), number(u16::MAX, 49, 2.0)], false)).expect("reads");
        assert_eq!(s[0].rows.len(), 2);
    }

    #[test]
    fn an_xls_with_values_beyond_the_column_limit_is_refused() {
        let bytes = xls(&[number(0, 0, 1.0), number(0, MAX_COLUMNS as u16, 2.0)], false);
        refused_with("wide.xls", &bytes, "beyond column 200");
    }

    #[test]
    fn an_xls_dimensions_record_cannot_reserve_billions_of_cells() {
        let mut d = 0u32.to_le_bytes().to_vec();
        d.extend(1_000_000_000u32.to_le_bytes());
        d.extend([0, 0, 10, 0, 0, 0]);
        let bytes = xls(&[record(0x0200, &d), number(0, 0, 1.0)], false);
        refused_with("dims.xls", &bytes, "too large");
        // An end before the start would overflow inside calamine.
        let mut d = 10u32.to_le_bytes().to_vec();
        d.extend(5u32.to_le_bytes());
        d.extend([0, 0, 1, 0, 0, 0]);
        let bytes = xls(&[record(0x0200, &d)], false);
        assert_eq!(xls_guard::check(&bytes), Err(xls_guard::Problem::Unreadable));
    }

    #[test]
    fn an_xls_whose_sector_chain_loops_is_refused() {
        let bytes = xls(&[number(0, 0, 1.0)], true);
        assert_eq!(xls_guard::check(&bytes), Err(xls_guard::Problem::Unreadable));
        refused_with("loop.xls", &bytes, "Could not read");
    }

    #[test]
    fn float_cells_keep_their_typed_digits() {
        assert_eq!(cell_text(&Data::Float(1234.5)), "1234.5");
        assert_eq!(cell_text(&Data::Float(18337.13)), "18337.13");
        assert_eq!(cell_text(&Data::Float(-50000.0)), "-50000");
        assert_eq!(cell_text(&Data::Int(7)), "7");
    }
}
