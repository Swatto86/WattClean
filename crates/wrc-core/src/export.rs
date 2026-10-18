//! Lists saved for a spreadsheet: as comma-separated text, or as an Excel
//! workbook (`xlsx`).

use crate::format::value_name;
use crate::scan::Finding;

/// A list to save: a header row and rows of text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn csv(&self) -> String {
        let mut text = csv_row(&self.headers);
        for row in &self.rows {
            text.push_str(&csv_row(row));
        }
        text
    }

    /// Plain text, one tab-separated line per row.
    pub fn text(&self) -> String {
        let line = |cells: &[String]| {
            let cells: Vec<String> = cells
                .iter()
                .map(|cell| cell.replace(['\t', '\r', '\n'], " "))
                .collect();
            format!("{}\r\n", cells.join("\t"))
        };
        let mut text = line(&self.headers);
        for row in &self.rows {
            text.push_str(&line(row));
        }
        text
    }

    /// An Excel workbook with the list on a sheet named `sheet`.
    pub fn xlsx(&self, sheet: &str) -> Vec<u8> {
        crate::xlsx::workbook(self, sheet)
    }
}

/// One CSV line, ending in CRLF.
pub fn csv_row(cells: &[String]) -> String {
    let mut text = cells
        .iter()
        .map(|cell| csv_cell(cell))
        .collect::<Vec<_>>()
        .join(",");
    text.push_str("\r\n");
    text
}

/// A CSV cell, quoted when needed; a leading `=`, `+`, `-` or `@` is
/// prefixed with `'` so a spreadsheet never runs registry data as a formula.
fn csv_cell(text: &str) -> String {
    let safe = if text.starts_with(['=', '+', '-', '@']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

/// The cleaner's or a search's findings.
pub fn findings_table(findings: &[Finding]) -> Table {
    Table {
        headers: ["Category", "Key", "Value", "Why", "Account"]
            .map(str::to_owned)
            .to_vec(),
        rows: findings
            .iter()
            .map(|finding| {
                vec![
                    finding.category.title().into(),
                    finding.key.display(),
                    finding
                        .value
                        .as_deref()
                        .map(value_name)
                        .unwrap_or_default()
                        .into(),
                    finding.detail.clone(),
                    finding.account.clone().unwrap_or_default(),
                ]
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_one_tab_separated_line_per_row() {
        let table = Table {
            headers: vec!["Key".into(), "Why".into()],
            rows: vec![vec![r"HKCU\A".into(), "two\nlines\tand a tab".into()]],
        };
        assert_eq!(table.text(), "Key\tWhy\r\nHKCU\\A\ttwo lines and a tab\r\n");
    }

    #[test]
    fn cells_are_quoted_and_formulas_defused() {
        let row = csv_row(&[
            "a,b".into(),
            "say \"hi\"".into(),
            "=1+1".into(),
            "plain".into(),
        ]);
        assert_eq!(row, "\"a,b\",\"say \"\"hi\"\"\",'=1+1,plain\r\n");
    }
}
