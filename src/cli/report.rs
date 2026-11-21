//! The report layer's presentation primitives: the output format, the table
//! renderer behind every view, and the framing block above a table.

use anyhow::Result;

use crate::core::optimize::AdvisorLanguage;
use crate::core::period::TimeRange;
use crate::session_env::LanguageSource;

use super::{eprint_window, framed, with_window};

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

/// Which window a view renders under, for the shared spine
/// ([`render_view`]): the format it produces, and the single window the
/// spine discloses, attaches, and frames — or none, where the caller owns
/// the disclosure (trend's comparison pair, whose two windows ride their
/// own disclosure and framing).
pub(crate) struct ViewCtx {
    pub format: Format,
    pub window: Option<TimeRange>,
}

impl ViewCtx {
    /// One single-window view: the spine discloses the window on stderr,
    /// attaches it to the JSON payload, and frames the preamble with it.
    pub(crate) fn single(format: Format, window: TimeRange) -> Self {
        Self {
            format,
            window: Some(window),
        }
    }

    /// Trend's pair shape: no single window exists, so the spine emits the
    /// JSON as given and renders the preamble text as given — the caller
    /// discloses its own pair and frames its own preamble beforehand.
    pub(crate) fn pair(format: Format) -> Self {
        Self {
            format,
            window: None,
        }
    }
}

/// One table's data: headers, per-column alignment, and rows. Views assemble
/// these and hand them to [`render_view`]'s body; the spine never builds a
/// row itself, so it cannot drift a view's columns.
pub(crate) struct TableSpec<'a> {
    pub headers: &'a [&'a str],
    pub aligns: &'a [Align],
    pub rows: Vec<Vec<String>>,
}

impl TableSpec<'_> {
    /// Render this table in the view's format. `json` never reaches here:
    /// the spine short-circuits it before the body runs, so [`render`]'s
    /// `unreachable` stays the single enforcement point.
    pub(crate) fn render(&self, format: Format) {
        render(self.headers, self.aligns, &self.rows, format);
    }
}

/// The one ritual every windowed table view shares, in order: window
/// disclosure on stderr, the JSON short-circuit (window-attached), the
/// empty short-circuit, the framed preamble, then the caller's tables.
/// Views assemble their data (the JSON value, the empty message, the
/// preamble text, the [`TableSpec`]s) and hand the rendering to the body;
/// the spine owns the order and the branches, never the rows.
///
/// The empty message arrives final (built with `empty_in_window` or the
/// view's own wording): printing it is the spine's, wording it the view's.
pub(crate) fn render_view(
    ctx: ViewCtx,
    json: serde_json::Value,
    empty_msg: Option<&str>,
    preamble_text: &str,
    body: impl FnOnce(Format),
) -> Result<()> {
    if let Some(window) = ctx.window {
        eprint_window(window);
    }
    if ctx.format == Format::Json {
        return emit_json(&match ctx.window {
            Some(window) => with_window(json, window),
            None => json,
        });
    }
    if let Some(message) = empty_msg {
        println!("{message}");
        return Ok(());
    }
    match ctx.window {
        Some(window) => preamble(ctx.format, &framed(preamble_text, window)),
        None => preamble(ctx.format, preamble_text),
    }
    body(ctx.format);
    Ok(())
}

/// The inventory JSON `status` for an unused surface: `unused` without the
/// flags, `unused-in-window` under them. The single decision point behind
/// the call-site inline `window.is_all()` the migration removes (PBI C4) —
/// `window.rs` is frozen, so the vocabulary lives here beside
/// [`render_view`], not scattered at the call site. Table cells keep
/// `unused_label` (uppercase); the JSON contract's lowercase values are
/// this function's.
pub(crate) fn unused_status(window: TimeRange) -> &'static str {
    match window.is_all() {
        true => "unused",
        false => "unused-in-window",
    }
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
            .map(|cell| escape_markdown_cell(cell))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

