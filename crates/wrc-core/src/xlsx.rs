//! An Excel workbook (`.xlsx`) of one sheet: the header row bold and frozen,
//! every cell plain text, so registry data can never run as a formula. The
//! parts go in a zip archive stored without compression, which Excel and
//! other spreadsheets accept, so no compression library is needed.

use std::fmt::Write;

use crate::export::Table;

/// Excel keeps at most this many characters in a cell.
const CELL_LIMIT: usize = 32_767;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;

/// Two cell formats: plain (0) and bold for the header row (1).
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

/// The workbook's bytes, with `table` on a sheet named `sheet`.
pub fn workbook(table: &Table, sheet: &str) -> Vec<u8> {
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="{}" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        escape(&sheet_name(sheet))
    );
    let parts = [
        ("[Content_Types].xml", CONTENT_TYPES.to_owned()),
        ("_rels/.rels", ROOT_RELS.to_owned()),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS.to_owned()),
        ("xl/styles.xml", STYLES.to_owned()),
        ("xl/worksheets/sheet1.xml", worksheet(table)),
    ];
    zip(&parts)
}

fn worksheet(table: &Table) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetData>"#,
    );
    let rows = std::iter::once((&table.headers, 1)).chain(table.rows.iter().map(|row| (row, 0)));
    for (number, (cells, style)) in (1..).zip(rows) {
        let _ = write!(xml, r#"<row r="{number}">"#);
        for (column, cell) in cells.iter().enumerate() {
            let _ = write!(
                xml,
                r#"<c r="{}{number}" t="inlineStr" s="{style}"><is><t xml:space="preserve">{}</t></is></c>"#,
                column_name(column),
                escape(&clip(cell))
            );
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    xml
}

/// `A` for 0, `Z` for 25, `AA` for 26.
fn column_name(mut column: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + u8::try_from(column % 26).unwrap_or(0));
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

fn clip(text: &str) -> String {
    text.chars().take(CELL_LIMIT).collect()
}

/// Excel refuses a sheet name over 31 characters or with `[]:*?/\`.
fn sheet_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !"[]:*?/\\".contains(*c))
        .take(31)
        .collect();
    if cleaned.trim().is_empty() {
        "Sheet1".into()
    } else {
        cleaned
    }
}

/// Text as XML character data; characters XML cannot hold are dropped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c < ' ' || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

/// A zip archive of these files, stored without compression.
fn zip(files: &[(&str, String)]) -> Vec<u8> {
    // DOS date 1980-01-01, midnight.
    const DATE: u16 = 0x0021;
    let mut archive = Vec::new();
    let mut directory = Vec::new();
    for (name, text) in files {
        let data = text.as_bytes();
        let crc = crc32(data);
        let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let offset = u32::try_from(archive.len()).unwrap_or(u32::MAX);
        let name_length = u16::try_from(name.len()).unwrap_or(u16::MAX);
        // Local file header.
        archive.extend(0x0403_4b50_u32.to_le_bytes());
        archive.extend(20_u16.to_le_bytes());
        archive.extend([0, 0, 0, 0, 0, 0]);
        archive.extend(DATE.to_le_bytes());
        archive.extend(crc.to_le_bytes());
        archive.extend(size.to_le_bytes());
        archive.extend(size.to_le_bytes());
        archive.extend(name_length.to_le_bytes());
        archive.extend(0_u16.to_le_bytes());
        archive.extend(name.as_bytes());
        archive.extend(data);
        // Its central directory entry.
        directory.extend(0x0201_4b50_u32.to_le_bytes());
        directory.extend(20_u16.to_le_bytes());
        directory.extend(20_u16.to_le_bytes());
        directory.extend([0, 0, 0, 0, 0, 0]);
        directory.extend(DATE.to_le_bytes());
        directory.extend(crc.to_le_bytes());
        directory.extend(size.to_le_bytes());
        directory.extend(size.to_le_bytes());
        directory.extend(name_length.to_le_bytes());
        directory.extend([0; 12]);
        directory.extend(offset.to_le_bytes());
        directory.extend(name.as_bytes());
    }
    let count = u16::try_from(files.len()).unwrap_or(u16::MAX);
    let directory_size = u32::try_from(directory.len()).unwrap_or(u32::MAX);
    let directory_offset = u32::try_from(archive.len()).unwrap_or(u32::MAX);
    archive.extend(directory);
    // End of central directory.
    archive.extend(0x0605_4b50_u32.to_le_bytes());
    archive.extend([0, 0, 0, 0]);
    archive.extend(count.to_le_bytes());
    archive.extend(count.to_le_bytes());
    archive.extend(directory_size.to_le_bytes());
    archive.extend(directory_offset.to_le_bytes());
    archive.extend(0_u16.to_le_bytes());
    archive
}

/// CRC-32 (IEEE), as zip uses.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table {
            headers: vec!["Key".into(), "Data".into()],
            rows: vec![vec![r"HKCU\Software".into(), "=SUM(A1)<b>\u{1}".into()]],
        }
    }

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn columns_are_named_as_spreadsheets_name_them() {
        let names: Vec<String> = [0, 25, 26, 27, 51, 52, 701, 702]
            .into_iter()
            .map(column_name)
            .collect();
        assert_eq!(names, ["A", "Z", "AA", "AB", "AZ", "BA", "ZZ", "AAA"]);
    }

    #[test]
    fn the_sheet_holds_text_cells_with_a_bold_header() {
        let xml = worksheet(&table());
        assert!(
            xml.contains(r#"<c r="A1" t="inlineStr" s="1"><is><t xml:space="preserve">Key</t>"#)
        );
        // Formula-like text stays text; markup and control characters are escaped or dropped.
        assert!(
            xml.contains(
                r#"<c r="B2" t="inlineStr" s="0"><is><t xml:space="preserve">=SUM(A1)&lt;b&gt;</t>"#
            ),
            "{xml}"
        );
        assert!(!xml.contains("<f>"));
    }

    #[test]
    fn the_archive_lists_every_part() {
        let bytes = workbook(&table(), "Scan: results?");
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], &0x0605_4b50_u32.to_le_bytes());
        assert_eq!(u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]), 6);
        let text = String::from_utf8_lossy(&bytes);
        for part in [
            "[Content_Types].xml",
            "xl/workbook.xml",
            "xl/worksheets/sheet1.xml",
        ] {
            assert!(text.contains(part), "{part}");
        }
        assert!(
            text.contains(r#"<sheet name="Scan results" "#),
            "illegal characters removed"
        );
    }

    /// Windows' own zip reader opens every part, and each is well-formed
    /// XML. (It does not check checksums: the check value test above does.)
    #[cfg(windows)]
    #[test]
    fn windows_reads_the_workbook_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("check.xlsx");
        std::fs::write(&file, workbook(&table(), "Check")).unwrap();
        let script = format!(
            "Add-Type -AssemblyName System.IO.Compression.FileSystem;              $zip = [IO.Compression.ZipFile]::OpenRead('{}'); $n = 0;              foreach ($e in $zip.Entries) {{ $r = New-Object IO.StreamReader($e.Open());              $t = $r.ReadToEnd(); $r.Close(); [xml]$t | Out-Null; $n++ }}; $zip.Dispose(); \"ok $n\"",
            file.display()
        );
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            text.trim(),
            "ok 6",
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
