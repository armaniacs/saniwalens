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
                println!("| {} |", row.join(" | "));
            }
        }
        Format::Table => {
            // Styling wraps a cell only after padding, so ANSI codes never
            // enter the width math.
            let style = Style::stdout();
            let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
            for row in rows {
                for (col, cell) in row.iter().enumerate() {
                    widths[col] = widths[col].max(cell.chars().count());
                }
            }
            let format_row = |cells: &[String]| -> String {
                cells
                    .iter()
                    .enumerate()
                    .map(|(col, cell)| pad(cell, widths[col], aligns[col]))
                    .collect::<Vec<_>>()
                    .join("  ")
            };
            let header_cells: Vec<String> = headers.iter().map(|h| (*h).to_string()).collect();
            println!("{}", style.heading(&format_row(&header_cells)));
            let total: usize = widths.iter().sum::<usize>() + 2 * widths.len().saturating_sub(1);
            println!("{}", style.dim(&"-".repeat(total)));
            for row in rows {
                println!("{}", format_row(row));
            }
        }
    }
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
