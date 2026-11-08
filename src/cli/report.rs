//! The report layer's presentation primitives: the output format, the table
//! renderer behind every view, and the framing block above a table.

use anyhow::Result;

use crate::session::Style;

/// One dimmed framing block above a table: what this view shows and how to
/// read it, so the output explains itself. Table format only — markdown is for
/// pasting the table, json is the machine contract.
pub(crate) fn preamble(format: Format, text: &str) {
    if matches!(format, Format::Table) {
        println!("{}\n", Style::stdout().dim(text));
    }
}

/// Column alignment for the table renderer.
#[derive(Clone, Copy)]
pub(crate) enum Align {
    Left,
    Right,
}

/// Output format for reports: `table` and `markdown` are for humans, `json`
/// is the machine contract — typed values, nothing but JSON on stdout (all
/// meta/freshness chatter goes to stderr).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Format {
    Table,
    Markdown,
    Json,
}

pub(crate) fn parse_format(value: Option<&str>) -> Result<Format> {
    match value.unwrap_or("table") {
        "table" => Ok(Format::Table),
        "markdown" | "md" => Ok(Format::Markdown),
        "json" => Ok(Format::Json),
        other => anyhow::bail!("unknown --format '{other}' (table|markdown|json)"),
    }
}

/// The `--format` values one windowed view accepts — the vocabulary beside
/// the format it produces. The table views and trend take all three values;
/// doctor's human form is its own text layout rather than a table pasted as
/// markdown, so `markdown` is not one of its values and the refusal names the
/// two it does take (`docs/specs/cli.md`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum ViewFormat {
    /// `table | markdown | json`.
    TableMarkdownJson,
    /// `table | json` — doctor only.
    TableJson,
}

impl ViewFormat {
    /// Resolve `--format` against this view's values. Called before the store
    /// opens, so an unaccepted value fails on its own rather than after the
    /// refresh and the db's creation (`docs/specs/cli.md`).
    pub(crate) fn parse(self, value: Option<&str>) -> Result<Format> {
        match self {
            ViewFormat::TableMarkdownJson => parse_format(value),
            ViewFormat::TableJson => match value.unwrap_or("table") {
                "table" => Ok(Format::Table),
                "json" => Ok(Format::Json),
                other => anyhow::bail!("unknown --format '{other}' (table|json)"),
            },
        }
    }
}

/// Print a machine-readable payload: pretty JSON, alone on stdout.
pub(crate) fn emit_json(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Render a table as aligned text or GitHub-flavored markdown. Auto-sizes
/// columns; `aligns` controls per-column alignment in table mode. `json` is
/// each command's own payload, emitted before reaching here.
pub(crate) fn render(headers: &[&str], aligns: &[Align], rows: &[Vec<String>], format: Format) {
    match format {
        Format::Json => unreachable!("json is emitted by the command, not the table renderer"),
        Format::Markdown => {
            println!("| {} |", headers.join(" | "));
            let sep: Vec<&str> = headers.iter().map(|_| "---").collect();
            println!("| {} |", sep.join(" | "));
            for row in rows {
                println!("{}", markdown_row(row));
            }
        }
        Format::Table => {
            // Styling wraps a cell only after padding, so ANSI codes never
            // enter the width math.
            let style = Style::stdout();
            let widths = column_widths(headers, rows);
            let header_cells: Vec<String> = headers.iter().map(|h| (*h).to_string()).collect();
            println!(
                "{}",
                style.heading(&format_row(&header_cells, &widths, aligns))
            );
            let total: usize = widths.iter().sum::<usize>() + 2 * widths.len().saturating_sub(1);
            println!("{}", style.dim(&"-".repeat(total)));
            for row in rows {
                println!("{}", format_row(row, &widths, aligns));
            }
        }
    }
}

/// One sanitized markdown body line (`| a | b |`) for a data row. Headers are
/// fixed strings and bypass this; every data cell goes through
/// [`sanitize_cell`] so the `|` structure cannot carry an escape to the
/// terminal.
fn markdown_row(row: &[String]) -> String {
    format!(
        "| {} |",
        row.iter()
            .map(|cell| sanitize_cell(cell))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

/// Column widths over sanitized cells, so escape bytes never widen a column.
fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            widths[col] = widths[col].max(sanitize_cell(cell).chars().count());
        }
    }
    widths
}

fn format_row(cells: &[String], widths: &[usize], aligns: &[Align]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(col, cell)| pad(&sanitize_cell(cell), widths[col], aligns[col]))
        .collect::<Vec<_>>()
        .join("  ")
}

