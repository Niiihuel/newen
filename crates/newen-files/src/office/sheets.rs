//! A workbook (xlsx, xlsm, xls, xlsb, ods) as Markdown: each sheet under its name, as a table
//! the way the spreadsheet shows it: the columns by letter, the rows by number.
use super::{inline, table};
use calamine::{Data, Range, Reader};
use std::path::Path;

/// Of the 20,000 characters limn's Markdown shows, what the sheets share, so that every sheet
/// shows its first rows: a short one all it needs, the long ones the rest alike, each at least
/// `LEAST`. A note says how many rows are left out.
const SHOWN: usize = 18_000;
const LEAST: usize = 1_500;
/// The columns of a sheet that are shown, A to Z.
const COLUMNS: usize = 26;

pub(super) fn reads(path: &Path) -> bool {
    matches!(
        super::extension(path).as_str(),
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods"
    )
}

pub(super) fn markdown(path: &Path) -> Result<String, String> {
    let mut book = calamine::open_workbook_auto(path).map_err(|e| e.to_string())?;
    let mut sheets = vec![];
    for name in book.sheet_names() {
        let values = book.worksheet_range(&name).map_err(|e| e.to_string())?;
        // A sheet with no formulas, or a format that keeps none, has an empty range of them.
        let formulas = book.worksheet_formula(&name).unwrap_or_default();
        sheets.push((name, values, formulas));
    }
    let wanted: Vec<usize> = (sheets.iter())
        .map(|(_, values, formulas)| sheet(values, formulas, SHOWN).chars().count())
        .collect();
    let mut out = String::new();
    for ((name, values, formulas), budget) in sheets.iter().zip(share(&wanted)) {
        out += &format!("## {}\n\n", inline(name));
        out += &sheet(values, formulas, budget);
    }
    Ok(out)
}

/// What each sheet may use of `SHOWN`: the ones that want less than an even share have it, and
/// the others share what is left.
fn share(wanted: &[usize]) -> Vec<usize> {
    let mut long: Vec<usize> = (0..wanted.len()).collect();
    loop {
        let kept: usize = (0..wanted.len())
            .filter(|n| !long.contains(n))
            .map(|n| wanted[n])
            .sum();
        let even = (SHOWN.saturating_sub(kept) / long.len().max(1)).max(LEAST);
        let (short, rest): (Vec<usize>, Vec<usize>) =
            long.iter().partition(|n| wanted[**n] <= even);
        if short.is_empty() || rest.is_empty() {
            return (0..wanted.len())
                .map(|n| {
                    if long.contains(&n) && !short.contains(&n) {
                        even
                    } else {
                        wanted[n]
                    }
                })
                .collect();
        }
        long = rest;
    }
}

/// The cells from the first that holds something to the last, a value or else the formula
/// that has none saved (a file written by a program that does not compute them): the rows that
/// fit in `budget` characters.
fn sheet(values: &Range<Data>, formulas: &Range<String>, budget: usize) -> String {
    let corners = [
        values.start().zip(values.end()),
        formulas.start().zip(formulas.end()),
    ];
    let Some(((top, left), (bottom, right))) = (corners.into_iter().flatten())
        .reduce(|(a, b), (c, d)| ((a.0.min(c.0), a.1.min(c.1)), (b.0.max(d.0), b.1.max(d.1))))
    else {
        return "This sheet is empty.\n\n".into();
    };
    let (rows, columns) = ((bottom - top + 1) as usize, (right - left + 1) as usize);
    let shown_columns = columns.min(COLUMNS);
    let mut lines = vec![std::iter::once(String::new())
        .chain((0..shown_columns).map(|c| column(left as usize + c)))
        .collect::<Vec<_>>()];
    // A row of the table is its cells, each with ` | ` around it.
    let length = |line: &[String]| line.iter().map(|c| c.chars().count() + 3).sum::<usize>() + 2;
    let mut used = 2 * length(&lines[0]);
    for row in top..=bottom {
        let mut line = vec![(row + 1).to_string()];
        for col in left..left + shown_columns as u32 {
            let value = values.get_value((row, col)).unwrap_or(&Data::Empty);
            let text = match (value, formulas.get_value((row, col))) {
                (Data::Empty, Some(formula)) if !formula.is_empty() => format!("={formula}"),
                (value, _) => cell(value),
            };
            line.push(inline(&text.replace(['\r', '\n'], " ")));
        }
        used += length(&line);
        // The first row always shows.
        if used > budget && lines.len() > 1 {
            break;
        }
        lines.push(line);
    }
    let shown_rows = lines.len() - 1;
    let mut out = table(&lines);
    if shown_rows < rows || shown_columns < columns {
        out += &format!(
            "Showing the first {shown_rows} of {rows} rows and {shown_columns} of {columns} columns.\n\n"
        );
    }
    out
}

