//! The browse picker (T-37, DA M1): an in-TUI, `.pdf`-filtered file picker,
//! the guaranteed way in for a terminal that does not paste a path on drop
//! (D-034). It picks; it never submits. Confirming hands the picked paths to
//! the loop, which sends them through the same drop gate and the same
//! `Dropped{n}` chomp as a paste, so the cat stays the way in.
//!
//! The keyboard model is [`BrowseState::key`]; the listing is `ui::fs`'s.
//! The picker is drawn as the other modals are (their box style, lightbar
//! and theme roles): over the full layout, which steps back behind it, or
//! across the whole widget tile. The one-line fallback takes no input
//! (D-064), so it has no picker. The look is the panel's own and is checked
//! against a self-golden until the user supplies a mockup frame (D-048).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::modals::{modal_style, step_back};
use super::{FULL_SIZE, LayoutKind, WIDGET_SIZE};
use crate::ui::canvas::{BoxStyle, Boxed, Canvas, plain_len};
use crate::ui::fs::{self, Entry, Filter};
use crate::ui::strings;
use crate::ui::theme::Theme;
use crate::ui::widgets::r#box::fit;

/// The picker: the folder it shows, what is in it, the cursor and the files
/// picked so far (from any folder visited).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrowseState {
    pub cwd: PathBuf,
    /// The listing of `cwd`, folders first ([`fs::list`]).
    pub entries: Vec<Entry>,
    /// Into `entries`; `entries.len()` is the "feed the cat" button.
    pub cursor: usize,
    /// Every picked file, as `cwd` joined with its name when it was picked.
    pub selected: BTreeSet<PathBuf>,
    pub filter: Filter,
    /// Hidden entries are listed (the `.` key).
    pub hidden: bool,
    /// `cwd` could not be listed; `entries` is empty.
    pub unreadable: bool,
}

/// A key as the picker reads it; the loop maps the terminal's keys onto it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Backspace,
    Esc,
    Char(char),
}

/// What a key asks of the loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowseAction {
    /// The picker stays open (its state may have changed).
    Stay,
    /// Close the picker; nothing is fed.
    Cancel,
    /// Close the picker and feed these paths to the cat, in order.
    Feed(Vec<PathBuf>),
}

impl BrowseState {
    /// The picker on `dir`, the cursor on its first entry.
    pub fn open(dir: &Path) -> BrowseState {
        let mut s = BrowseState {
            cwd: dir.to_path_buf(),
            ..BrowseState::default()
        };
        s.relist();
        s
    }

    /// Lists `cwd` again; the cursor goes back to the top.
    fn relist(&mut self) {
        let listed = fs::list(&self.cwd, self.filter, self.hidden);
        self.unreadable = listed.is_err();
        self.entries = listed.unwrap_or_default();
        self.cursor = 0;
    }

    /// The button's place in the cursor's range.
    fn button(&self) -> usize {
        self.entries.len()
    }