/// Strip ANSI escape sequences from one data cell. Transcript-derived strings
/// (agent/skill ids, project/file paths, friction labels) can carry `ESC`
/// bytes; `render()` is the single choke point where they would reach the
/// terminal, so both the Table and Markdown paths sanitize here, before any
/// width math. Only ASCII-range bytes are ever removed, so multi-byte UTF-8
/// (including Japanese) passes through untouched. Handles:
/// - CSI `ESC [ … final` (e.g. `\x1b[2J`, `\x1b[31m`),
/// - OSC `ESC ] … (BEL | ESC \)` (e.g. `\x1b]0;title\x07`),
/// - other `ESC` + follower sequences and a trailing bare `ESC`,
///   plus a stray `BEL` left over from a split sequence.
fn sanitize_cell(cell: &str) -> String {
    let bytes = cell.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0x1b => {
                // ESC: consume the whole sequence, emit nothing.
                if i + 1 >= bytes.len() {
                    i += 1; // trailing bare ESC
                    continue;
                }
                match bytes[i + 1] {
                    b'[' => {
                        // CSI: params/intermediates until a final byte 0x40-0x7E.
                        i += 2;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                        i += 1; // the final byte itself (or past the end)
                    }
                    b']' => {
                        // OSC: until BEL or ST (`ESC \`), unterminated eats the rest.
                        i += 2;
                        while i < bytes.len() {
                            if bytes[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    b'(' | b')' | b'#' => {
                        // 3-byte sequences (`ESC ( B`, `ESC # 8`, …).
                        i += 3;
                    }
                    _ => {
                        // Not a recognized sequence leader: drop only the
                        // `ESC` byte itself and keep the follower as data.
                        // The follower alone is a harmless printable, while
                        // dropping it would silently eat user data.
                        i += 1;
                    }
                }
            }
            0x07 => {
                i += 1; // stray BEL
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    // Safe: the input was valid UTF-8 and only ASCII-range bytes were removed
    // (`ESC` never occurs inside a multi-byte UTF-8 sequence).
    String::from_utf8(out).expect("sanitize removes ASCII bytes only")
}

fn pad(text: &str, width: usize, align: Align) -> String {
    let fill = " ".repeat(width.saturating_sub(text.chars().count()));
    match align {
        Align::Left => format!("{text}{fill}"),
        Align::Right => format!("{fill}{text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_vocabulary_takes_all_three_values() {
        for (value, expected) in [
            (None, Format::Table),
            (Some("table"), Format::Table),
            (Some("markdown"), Format::Markdown),
            // The `md` shorthand every table view has always taken.
            (Some("md"), Format::Markdown),
            (Some("json"), Format::Json),
        ] {
            assert_eq!(
                ViewFormat::TableMarkdownJson.parse(value).unwrap(),
                expected,
                "{value:?}"
            );
        }
        let err = ViewFormat::TableMarkdownJson
            .parse(Some("xml"))
            .expect_err("an unaccepted value must fail");
        assert_eq!(
            err.to_string(),
            "unknown --format 'xml' (table|markdown|json)"
        );
    }

    #[test]
    fn sanitize_strips_csi_sequences() {
        // Synthetic hostile cells: CSI `ESC [` with params and finals.
        assert_eq!(sanitize_cell("a\x1b[2Jb"), "ab");
        assert_eq!(sanitize_cell("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(sanitize_cell("\x1b[1;36mhead\x1b[0m"), "head");
    }

    #[test]
    fn sanitize_strips_osc_sequences() {
        // OSC `ESC ]` terminated by BEL or ST (`ESC \`).
        assert_eq!(sanitize_cell("a\x1b]0;title\x07b"), "ab");
        assert_eq!(sanitize_cell("a\x1b]0;title\x1b\\b"), "ab");
    }

    #[test]
    fn sanitize_strips_bare_escapes() {
        // A lone ESC plus its follower, and a trailing ESC with nothing after.
        // Only the ESC byte is dropped; the follower is harmless data.
        assert_eq!(sanitize_cell("a\x1bb"), "ab");
        assert_eq!(sanitize_cell("a\x1b"), "a");
        assert_eq!(sanitize_cell("\x1bM"), "M");
        // No ESC byte ever survives, whatever the mix.
        let mixed = "\x1b[2J\x1b]0;t\x07\x1bM\x1bplain 日本語";
        let clean = sanitize_cell(mixed);
        assert!(!clean.contains('\x1b'), "{clean:?}");
        assert_eq!(clean, "Mplain 日本語");
    }

    #[test]
    fn sanitize_leaves_normal_text_untouched() {
        // Boundary: no escapes, ASCII mixed with Japanese — must not change.
        assert_eq!(sanitize_cell("hello"), "hello");
        assert_eq!(sanitize_cell("日本語テスト"), "日本語テスト");
        assert_eq!(sanitize_cell("mixed 日本語 abc"), "mixed 日本語 abc");
    }

    #[test]
    fn markdown_row_sanitizes_cells_but_keeps_structure() {
        let row = vec!["a\x1b[2Jb".to_string(), "plain".to_string()];
        assert_eq!(markdown_row(&row), "| ab | plain |");
    }

    #[test]
    fn table_widths_ignore_escapes() {
        // The escape bytes must not count toward column width.
        let rows = vec![vec!["\x1b[31mab\x1b[0m".to_string()]];
        assert_eq!(column_widths(&["h"], &rows), vec![2]);
        assert_eq!(format_row(&rows[0], &[2], &[Align::Left]), "ab");
    }

    #[test]
    fn doctors_vocabulary_takes_table_and_json_only() {
        for (value, expected) in [
            (None, Format::Table),
            (Some("table"), Format::Table),
            (Some("json"), Format::Json),
        ] {
            assert_eq!(
                ViewFormat::TableJson.parse(value).unwrap(),
                expected,
                "{value:?}"
            );
        }
        // `markdown` (and the `md` shorthand that resolves to it elsewhere)
        // is refused, and the message names doctor's own two values — the
        // shared three-value message would advertise a format it rejects.
        for value in ["markdown", "md", "xml"] {
            let err = ViewFormat::TableJson
                .parse(Some(value))
                .expect_err("an unaccepted value must fail");
            assert_eq!(
                err.to_string(),
                format!("unknown --format '{value}' (table|json)")
            );
        }
    }
}