/// Escape one data cell for the markdown table: sanitize first (no escape to
/// the terminal), then backslash-escape `|` so it reads as data rather than a
/// column break, then fold newlines to `<br>` so one logical row stays one
/// physical line. Headers are fixed strings and bypass this; the Table path
/// is unaffected (spaces separate its columns, pipes are harmless there).
fn escape_markdown_cell(cell: &str) -> String {
    sanitize_cell(cell)
        .replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace(['\r', '\n'], "<br>")
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

/// Strip terminal-hostile characters from one data cell: every ESC-led
/// sequence (CSI, OSC, charset designators, bare `ESC`), every Cc control
/// char — C0, C1 (U+0080-009F, riding UTF-8 as 0xC2 0x80-0x9F), and DEL —
/// except the line structure `\n` and tab `\t` that prose layout (and the
/// markdown `<br>` fold) relies on, and the Cf bidi overrides / isolates /
/// zero-width format chars (U+202A-202E, U+2066-2069, U+200B-200F, U+FEFF)
/// that display invisibly. Transcript-derived strings (agent/skill ids,
/// project/file
/// paths, friction labels) are untrusted; `render()` is the choke point
/// where they would reach the terminal, so both the Table and Markdown
/// paths sanitize here, before any width math. Doctor's prose bypasses
/// `render()` (it is its own text layout), so it routes its interpolated
/// values through this function directly. Kept chars re-encode verbatim,
/// so multi-byte text (including Japanese) passes through untouched.
/// Handles:
/// - CSI `ESC [ … final` (e.g. `\x1b[2J`, `\x1b[31m`),
/// - OSC `ESC ] … (BEL | ESC \)` (e.g. `\x1b]0;title\x07`),
/// - other `ESC` + follower sequences and a trailing bare `ESC`,
///   plus a stray `BEL` left over from a split sequence.
pub(crate) fn sanitize_cell(cell: &str) -> String {
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
                        // 3-byte sequences (`ESC ( B`, `ESC # 8`, …). The
                        // designator is a single byte in a well-formed
                        // sequence; consuming a whole char keeps the scan on
                        // a char boundary if a multi-byte char follows.
                        i += 2;
                        if i < bytes.len() {
                            let ch = cell[i..].chars().next().expect("input is valid UTF-8");
                            i += ch.len_utf8();
                        }
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
            // Line structure is kept: prose layout, and markdown folds `\n`.
            b'\n' | b'\t' => {
                out.push(bytes[i]);
                i += 1;
            }
            // Every other C0 (CR included) and DEL: a terminal acts on them —
            // CR alone rewrites the line head.
            0x00..=0x06 | 0x08 | 0x0b..=0x1a | 0x1c..=0x1f | 0x7f => {
                i += 1;
            }
            // Multi-byte UTF-8: decode the whole char so C1 and Cf are
            // recognized as chars while real text survives untouched.
            _ if bytes[i] >= 0x80 => {
                let ch = cell[i..].chars().next().expect("input is valid UTF-8");
                i += ch.len_utf8();
                if !is_stripped_char(ch) {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    // Safe: only whole chars were removed and every kept char was re-encoded
    // verbatim, so `out` is the input's char sequence minus the stripped ones.
    String::from_utf8(out).expect("sanitize removes whole chars only")
}

/// A non-ESC char the strip vocabulary removes: any Cc control except the
/// kept line structure `\n` / `\t`, plus the Cf bidi overrides / isolates and
/// zero-width / direction-marking format chars (`is_control()` misses the
/// latter — they are Cf, and they misrepresent or spoof the visible text).
fn is_stripped_char(c: char) -> bool {
    (c.is_control() && !matches!(c, '\n' | '\t'))
        || matches!(
            c as u32,
            0x202a..=0x202e | 0x2066..=0x2069 | 0x200b..=0x200f | 0xfeff
        )
}

fn pad(text: &str, width: usize, align: Align) -> String {
    let fill = " ".repeat(width.saturating_sub(text.chars().count()));
    match align {
        Align::Left => format!("{text}{fill}"),
        Align::Right => format!("{fill}{text}"),
    }
}

/// The `--verbose`-only decision line (PBI 2026-11-14-02): the resolved
/// language code and the layer that named it, in one greppable line.
/// Rendered on stderr by the caller — never into prompts or JSON — so normal
/// output stays byte-identical with or without `--verbose`.
pub fn language_decision_line(source: LanguageSource, language: AdvisorLanguage) -> String {
    format!(
        "language: {} (decided by {})",
        language.label(),
        source.label()
    )
}

/// Print [`language_decision_line`] on stderr, dimmed like the other shell
/// chatter — a no-op unless `verbose`. The shell calls this with its own
/// flag, so the condition lives in one tested place and the shell grows by
/// one call line instead of a branch.
pub fn eprint_language_decision(verbose: bool, source: LanguageSource, language: AdvisorLanguage) {
    if verbose {
        eprintln!(
            "{}",
            Style::stderr().dim(&language_decision_line(source, language))
        );
    }
}

/// Print a freshness line to stderr — dimmed context normally, highlighted
/// when it carries a staleness hint the user should act on.
pub(crate) fn eprint_freshness(line: &str) {
    let style = Style::stderr();
    if line.contains("saniwalens analyze") {
        eprintln!("{}", style.warn(line));
    } else {
        eprintln!("{}", style.dim(line));
    }
}

/// The `--frozen` freshness header: how old the store is, with a refresh hint
/// once it is older than a day. This is what lets the user *notice* staleness.
pub(crate) fn freshness_line(analyzed_at: Option<&str>) -> String {
    let Some(parsed) = analyzed_at.and_then(crate::time::parse_rfc3339_nanos) else {
        return "store: freshness unknown (no analyze recorded) — run `saniwalens analyze`"
            .to_string();
    };
    // Whole seconds between now and the stamp, truncated toward zero — the
    // old `(Utc::now() - parsed).num_seconds()`.
    let age_secs = ((crate::time::now_nanos() - parsed) / 1_000_000_000) as i64;
    let hint = if age_secs >= 24 * 3600 {
        " — run `saniwalens analyze` (or drop --frozen) to refresh"
    } else {
        ""
    };
    format!(
        "store: analyzed {} ago (--frozen){hint}",
        humanize_age(age_secs)
    )
}

fn humanize_age(secs: i64) -> String {
    let secs = secs.max(0);
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

/// ANSI styling for the human formats. Colors turn on only when the target is
/// a terminal, `NO_COLOR` is unset, and `TERM` is not `dumb` — so pipes,
/// captures, and `--format markdown|json` always get plain bytes.
pub(crate) struct Style {
    on: bool,
}

impl Style {
    pub(crate) fn stdout() -> Self {
        use std::io::IsTerminal;
        Self {
            on: use_color(std::io::stdout().is_terminal()),
        }
    }

    pub(crate) fn stderr() -> Self {
        use std::io::IsTerminal;
        Self {
            on: use_color(std::io::stderr().is_terminal()),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub(crate) fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }

    pub(crate) fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    /// Section headings (`== … ==`, table headers).
    pub(crate) fn heading(&self, text: &str) -> String {
        self.paint("1;36", text)
    }

    /// Pointers to follow-up commands.
    pub(crate) fn command(&self, text: &str) -> String {
        self.paint("36", text)
    }

    /// Attention: a finding's count, a wedge label, a staleness warning.
    pub(crate) fn warn(&self, text: &str) -> String {
        self.paint("33", text)
    }

    pub(crate) fn good(&self, text: &str) -> String {
        self.paint("32", text)
    }
}

fn use_color(tty: bool) -> bool {
    tty && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(512))]

        /// The sanitize contract over arbitrary input: idempotent, hostile
        /// vocabulary gone (Cc minus line structure, Cf bidi/zero-width),
        /// and multi-byte text re-encoded verbatim.
        #[test]
        fn sanitize_cell_is_idempotent_and_drops_hostile_bytes(s in ".*") {
            let once = sanitize_cell(&s);
            let once_clone = once.clone();
            let twice = sanitize_cell(&once);
            prop_assert_eq!(twice, once_clone);

            for c in once.chars() {
                prop_assert!(
                    !(c.is_control() && !matches!(c, '\n' | '\t')
                        || matches!(
                            c as u32,
                            0x202a..=0x202e | 0x2066..=0x2069 | 0x200b..=0x200f | 0xfeff
                        )),
                    "hostile char U+{:04X} survived: {once:?}",
                    c as u32
                );
            }
            // Japanese passes through untouched: the doc's multi-byte promise.
            let jp = sanitize_cell("使用状況は変わりません");
            prop_assert_eq!(jp, "使用状況は変わりません");
        }
    }

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
    fn sanitize_strips_c1_controls_and_del() {
        // C1 rides UTF-8 (0xC2 0x80-0x9F); U+009B is the 8-bit CSI a terminal
        // would act on, and DEL is a C0 the byte scan used to keep.
        assert_eq!(sanitize_cell("a\u{9b}31mb"), "a31mb");
        assert_eq!(sanitize_cell("a\u{85}b"), "ab");
        assert_eq!(sanitize_cell("a\u{7f}b"), "ab");
        for cp in 0x80u32..=0x9f {
            let c = char::from_u32(cp).expect("C1 block is scalar");
            assert_eq!(
                sanitize_cell(&format!("a{c}b")),
                "ab",
                "U+{cp:04X} survived"
            );
        }
    }

    #[test]
    fn sanitize_strips_cr_but_keeps_line_structure() {
        // CR rewrites the line head; \n and \t are prose layout (and the
        // markdown <br> fold relies on \n surviving).
        assert_eq!(sanitize_cell("a\rb"), "ab");
        assert_eq!(sanitize_cell("\roverwrite"), "overwrite");
        assert_eq!(sanitize_cell("a\nb"), "a\nb");
        assert_eq!(sanitize_cell("a\tb"), "a\tb");
        assert_eq!(sanitize_cell("a\r\nb"), "a\nb");
    }

    #[test]
    fn sanitize_strips_bidi_overrides_and_zero_width() {
        // Cf chars are invisible (`is_control()` misses them): an override
        // reorders what the user reads, isolates scope the reordering, and
        // zero-width chars spoof copied text.
        for cp in [
            0x202a, 0x202b, 0x202c, 0x202d, 0x202e, 0x2066, 0x2067, 0x2068, 0x2069, 0x200b, 0x200c,
            0x200d, 0x200e, 0x200f, 0xfeff,
        ] {
            let c = char::from_u32(cp).expect("listed codepoints are scalar");
            assert_eq!(
                sanitize_cell(&format!("a{c}b")),
                "ab",
                "U+{cp:04X} survived"
            );
        }
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
    fn markdown_cells_escape_pipes() {
        // A literal `|` must read as data, never as a column break.
        assert_eq!(escape_markdown_cell("a|b"), "a\\|b");
        assert_eq!(escape_markdown_cell("|lead"), "\\|lead");
        assert_eq!(escape_markdown_cell("trail|"), "trail\\|");
        assert_eq!(escape_markdown_cell("a||b"), "a\\|\\|b");
        assert_eq!(escape_markdown_cell("plain"), "plain");
        // Headers bypass escaping (fixed strings); only body cells escape.
        let row = vec!["a|b".to_string(), "c".to_string()];
        assert_eq!(markdown_row(&row), "| a\\|b | c |");
    }

    #[test]
    fn markdown_cells_fold_newlines() {
        // One logical row stays one physical line: newlines become `<br>`.
        // `\r` never survives sanitize_cell, so "a\rb" collapses to "ab" —
        // the fold only ever sees `\n`.
        assert_eq!(escape_markdown_cell("a\nb"), "a<br>b");
        assert_eq!(escape_markdown_cell("a\r\nb"), "a<br>b");
        assert_eq!(escape_markdown_cell("a\rb"), "ab");
        assert_eq!(escape_markdown_cell("trail\n"), "trail<br>");
        assert_eq!(escape_markdown_cell("plain"), "plain");
        let row = vec!["a\nb|c".to_string()];
        assert_eq!(markdown_row(&row), "| a<br>b\\|c |");
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

    #[test]
    fn language_decision_line_names_source_and_code() {
        // PBI 2026-11-14-02: the `--verbose`-only line carries the resolved
        // code and the winning layer in one greppable line.
        assert_eq!(
            language_decision_line(LanguageSource::Config, AdvisorLanguage::Ja),
            "language: ja (decided by config)"
        );
        assert_eq!(
            language_decision_line(LanguageSource::Flag, AdvisorLanguage::En),
            "language: en (decided by flag)"
        );
        assert_eq!(
            language_decision_line(LanguageSource::Locale, AdvisorLanguage::Ja),
            "language: ja (decided by locale)"
        );
        assert_eq!(
            language_decision_line(LanguageSource::Default, AdvisorLanguage::En),
            "language: en (decided by default)"
        );
    }
}