    /// The entry under the cursor; `None` on the button.
    fn current(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    fn path_of(&self, e: &Entry) -> PathBuf {
        self.cwd.join(&e.name)
    }

    /// The keyboard model:
    ///
    /// | key | does |
    /// |---|---|
    /// | `↑` `↓` | moves the cursor over the entries and the button, stopping at either end |
    /// | Enter on a folder, `→` | opens it |
    /// | Enter on a file | picks it and moves to the button |
    /// | Enter on the button | feeds the picked files, if any |
    /// | Space | picks or unpicks the file under the cursor, then moves down |
    /// | `a` | picks every `.pdf` in view |
    /// | Backspace, `←` | goes up a folder, the cursor on the one it left |
    /// | `.` | shows or hides hidden entries |
    /// | Esc | closes the picker |
    pub fn key(&mut self, key: BrowseKey) -> BrowseAction {
        match key {
            BrowseKey::Up => self.cursor = self.cursor.saturating_sub(1),
            BrowseKey::Down => self.cursor = (self.cursor + 1).min(self.button()),
            BrowseKey::Enter => match self.current().cloned() {
                Some(e) if e.is_dir => self.descend(&e),
                Some(e) => {
                    self.selected.insert(self.path_of(&e));
                    self.cursor = self.button();
                }
                None if self.selected.is_empty() => {}
                None => return BrowseAction::Feed(self.selected.iter().cloned().collect()),
            },
            BrowseKey::Right => {
                if let Some(e) = self.current().filter(|e| e.is_dir).cloned() {
                    self.descend(&e);
                }
            }
            BrowseKey::Backspace | BrowseKey::Left => self.up(),
            BrowseKey::Char(' ') => {
                if let Some(e) = self.current().filter(|e| !e.is_dir).cloned() {
                    let path = self.path_of(&e);
                    if !self.selected.remove(&path) {
                        self.selected.insert(path);
                    }
                    self.cursor = (self.cursor + 1).min(self.button());
                }
            }
            BrowseKey::Char('a' | 'A') => {
                let files: Vec<PathBuf> = self
                    .entries
                    .iter()
                    .filter(|e| !e.is_dir)
                    .map(|e| self.path_of(e))
                    .collect();
                self.selected.extend(files);
            }
            BrowseKey::Char('.') => {
                let at = self.current().map(|e| e.name.clone());
                self.hidden = !self.hidden;
                self.relist();
                if let Some(i) = at.and_then(|n| self.entries.iter().position(|e| e.name == n)) {
                    self.cursor = i;
                }
            }
            BrowseKey::Esc => return BrowseAction::Cancel,
            BrowseKey::Char(_) => {}
        }
        BrowseAction::Stay
    }

    fn descend(&mut self, e: &Entry) {
        self.cwd = self.path_of(e);
        self.relist();
    }

    /// Up one folder, the cursor on the folder it came from; nothing at the
    /// root.
    fn up(&mut self) {
        let Some(parent) = self.cwd.parent().map(Path::to_path_buf) else {
            return;
        };
        if parent.as_os_str().is_empty() {
            return;
        }
        let from = self.cwd.file_name().map(ToOwned::to_owned);
        self.cwd = parent;
        self.relist();
        if let Some(i) = from.and_then(|n| self.entries.iter().position(|e| e.name == n)) {
            self.cursor = i;
        }
    }
}

/// The picker's box over the full layout: x, y, w, h.
const FULL_BOX: (i32, i32, i32, i32) = (8, 2, 96, 34);
/// Rows of the box that are not the list: the top edge, the folder, a
/// separator, then a separator, the button, the keys and the bottom edge.
const CHROME_ROWS: i32 = 7;
/// The size column's width, the longest `fs::size_label` (`1023.9 GiB`).
const SIZE_W: i32 = 10;
/// The narrowest inside of the box that has the size column.
const SIZES_FROM: i32 = 40;

/// Draws the picker for `kind`: over the full layout (stepped back behind
/// it, with a shadow), across the widget tile, and not at all in the one-line
/// fallback.
pub fn draw(c: &mut Canvas, s: &BrowseState, kind: LayoutKind, theme: &Theme) {
    match kind {
        LayoutKind::Full => {
            step_back(c, 0.68, theme.roles.bg);
            c.clipped(FULL_SIZE.0, FULL_SIZE.1, |c| {
                draw_box(c, s, FULL_BOX, true, theme);
            });
        }
        LayoutKind::Widget => {
            let (w, h) = WIDGET_SIZE;
            c.clipped(w, h, |c| {
                draw_box(c, s, (0, 0, i32::from(w), i32::from(h)), false, theme);
            });
        }
        LayoutKind::OneLine => {}
    }
}

/// The box at `(x, y, w, h)`: the folder, the listing, the button and the
/// keys, the picked count on the top edge.
fn draw_box(
    c: &mut Canvas,
    s: &BrowseState,
    (x, y, w, h): (i32, i32, i32, i32),
    shadow: bool,
    theme: &Theme,
) {
    let r = &theme.roles;
    let note = format!(
        "{{D}}{}",
        strings::N_SELECTED.replace("{n}", &format!("{{W}}{}{{D}}", s.selected.len()))
    );
    let style = BoxStyle {
        shadow,
        ..modal_style(strings::BROWSE_TITLE, Some(&note), theme)
    };
    let b = c.boxed(x, y, w, h, &style, theme);
    let inner = w - 2;

    // The folder, cut from the left so its own name stays in view.
    let cwd = s.cwd.display().to_string();
    c.text(
        x + 2,
        y + 1,
        &fit_left(&cwd, room_of(inner - 2)),
        r.heading,
        Some(b.fill),
    );

    let sep = [r.bg, r.accent, r.needs_input];
    b.sep(c, y + 2, Some(&sep), theme);
    let rows = h - CHROME_ROWS;
    list(c, s, &b, rows, theme);
    b.sep(c, y + h - 4, Some(&sep), theme);

    let button = if s.selected.is_empty() {
        strings::FEED_BUTTON_EMPTY
    } else if s.cursor >= s.entries.len() {
        strings::FEED_BUTTON_ON
    } else {
        strings::FEED_BUTTON_OFF
    };
    c.rich(x + 2, y + h - 3, button, None, theme);
    let keys = if to_i32(plain_len(strings::BROWSE_KEYS)) + 2 <= inner {
        strings::BROWSE_KEYS
    } else {
        strings::BROWSE_KEYS_SHORT
    };
    c.rich(x + 2, y + h - 2, keys, None, theme);
}

/// The listing's `rows` rows, scrolled to keep the cursor in view: the cursor
/// row on the lightbar, a picked file ticked, a folder marked `▸`, file sizes
/// on the right where the box is wide enough.
fn list(c: &mut Canvas, s: &BrowseState, b: &Boxed, rows: i32, theme: &Theme) {
    let r = &theme.roles;
    let top = b.y + 3;
    if s.entries.is_empty() {
        let (text, fg) = if s.unreadable {
            (strings::CANT_READ_FOLDER, r.error)
        } else {
            (strings::NO_PDFS_HERE, r.dim)
        };
        c.text(b.x + 4, top, text, fg, None);
        return;
    }
    let rows = usize::try_from(rows.max(1)).unwrap_or(1);
    let focus = s.cursor.min(s.entries.len() - 1);
    let first = focus.saturating_sub(rows - 1);
    let sizes = b.w - 2 >= SIZES_FROM;
    let name_x = b.x + 8;
    let name_end = if sizes {
        b.x + b.w - 3 - SIZE_W
    } else {
        b.x + b.w - 2
    };
    let name_w = usize::try_from(name_end - name_x).unwrap_or(0);
    for (row, (i, e)) in s
        .entries
        .iter()
        .enumerate()
        .skip(first)
        .take(rows)
        .enumerate()
    {
        let ry = top + to_i32(row);
        let on = i == s.cursor;
        if on {
            c.fill(b.x + 1, ry, b.w - 2, 1, r.body, r.lightbar);
            c.put(b.x + 2, ry, '►', Some(r.hotkey), None);
        }
        if e.is_dir {
            c.put(b.x + 5, ry, '▸', Some(r.accent), None);
            c.text(name_x, ry, &fit(&e.display(), name_w), r.heading, None);
            continue;
        }
        let picked = s.selected.contains(&s.cwd.join(&e.name));
        c.text(b.x + 4, ry, "[", r.dim, None);
        if picked {
            c.put(b.x + 5, ry, '√', Some(r.ok), None);
        }
        c.text(b.x + 6, ry, "]", r.dim, None);
        let fg = if on { r.heading } else { r.file };
        c.text(name_x, ry, &fit(&e.display(), name_w), fg, None);
        if sizes {
            let w = room_of(SIZE_W);
            let size = format!("{:>w$}", fit(&fs::size_label(e.size), w));
            c.text(b.x + b.w - 2 - SIZE_W, ry, &size, r.dim, None);
        }
    }
}

fn room_of(n: i32) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// `s`, cut to `max` characters from the left with a leading `…` when longer.
fn fit_left(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::from('…');
    out.extend(s.chars().skip(n - (max - 1)));
    out
}

fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use std::fs as stdfs;

