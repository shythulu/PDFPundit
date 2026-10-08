//! Markdown emitter (T-32b, TD §5.6, D-060).
//!
//! [`to_markdown`] writes GitHub-flavoured Markdown from T-32a's page layouts.
//! It is a pure function of its arguments: the same layouts and options give
//! the same bytes on every run and platform, and nothing here looks at the
//! disk. Where the images directory is, and what the file is called, is the
//! export job's business (`jobs.rs`, `place.rs`); this module only writes the
//! directory's name into the image links.
//!
//! What it writes, per page, blocks separated by one blank line:
//! - with [`MarkdownOptions::page_separators`], a `<!-- page N -->` comment
//!   (N counts from 1);
//! - each [`LayoutNote`] as a visible comment, so text that is on the page but
//!   was not extracted is never dropped silently (TD §5.6). The notes stand
//!   for text OCR would read; OCR (M9, feature `ocr`) has no reader yet, so
//!   they are written in every build;
//! - headings as ATX `#` lines, paragraphs as their lines (soft breaks),
//!   lists as `-` or `1.` items, tables as GFM pipe tables (a table without a
//!   header row gets an empty one: GFM needs it), images as
//!   `![](<dir>/<name>)` links into the images directory, rules as `---`.
//!
//! Inline text is escaped so that a PDF's own characters never become
//! Markdown syntax: `\`, `` ` ``, `*`, `_`, `[`, `]`, `<`, `>`, `~` always,
//! `&` before something that could read as an entity, `|` in table cells, and
//! a line start that would open a block (`#`, `-`, `+`, `=`, `1.`, `1)`).
//! Bold and italic runs become `**…**` and `*…*` with their outer spaces moved
//! outside the markers (best effort: CommonMark's flanking rules leave the
//! markers as text where a styled run starts or ends with punctuation glued
//! to a word); monospaced runs become code spans. A link keeps only
//! `http`, `https`, `mailto` and `ftp` targets; any other scheme (a
//! `javascript:` action, say) keeps its text and loses the link.
//!
//! The output never names the app or anything the UI draws (goal point 3;
//! `src/engine/artefact_tests.rs` checks the fixtures' output against the UI
//! string table).

#[cfg(test)]
mod tests;

use std::fmt::Write as _;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use super::layout::{Block, LayoutNote, Line, PageLayout, Run};

/// How [`to_markdown`] writes a document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarkdownOptions {
    /// A `<!-- page N -->` comment before each page.
    pub page_separators: bool,
    /// The images directory's file name (D-060's content-derived
    /// `<stem>.<hash8>.images`, or its `(N)` form), never a path. `None` when
    /// no images were written: an image block is then a comment saying so.
    pub image_dir_name: Option<String>,
}