/// A column's letters: 0 is A, 26 is AA.
fn column(mut n: usize) -> String {
    let mut letters = vec![];
    loop {
        letters.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// A value as the spreadsheet writes it in General format.
fn cell(value: &Data) -> String {
    match value {
        Data::Int(n) => n.to_string(),
        Data::Float(n) => number(*n),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Bool(true) => "TRUE".into(),
        Data::Bool(false) => "FALSE".into(),
        Data::DateTime(date) if date.is_duration() => {
            let seconds = (date.as_f64() * 86_400.0).round() as i64;
            format!(
                "{}:{:02}:{:02}",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        }
        Data::DateTime(date) => {
            let (y, mo, d, h, mi, s, _) = date.to_ymd_hms_milli();
            match (date.as_f64() < 1.0, (h, mi, s) == (0, 0, 0)) {
                (true, _) => format!("{h:02}:{mi:02}:{s:02}"),
                (false, true) => format!("{y}-{mo:02}-{d:02}"),
                (false, false) => format!("{y}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}"),
            }
        }
        Data::Error(error) => error.to_string(),
        Data::Empty => String::new(),
    }
}

/// A number to the 15 significant digits a spreadsheet keeps: 0.1 + 0.2 is 0.3.
fn number(n: f64) -> String {
    if n == 0.0 || !n.is_finite() || !(1e-9..1e15).contains(&n.abs()) {
        return n.to_string();
    }
    let decimals = (14 - n.abs().log10().floor() as i32).max(0) as usize;
    let fixed = format!("{n:.decimals$}");
    match fixed.contains('.') {
        true => fixed.trim_end_matches('0').trim_end_matches('.').to_owned(),
        false => fixed,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::zipped;
    use super::*;

    /// A sheet shows the rows that fit its share, and says how many it has.
    #[test]
    fn a_long_sheet_shows_what_fits() {
        let mut values = Range::new((0, 0), (999, 2));
        for row in 0..1000 {
            for col in 0..3 {
                values.set_value((row, col), Data::Int(row as i64));
            }
        }
        let shown = sheet(&values, &Range::default(), LEAST);
        assert!(shown.chars().count() <= LEAST + 200, "{}", shown.len());
        assert!(shown.contains("| 1 | 0 | 0 | 0 |"));
        let rows = shown
            .lines()
            .filter(|l| l.starts_with("| ") && !l.starts_with("|  |"))
            .count();
        assert!(shown.ends_with(&format!(
            "Showing the first {rows} of 1000 rows and 3 of 3 columns.\n\n"
        )));
    }

    #[test]
    fn short_sheets_keep_theirs_and_long_ones_share_the_rest() {
        assert_eq!(share(&[100, 50_000]), [100, SHOWN - 100]);
        assert_eq!(share(&[30_000, 30_000, 200]), [8_900, 8_900, 200]);
        assert_eq!(share(&[500, 600]), [500, 600]);
        assert_eq!(share(&[50_000; 20]), [LEAST; 20]);
    }

    #[test]
    fn numbers_keep_fifteen_significant_digits() {
        assert_eq!(number(0.1 + 0.2), "0.3");
        assert_eq!(number(1234.5), "1234.5");
        assert_eq!(number(-2.0), "-2");
        assert_eq!(number(1.0 / 3.0), "0.333333333333333");
        assert_eq!(number(1e20), "100000000000000000000");
        assert_eq!(
            [column(0), column(25), column(26), column(701)],
            ["A", "Z", "AA", "ZZ"]
        );
    }

    /// A sheet by its letters and row numbers from where its cells start: numbers, text, a
    /// boolean, a cell that holds Markdown's marks, and a formula with no value saved.
    #[test]
    fn a_workbook_reads_as_a_table_per_sheet() {
        let parts = [
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Ventas" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
                    <row r="2"><c r="B2" t="inlineStr"><is><t>Mes</t></is></c><c r="C2" t="inlineStr"><is><t>Total</t></is></c></row>
                    <row r="3"><c r="B3" t="inlineStr"><is><t>a|*b*</t></is></c><c r="C3"><v>1.5</v></c><c r="D3" t="b"><v>1</v></c></row>
                    <row r="4"><c r="E4"><f>C3*2</f></c></row>
                </sheetData></worksheet>"#,
            ),
        ];
        let path = zipped("a.xlsx", &parts);
        assert_eq!(
            markdown(&path).unwrap(),
            "## Ventas\n\n|  | B | C | D | E |\n|---|---|---|---|---|\n| 2 | Mes | Total |  |  |\n\
             | 3 | a\\|\\*b\\* | 1.5 | TRUE |  |\n| 4 |  |  |  | =C3\\*2 |\n\n"
        );
    }
}