    use super::*;
    use crate::place::ScratchDir;
    use crate::ui::fs::tests::fixture;

    fn theme() -> &'static Theme {
        Theme::default_theme()
    }

    fn names(s: &BrowseState) -> Vec<String> {
        s.entries.iter().map(Entry::display).collect()
    }

    fn row_text(c: &Canvas, y: u16) -> String {
        (0..c.w).map(|x| c.get(x, y).expect("cell").ch).collect()
    }

    /// `a.pdf`, `b.PDF`, `c.txt`, `sub/` (holding `deep.pdf`), `.hidden.pdf`.
    fn tree(label: &str) -> ScratchDir {
        let dir = fixture(label);
        stdfs::write(dir.join("sub/deep.pdf"), b"%PDF-").unwrap();
        stdfs::write(dir.join(".hidden.pdf"), b"%PDF-").unwrap();
        dir
    }

    #[test]
    fn it_opens_on_the_filtered_sorted_listing() {
        let dir = tree("browse-open");
        let s = BrowseState::open(dir.path());
        assert_eq!(names(&s), ["sub", "a.pdf", "b.PDF"]);
        assert_eq!(s.entries.iter().filter(|e| e.is_dir).count(), 1);
        assert_eq!((s.cursor, s.filter, s.hidden), (0, Filter::PdfOnly, false));
        assert!(s.selected.is_empty() && !s.unreadable);
        assert_eq!(s.cwd, dir.path());
    }

    #[test]
    fn the_cursor_runs_over_the_entries_and_the_button() {
        use BrowseKey::{Down, Up};
        let dir = tree("browse-cursor");
        let mut s = BrowseState::open(dir.path());
        for (key, want) in [(Up, 0), (Down, 1), (Down, 2), (Down, 3), (Down, 3), (Up, 2)] {
            assert_eq!(s.key(key), BrowseAction::Stay);
            assert_eq!(s.cursor, want, "{key:?}");
        }
    }

    #[test]
    fn folders_open_and_backspace_goes_back_to_the_one_left() {
        let dir = tree("browse-nav");
        let mut s = BrowseState::open(dir.path());
        s.key(BrowseKey::Enter);
        assert_eq!(s.cwd, dir.join("sub"));
        assert_eq!((names(&s), s.cursor), (vec!["deep.pdf".to_string()], 0));
        s.key(BrowseKey::Backspace);
        assert_eq!(s.cwd, dir.path());
        assert_eq!(s.cursor, 0, "on sub");
        // → opens a folder and does nothing on a file; ← is Backspace.
        s.key(BrowseKey::Right);
        assert_eq!(s.cwd, dir.join("sub"));
        s.key(BrowseKey::Left);
        assert_eq!(s.cwd, dir.path());
        s.key(BrowseKey::Down);
        s.key(BrowseKey::Right);
        assert_eq!((s.cwd.as_path(), s.cursor), (dir.path(), 1));
        // Up from a folder lands on it in its parent's listing.
        let parent = dir.path().parent().expect("a parent");
        s.key(BrowseKey::Backspace);
        assert_eq!(s.cwd, parent);
        let at = &s.entries[s.cursor];
        assert_eq!(Some(at.name.as_os_str()), dir.path().file_name());
    }

    #[test]
    fn backspace_at_the_root_stays() {
        let dir = ScratchDir::new("browse-root");
        let root = dir.path().ancestors().last().expect("a root").to_path_buf();
        let mut s = BrowseState::open(&root);
        let before = s.clone();
        assert_eq!(s.key(BrowseKey::Backspace), BrowseAction::Stay);
        assert_eq!(s, before);
    }

    #[test]
    fn picking_space_a_and_enter() {
        let dir = tree("browse-pick");
        let (a, b) = (dir.join("a.pdf"), dir.join("b.PDF"));
        let mut s = BrowseState::open(dir.path());
        // Space on a folder does nothing.
        s.key(BrowseKey::Char(' '));
        assert!(s.selected.is_empty());
        assert_eq!(s.cursor, 0);
        // Space picks the file and moves down; again on it unpicks.
        s.key(BrowseKey::Down);
        s.key(BrowseKey::Char(' '));
        assert_eq!(s.selected.iter().collect::<Vec<_>>(), [&a]);
        assert_eq!(s.cursor, 2);
        s.key(BrowseKey::Up);
        s.key(BrowseKey::Char(' '));
        assert!(s.selected.is_empty());
        // Enter on a file picks it and goes to the button.
        s.key(BrowseKey::Enter);
        assert_eq!(s.selected.iter().collect::<Vec<_>>(), [&b]);
        assert_eq!(s.cursor, 3);
        // `a` picks every pdf in view, never a folder or a hidden file.
        s.key(BrowseKey::Char('a'));
        assert_eq!(s.selected.iter().collect::<Vec<_>>(), [&a, &b]);
        // Picks survive a visit elsewhere and add up.
        s.key(BrowseKey::Up);
        s.key(BrowseKey::Up);
        s.key(BrowseKey::Up);
        s.key(BrowseKey::Enter);
        s.key(BrowseKey::Char('A'));
        assert_eq!(s.selected.len(), 3);
        s.key(BrowseKey::Backspace);
        assert_eq!(s.selected.len(), 3);
        // Other keys do nothing.
        let before = s.clone();
        for key in [
            BrowseKey::Char('q'),
            BrowseKey::Char('b'),
            BrowseKey::Char('x'),
        ] {
            assert_eq!(s.key(key), BrowseAction::Stay);
        }
        assert_eq!(s, before);
    }

    #[test]
    fn the_button_feeds_the_picked_files_in_order() {
        let dir = tree("browse-feed");
        let mut s = BrowseState::open(dir.path());
        for _ in 0..3 {
            s.key(BrowseKey::Down);
        }
        assert_eq!(s.cursor, 3);
        // Nothing picked: the button does nothing.
        assert_eq!(s.key(BrowseKey::Enter), BrowseAction::Stay);
        s.key(BrowseKey::Char('a'));
        assert_eq!(
            s.key(BrowseKey::Enter),
            BrowseAction::Feed(vec![dir.join("a.pdf"), dir.join("b.PDF")])
        );
        assert_eq!(s.key(BrowseKey::Esc), BrowseAction::Cancel);
    }

    #[test]
    fn dot_shows_hidden_entries_and_keeps_the_cursor() {
        let dir = tree("browse-hidden");
        let mut s = BrowseState::open(dir.path());
        s.key(BrowseKey::Down);
        s.key(BrowseKey::Down);
        assert_eq!(s.entries[s.cursor].display(), "b.PDF");
        s.key(BrowseKey::Char('.'));
        assert!(s.hidden);
        assert_eq!(names(&s), ["sub", ".hidden.pdf", "a.pdf", "b.PDF"]);
        assert_eq!(s.entries[s.cursor].display(), "b.PDF");
        s.key(BrowseKey::Char('.'));
        assert_eq!(names(&s), ["sub", "a.pdf", "b.PDF"]);
        assert_eq!(s.entries[s.cursor].display(), "b.PDF");
    }

    #[test]
    fn an_unreadable_folder_is_shown_empty_and_left_with_backspace() {
        let dir = tree("browse-gone");
        let mut s = BrowseState::open(&dir.join("gone"));
        assert!(s.unreadable && s.entries.is_empty());
        assert_eq!(s.cursor, 0);
        assert_eq!(s.key(BrowseKey::Enter), BrowseAction::Stay);
        assert_eq!(s.key(BrowseKey::Char(' ')), BrowseAction::Stay);
        let mut c = Canvas::new(112, 38, theme());
        draw(&mut c, &s, LayoutKind::Full, theme());
        assert!(row_text(&c, 5).contains(strings::CANT_READ_FOLDER));
        s.key(BrowseKey::Backspace);
        assert!(!s.unreadable);
        assert_eq!(s.cursor, 0, "the folder it left is not listed");
        assert_eq!(s.cwd, dir.path());
    }

    /// A hand-made picker: two folders and three files, one picked, the
    /// cursor on the second file.
    fn sample() -> BrowseState {
        let entry = |name: &str, is_dir: bool, size: u64| Entry {
            name: name.into(),
            is_dir,
            size,
        };
        let cwd = PathBuf::from("/evidence/case-2291");
        BrowseState {
            entries: vec![
                entry("2024-q3", true, 0),
                entry("scans", true, 0),
                entry("contract_signed.pdf", false, 245_000),
                entry("invoice_scan.pdf", false, 1_100_000),
                entry("{R}evil\u{1b}[2J.pdf", false, 12),
            ],
            cursor: 3,
            selected: [cwd.join("invoice_scan.pdf")].into_iter().collect(),
            cwd,
            ..BrowseState::default()
        }
    }

    #[test]
    fn the_full_picker_draws_its_rows() {
        let s = sample();
        let r = &theme().roles;
        let mut c = Canvas::new(112, 38, theme());
        draw(&mut c, &s, LayoutKind::Full, theme());
        let cell = |x: u16, y: u16| c.get(x, y).expect("cell");
        let top = row_text(&c, 2);
        assert!(
            top.contains(&format!("╡ {} ╞", strings::BROWSE_TITLE)),
            "{top:?}"
        );
        assert!(top.contains(" 1 selected "), "{top:?}");
        assert!(row_text(&c, 3).contains("/evidence/case-2291"));
        // Folders, marked; files with their box, the picked one ticked.
        assert!(
            row_text(&c, 5).contains("▸  2024-q3"),
            "{:?}",
            row_text(&c, 5)
        );
        assert!(row_text(&c, 7).contains("[ ] contract_signed.pdf"));
        assert!(row_text(&c, 8).contains("► [√] invoice_scan.pdf"));
        assert!(row_text(&c, 8).trim_end().ends_with("1.0 MiB ║"));
        assert!(row_text(&c, 7).contains("239 KiB"));
        // The cursor row is on the lightbar; the others are not.
        assert_eq!(cell(9, 8).bg, r.lightbar);
        assert_ne!(cell(9, 7).bg, r.lightbar);
        // A name from the disk is drawn as it is, never read as markup.
        assert!(row_text(&c, 9).contains("{R}evil\u{fffd}[2J.pdf"));
        // The button (not under the cursor) and the keys.
        assert!(row_text(&c, 33).contains("[ feed the cat ]"));
        assert!(row_text(&c, 34).contains("esc cancel"));
        assert_eq!(cell(103, 35).ch, '╝');
    }

    #[test]
    fn the_button_lights_under_the_cursor_and_dims_with_nothing_picked() {
        let mut s = sample();
        s.cursor = s.entries.len();
        let mut c = Canvas::new(112, 38, theme());
        draw(&mut c, &s, LayoutKind::Full, theme());
        assert!(row_text(&c, 33).contains(" ► feed the cat "));
        assert_eq!(c.get(11, 33).expect("cell").bg, theme().roles.accent);
        s.selected.clear();
        let mut c = Canvas::new(112, 38, theme());
        draw(&mut c, &s, LayoutKind::Full, theme());
        assert!(row_text(&c, 33).contains("[ feed the cat ]"));
        assert!(row_text(&c, 2).contains(" 0 selected "));
    }

    #[test]
    fn a_long_listing_scrolls_to_the_cursor() {
        let mut s = sample();
        s.entries = (0..100)
            .map(|i| Entry {
                name: format!("file_{i:03}.pdf").into(),
                is_dir: false,
                size: 1,
            })
            .collect();
        s.cursor = 60;
        let mut c = Canvas::new(112, 38, theme());
        draw(&mut c, &s, LayoutKind::Full, theme());
        // 27 rows from row 5: the cursor's is the last.
        assert!(row_text(&c, 5).contains("file_034.pdf"));
        assert!(row_text(&c, 31).contains("► [ ] file_060.pdf"));
        // On the button, the listing stays at its end.
        s.cursor = 100;
        let mut c = Canvas::new(112, 38, theme());
        draw(&mut c, &s, LayoutKind::Full, theme());
        assert!(row_text(&c, 31).contains("file_099.pdf"));
    }

    #[test]
    fn the_widget_picker_fills_the_tile_and_no_more() {
        let s = sample();
        let mut before = Canvas::new(60, 20, theme());
        before.put(40, 18, 'x', None, None);
        let mut c = before.clone();
        draw(&mut c, &s, LayoutKind::Widget, theme());
        for y in 0..20 {
            for x in 0..60 {
                if x >= 32 || y >= 16 {
                    assert_eq!(c.get(x, y), before.get(x, y), "({x}, {y})");
                }
            }
        }
        assert_eq!(c.get(0, 0).expect("cell").ch, '╔');
        assert_eq!(c.get(31, 15).expect("cell").ch, '╝');
        assert!(
            row_text(&c, 6).contains("► [√] invoice_sca"),
            "{:?}",
            row_text(&c, 6)
        );
        assert!(!row_text(&c, 6).contains("MiB"), "no size column");
        assert!(row_text(&c, 14).contains("␣ pick · a all · ⌫ up · esc"));
        // The one-line fallback draws nothing.
        let mut one = Canvas::new(20, 1, theme());
        let blank = one.clone();
        draw(&mut one, &s, LayoutKind::OneLine, theme());
        assert_eq!(one, blank);
    }

    /// Hostile folder and file names, and a folder path holding one, are
    /// drawn as they are in both pickers, controls and bidi controls as `�`.
    #[test]
    fn hostile_names_are_drawn_literally() {
        for (name, shown) in crate::ui::canvas::HOSTILE_NAMES {
            let mut s = sample();
            s.entries = vec![
                Entry {
                    name: format!("d{name}").into(),
                    is_dir: true,
                    size: 0,
                },
                Entry {
                    name: name.into(),
                    is_dir: false,
                    size: 12,
                },
            ];
            s.cursor = 0;
            s.cwd = PathBuf::from(format!("/case/{name}"));
            for (kind, size) in [
                (LayoutKind::Full, (112, 38)),
                (LayoutKind::Widget, (32, 16)),
            ] {
                let mut c = Canvas::new(size.0, size.1, theme());
                draw(&mut c, &s, kind, theme());
                c.assert_printable();
                let rows: Vec<String> = (0..c.h).map(|y| row_text(&c, y)).collect();
                let has = |want: &str| rows.iter().any(|r| r.contains(want));
                assert!(has(&format!("▸  d{shown}")), "{kind:?} {name:?}");
                assert!(has(&format!("[ ] {shown}")), "{kind:?} {name:?}");
                if kind == LayoutKind::Full {
                    assert!(has(&format!("/case/{shown}")), "{name:?}");
                }
            }
        }
    }

    #[test]
    fn long_folders_keep_their_end() {
        assert_eq!(fit_left("/a/b/c", 10), "/a/b/c");
        assert_eq!(fit_left("/evidence/case-2291", 8), "…se-2291");
        assert_eq!(fit_left("abc", 0), "");
        let mut s = sample();
        s.cwd = PathBuf::from(format!("/{}/last-folder", "x".repeat(200)));
        let mut c = Canvas::new(112, 38, theme());
        draw(&mut c, &s, LayoutKind::Full, theme());
        let line = row_text(&c, 3);
        assert!(line.contains("║ …xxx"), "{line:?}");
        assert!(line.contains("xxx/last-folder ║"), "{line:?}");
    }

    /// Every phrase the picker draws is in the artefact deny-list.
    #[test]
    fn the_pickers_phrases_are_in_the_deny_list() {
        let plain = |markup: &str| {
            let mut c = Canvas::new(120, 1, theme());
            let end = c.rich(0, 0, markup, None, theme());
            row_text(&c, 0)
                .chars()
                .take(usize::try_from(end).unwrap_or(0))
                .collect::<String>()
        };
        for m in [
            strings::FEED_BUTTON_ON,
            strings::FEED_BUTTON_OFF,
            strings::FEED_BUTTON_EMPTY,
            strings::BROWSE_KEYS,
            strings::BROWSE_KEYS_SHORT,
        ] {
            let p = plain(m);
            assert!(
                strings::ALL
                    .iter()
                    .any(|a| p.trim() == a.trim() || p.contains(a)),
                "{p:?} is not in strings::ALL"
            );
        }
        for s in [
            strings::BROWSE_TITLE,
            strings::N_SELECTED,
            strings::NO_PDFS_HERE,
            strings::CANT_READ_FOLDER,
            strings::BROWSING,
        ] {
            assert!(strings::ALL.contains(&s), "{s:?}");
        }
    }
}