/// The Markdown for `doc`, its pages in order. Ends with one newline, or is
/// empty when there is nothing to write.
pub fn to_markdown(doc: &[PageLayout], opts: &MarkdownOptions) -> String {
    let mut parts: Vec<String> = Vec::new();
    for page in doc {
        if opts.page_separators {
            parts.push(format!("<!-- page {} -->", u64::from(page.index) + 1));
        }
        parts.extend(page.notes.iter().map(note));
        parts.extend(page.blocks.iter().filter_map(|b| block(b, opts)));
    }
    let mut out = parts.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

// ── notes ───────────────────────────────────────────────────────────────

/// `n thing` or `n things`.
fn count(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The visible comment for text the page holds but the export does not
/// (TD §5.6). The outlined-text wording is the TD's.
fn note(n: &LayoutNote) -> String {
    let text = match *n {
        LayoutNote::OutlinedText { paths, contours } => format!(
            "text drawn as outlines on this page ({}, {}) was not extracted",
            count(paths, "path", "paths"),
            count(contours, "contour", "contours"),
        ),
        LayoutNote::Type3Text => "this page draws text with Type 3 glyph procedures; \
             text they do not map to Unicode was not extracted"
            .to_owned(),
        LayoutNote::ImageOnly => {
            "this page is images only; any text in them was not extracted".to_owned()
        }
        LayoutNote::Unmapped { glyphs } => format!(
            "{} on this page {} not extracted (no Unicode mapping, invisible, or under 2 pt)",
            count(glyphs, "glyph", "glyphs"),
            if glyphs == 1 { "was" } else { "were" },
        ),
    };
    comment(&text)
}

/// An HTML comment holding `text`, which can neither end it early nor run
/// over a line.
fn comment(text: &str) -> String {
    let mut safe: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    while safe.contains("--") {
        safe = safe.replace("--", "- -");
    }
    format!("<!-- {} -->", safe.trim())
}

// ── blocks ──────────────────────────────────────────────────────────────

/// One block's Markdown, or `None` when it holds no text.
fn block(b: &Block, opts: &MarkdownOptions) -> Option<String> {
    let text = match b {
        Block::Heading { level, runs } => {
            let text = inline(runs, true);
            if text.is_empty() {
                return None;
            }
            format!("{} {text}", "#".repeat(usize::from((*level).clamp(1, 6))))
        }
        Block::Paragraph(lines) => {
            let lines: Vec<String> = lines
                .iter()
                .map(|l| inline(&l.0, false))
                .filter(|l| !l.is_empty())
                .collect();
            lines.join("\n")
        }
        Block::List { ordered, items } => list(*ordered, items),
        Block::Table { rows, header } => table(rows, *header),
        Block::Image { name, .. } => match &opts.image_dir_name {
            Some(dir) => format!("![]({}/{})", path_part(dir), path_part(name)),
            None => comment(&format!("image {name} was not written")),
        },
        Block::Rule => "---".to_owned(),
    };
    (!text.is_empty()).then_some(text)
}

/// `- item` or `1. item`, numbered from 1 over the items that hold text;
/// an item's later lines are indented to its text.
fn list(ordered: bool, items: &[Vec<Line>]) -> String {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let lines: Vec<String> = item
            .iter()
            .map(|l| inline(&l.0, false))
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        let marker = if ordered {
            format!("{}.", out.len() + 1)
        } else {
            "-".to_owned()
        };
        let indent = " ".repeat(marker.chars().count() + 1);
        let mut text = format!("{marker} {}", lines[0]);
        for l in &lines[1..] {
            let _ = write!(text, "\n{indent}{l}");
        }
        out.push(text);
    }
    out.join("\n")
}

/// A GFM pipe table, every row as wide as the widest. Without a header row
/// the first line is an empty header, since GFM requires one.
fn table(rows: &[Vec<String>], header: bool) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    let row = |cells: &[String]| {
        let mut out = String::from("|");
        for i in 0..width {
            let cell = cells.get(i).map_or_else(String::new, |c| cell(c));
            if cell.is_empty() {
                out.push_str("  |");
            } else {
                let _ = write!(out, " {cell} |");
            }
        }
        out
    };
    let rule = format!("|{}", " --- |".repeat(width));
    let (head, body) = match rows.split_first() {
        Some((first, rest)) if header => (row(first), rest),
        _ => (row(&[]), rows),
    };
    let mut lines = vec![head, rule];
    lines.extend(body.iter().map(|r| row(r)));
    lines.join("\n")
}

/// One table cell: one line, inline-escaped, `|` escaped too.
fn cell(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    escape(collapse(&flat).trim()).replace('|', "\\|")
}

// ── inline text ─────────────────────────────────────────────────────────

/// The Markdown for one line of runs: styled, linked and escaped, the line
/// start made safe. `heading` escapes every `#` (a trailing ` #` would
/// otherwise close the heading).
fn inline(runs: &[Run], heading: bool) -> String {
    let runs = merge(runs);
    let mut out = String::new();
    let mut i = 0;
    while i < runs.len() {
        let link = runs[i].link.as_deref().filter(|uri| allowed(uri));
        let mut j = i + 1;
        while j < runs.len() && runs[j].link.as_deref().filter(|uri| allowed(uri)) == link {
            j += 1;
        }
        let text: String = runs[i..j].iter().map(styled).collect();
        match link {
            Some(uri) if !text.trim().is_empty() => {
                let (lead, body, trail) = split_spaces(&text);
                let _ = write!(out, "{lead}[{body}]({}){trail}", destination(uri));
            }
            _ => out.push_str(&text),
        }
        i = j;
    }
    let out = collapse(&out);
    let out = out.trim();
    let out = if heading {
        out.replace('#', "\\#")
    } else {
        out.to_owned()
    };
    line_start(&out)
}

/// Adjacent runs with the same style and link joined, so two bold runs never
/// write `**a****b**`.
fn merge(runs: &[Run]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for r in runs {
        let text: String = r
            .text
            .chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .collect();
        match out.last_mut() {
            Some(last)
                if (last.bold, last.italic, last.mono, &last.link)
                    == (r.bold, r.italic, r.mono, &r.link) =>
            {
                last.text.push_str(&text);
            }
            _ => out.push(Run { text, ..r.clone() }),
        }
    }
    out
}

/// One run with its style, the run's outer spaces outside the markers.
fn styled(r: &Run) -> String {
    let (lead, body, trail) = split_spaces(&r.text);
    if body.is_empty() {
        return r.text.clone();
    }
    let body = if r.mono {
        code_span(body)
    } else {
        escape(body)
    };
    let mark = match (r.bold, r.italic) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => "",
    };
    format!("{lead}{mark}{body}{mark}{trail}")
}

/// `(leading spaces, the rest trimmed, trailing spaces)`.
fn split_spaces(s: &str) -> (&str, &str, &str) {
    let body = s.trim_matches(' ');
    let lead = &s[..s.len() - s.trim_start_matches(' ').len()];
    let trail = &s[s.trim_end_matches(' ').len()..];
    (lead, body, trail)
}

/// A code span around `text`: a fence one backtick longer than its longest
/// run of them, padded when it starts or ends with one.
fn code_span(text: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    if text.starts_with('`') || text.ends_with('`') {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

/// `text` with every character Markdown could read as syntax escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '~' => {
                out.push('\\');
                out.push(c);
            }
            '&' if entity_follows(&text[i + 1..]) => out.push_str("\\&"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Whether `rest`, the text after an `&`, would make it an entity or a
/// numeric character reference (`&copy;`, `&#169;`, `&#xA9;`): "AT&T" stays
/// as it is.
fn entity_follows(rest: &str) -> bool {
    let Some(end) = rest.find(';') else {
        return false;
    };
    let body = &rest[..end];
    if let Some(num) = body.strip_prefix('#') {
        let (digits, radix) = match num.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16),
            None => (num, 10),
        };
        (1..=7).contains(&digits.len()) && digits.chars().all(|c| c.is_digit(radix))
    } else {
        (1..=32).contains(&body.len())
            && body.starts_with(|c: char| c.is_ascii_alphabetic())
            && body.chars().all(|c| c.is_ascii_alphanumeric())
    }
}

/// Runs of spaces outside code spans collapse to one. A code span keeps its
/// inner spaces; it never holds two in a row from a layout's runs anyway, so
/// collapsing everywhere is safe and keeps the output tidy.
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for c in s.chars() {
        if c == ' ' {
            if !last_space {
                out.push(c);
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out
}

/// `line` with a start that would open a block escaped: an ATX heading
/// (`#`), a list item or rule (`-`, `+`, `*` is already escaped), a setext
/// underline (`=`), or an ordered item (`1.` or `1)` before a space).
fn line_start(line: &str) -> String {
    match line.chars().next() {
        Some('#' | '-' | '+' | '=') => return format!("\\{line}"),
        Some(c) if c.is_ascii_digit() => {}
        _ => return line.to_owned(),
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let mut rest = line[digits..].chars();
    match (rest.next(), rest.next()) {
        (Some(d @ ('.' | ')')), None | Some(' ')) if digits <= 9 => {
            format!("{}\\{d}{}", &line[..digits], &line[digits + 1..])
        }
        _ => line.to_owned(),
    }
}

// ── links and paths ─────────────────────────────────────────────────────

/// The schemes a link may point to. Anything else keeps its text only.
const SCHEMES: [&str; 5] = ["http:", "https:", "mailto:", "ftp:", "ftps:"];

fn allowed(uri: &str) -> bool {
    SCHEMES.iter().any(|s| {
        uri.len() > s.len()
            && uri.is_char_boundary(s.len())
            && uri[..s.len()].eq_ignore_ascii_case(s)
    })
}

/// What a link target may not hold as is: controls, spaces and the
/// characters that end or confuse a Markdown destination.
const DEST: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'(')
    .add(b')')
    .add(b'\\')
    .add(b'`')
    .add(b'[')
    .add(b']');

/// A link destination: `uri` with the characters [`DEST`] lists
/// percent-encoded (an existing `%xx` is kept as it is).
fn destination(uri: &str) -> String {
    utf8_percent_encode(uri, DEST).to_string()
}

/// What a path segment may not hold as is: [`DEST`], plus `%`, `#` and `?`,
/// which a viewer would read as an escape, a fragment or a query.
const SEGMENT: &AsciiSet = &DEST.add(b'%').add(b'#').add(b'?').add(b'/');

/// One relative path segment of an image link (a directory or file name).
fn path_part(name: &str) -> String {
    utf8_percent_encode(name, SEGMENT).to_string()
}
