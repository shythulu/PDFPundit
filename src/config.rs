//! `Config` (TD §6): `config.toml` in the config dir. A missing file gives the
//! defaults; an unknown key, a value of the wrong type or a file that is not
//! TOML gives a startup [`Warning`] and the default for what it touched, never
//! an error.
//!
//! The sections are TD:589-617 with three amendments: no `[general] offline`
//! (D-050: nothing in the binary touches the network, so the key is reported as
//! unknown), `[general] output_dir` as TD:592 states it (D-061), and the
//! `[fonts] unreproducible` and `[repair] salvage_*` / `max_search_stream`
//! knobs (SE Q3, D-041, D-056, D-072). `threads` and the scratch cap are not
//! knobs because they never change output.
// The shell (T-23b) loads the config; until it lands only the tests use this.
// TODO(T-23b): remove this allow once the shell calls `Config::load`.
#![allow(dead_code)]

use std::fmt;
use std::path::{Path, PathBuf};

use crate::appdirs::AppDirs;
use crate::engine::{
    AnalyzeOptions, FontSourcePolicy, PageSize, Ratio, RepairOptions, SalvageBudget,
    UnreproduciblePolicy,
};

/// The file name inside [`AppDirs::config_dir`].
pub const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub general: General,
    pub repair: Repair,
    pub fonts: Fonts,
    pub custody: Custody,
    pub ui: Ui,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct General {
    /// `output_dir = ""` is [`OutputDir::Beside`].
    pub output_dir: OutputDir,
    pub default_page_size: PageSize,
}

/// Where outputs go (D-061).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputDir {
    /// Beside the input (`output_dir = ""`, the default).
    Beside,
    /// This directory, always absolute: a relative value is resolved against
    /// the home directory, with a warning.
    Dir(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repair {
    pub auto_accept_confidence: Ratio,
    pub max_font_candidates: u32,
    pub extract_unplaceable_images: bool,
    /// W per damaged stream (D-041).
    pub salvage_work: u64,
    /// W per stream when `salvage_work` found no accept (D-041).
    pub salvage_deep_work: u64,
    /// The per-file pool `salvage_deep_work` is drawn from (D-056).
    pub salvage_deep_pool: u64,
    /// Raw stream bytes above which no search runs (D-072).
    pub max_search_stream: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fonts {
    /// `source = "bundled" | "system"`.
    pub source: FontSourcePolicy,
    /// `false`: pick the best candidate without asking.
    pub prompt_unresolved: bool,
    /// `"ask" | "substitute_generic" | "text_only"` (SE Q3).
    pub unreproducible: UnreproduciblePolicy,
}

/// Chain-of-custody mode (M6b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Custody {
    pub enabled: bool,
    pub hashes: Vec<CustodyHash>,
    pub ask_case_details: bool,
    /// `None` (`log_path = ""`): `custody.log` in the data dir.
    pub log_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustodyHash {
    Sha256,
    Sha1,
    Md5,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    pub theme: String,
    pub mouse: bool,
    pub layout: Layout,
    /// Widget layout: ask the terminal to grow when a decision is needed.
    pub request_resize: bool,
}

/// `auto` picks the widget below 112×38 (UI plan D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Auto,
    Full,
    Widget,
}

impl Default for General {
    fn default() -> Self {
        General {
            output_dir: OutputDir::Beside,
            default_page_size: PageSize::A4,
        }
    }
}

impl Default for Repair {
    fn default() -> Self {
        Repair {
            auto_accept_confidence: Ratio { num: 35, den: 100 },
            max_font_candidates: 5,
            extract_unplaceable_images: true,
            salvage_work: 700_000_000,
            salvage_deep_work: 10_000_000_000,
            salvage_deep_pool: 80_000_000_000,
            max_search_stream: 4_194_304,
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts {
            source: FontSourcePolicy::Bundled,
            prompt_unresolved: true,
            unreproducible: UnreproduciblePolicy::Ask,
        }
    }
}

impl Default for Custody {
    fn default() -> Self {
        Custody {
            enabled: false,
            hashes: vec![CustodyHash::Sha256, CustodyHash::Sha1, CustodyHash::Md5],
            ask_case_details: true,
            log_path: None,
        }
    }
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            theme: "DarkBerry Blackwater".into(),
            mouse: true,
            layout: Layout::Auto,
            request_resize: true,
        }
    }
}

/// A startup warning about the config file. The defaults stand in for whatever
/// it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// The file exists but could not be read, or is not TOML: every setting is
    /// the default.
    Unreadable { reason: String },
    /// `section` or `section.key`, which no setting has.
    UnknownKey { key: String },
    /// The value at `section.key` is not `expected`; the default stands.
    BadValue { key: String, expected: &'static str },
    /// A relative `output_dir`, resolved against the home directory.
    RelativeOutputDir { given: PathBuf, resolved: PathBuf },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::Unreadable { reason } => {
                write!(
                    f,
                    "config: could not read it ({reason}); using the defaults"
                )
            }
            Warning::UnknownKey { key } => write!(f, "config: unknown key `{key}`, ignored"),
            Warning::BadValue { key, expected } => {
                write!(f, "config: `{key}` should be {expected}; using the default")
            }
            Warning::RelativeOutputDir { given, resolved } => write!(
                f,
                "config: `general.output_dir` {} is relative; using {}",
                given.display(),
                resolved.display()
            ),
        }
    }
}

impl Config {
    /// Loads `config.toml` from `dirs.config_dir`. Returns the path it read
    /// (or would have read), for the run record (eng-r2-q10).
    pub fn load(dirs: &AppDirs) -> (Config, Vec<Warning>, PathBuf) {
        let path = dirs.config_dir.join(CONFIG_FILE);
        let home = std::env::home_dir();
        let (config, warnings) = load_file(&path, home.as_deref());
        (config, warnings, path)
    }

    /// The options a repair runs with under this config. The salvage budget is
    /// the four `[repair]` work knobs; `threads` and the scratch cap keep their
    /// defaults because they never change output (D-066, D-072).
    pub fn repair_options(&self) -> RepairOptions {
        let r = &self.repair;
        RepairOptions {
            passes: None,
            font_policy: self.fonts.source,
            unreproducible: self.fonts.unreproducible,
            default_page_size: self.general.default_page_size,
            extract_unplaceable_images: r.extract_unplaceable_images,
            auto_accept_confidence: r.auto_accept_confidence,
            max_font_candidates: r.max_font_candidates,
            analyze: AnalyzeOptions {
                salvage_budget: SalvageBudget {
                    work: r.salvage_work,
                    deep_work: r.salvage_deep_work,
                    deep_pool: r.salvage_deep_pool,
                    max_search_stream: r.max_search_stream,
                },
                ..AnalyzeOptions::default()
            },
        }
    }

    /// The config as a complete `config.toml`; [`parse`] reads it back equal.
    pub fn to_toml(&self) -> String {
        use toml::{Table, Value};
        let s = |v: &str| Value::String(v.to_string());
        // TOML integers are i64, so [`parse`] never yields a larger value; one
        // set in code above `i64::MAX` is written as `i64::MAX`.
        let int = |n: u64| Value::Integer(i64::try_from(n).unwrap_or(i64::MAX));
        let path = |p: &Path| s(&p.to_string_lossy());

        let mut general = Table::new();
        let out = match &self.general.output_dir {
            OutputDir::Beside => s(""),
            OutputDir::Dir(p) => path(p),
        };
        general.insert("output_dir".into(), out);
        general.insert(
            "default_page_size".into(),
            s(&page_size_name(self.general.default_page_size)),
        );

        let r = &self.repair;
        let mut repair = Table::new();
        let c = r.auto_accept_confidence;
        repair.insert("auto_accept_confidence".into(), Value::Float(ratio_f64(c)));
        repair.insert(
            "max_font_candidates".into(),
            int(u64::from(r.max_font_candidates)),
        );
        repair.insert(
            "extract_unplaceable_images".into(),
            Value::Boolean(r.extract_unplaceable_images),
        );
        repair.insert("salvage_work".into(), int(r.salvage_work));
        repair.insert("salvage_deep_work".into(), int(r.salvage_deep_work));
        repair.insert("salvage_deep_pool".into(), int(r.salvage_deep_pool));
        repair.insert("max_search_stream".into(), int(r.max_search_stream));

        let mut fonts = Table::new();
        let source = match self.fonts.source {
            FontSourcePolicy::Bundled => "bundled",
            FontSourcePolicy::BundledThenSystem => "system",
        };
        fonts.insert("source".into(), s(source));
        fonts.insert(
            "prompt_unresolved".into(),
            Value::Boolean(self.fonts.prompt_unresolved),
        );
        let unreproducible = match self.fonts.unreproducible {
            UnreproduciblePolicy::Ask => "ask",
            UnreproduciblePolicy::SubstituteGeneric => "substitute_generic",
            UnreproduciblePolicy::TextOnly => "text_only",
        };
        fonts.insert("unreproducible".into(), s(unreproducible));

        let mut custody = Table::new();
        custody.insert("enabled".into(), Value::Boolean(self.custody.enabled));
        let hashes = self.custody.hashes.iter().map(|h| {
            s(match h {
                CustodyHash::Sha256 => "sha256",
                CustodyHash::Sha1 => "sha1",
                CustodyHash::Md5 => "md5",
            })
        });
        custody.insert("hashes".into(), Value::Array(hashes.collect()));
        custody.insert(
            "ask_case_details".into(),
            Value::Boolean(self.custody.ask_case_details),
        );
        let log = self.custody.log_path.as_deref().map_or_else(|| s(""), path);
        custody.insert("log_path".into(), log);

        let mut ui = Table::new();
        ui.insert("theme".into(), s(&self.ui.theme));
        ui.insert("mouse".into(), Value::Boolean(self.ui.mouse));
        let layout = match self.ui.layout {
            Layout::Auto => "auto",
            Layout::Full => "full",
            Layout::Widget => "widget",
        };
        ui.insert("layout".into(), s(layout));
        ui.insert(
            "request_resize".into(),
            Value::Boolean(self.ui.request_resize),
        );

        let mut root = Table::new();
        root.insert("general".into(), Value::Table(general));
        root.insert("repair".into(), Value::Table(repair));
        root.insert("fonts".into(), Value::Table(fonts));
        root.insert("custody".into(), Value::Table(custody));
        root.insert("ui".into(), Value::Table(ui));
        toml::to_string(&root).expect("a table of plain values serialises")
    }
}

/// Reads and parses one config file; a missing file is the defaults, silently.
fn load_file(path: &Path, home: Option<&Path>) -> (Config, Vec<Warning>) {
    let unreadable = |reason: String| (Config::default(), vec![Warning::Unreadable { reason }]);
    match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => parse(&text, home),
            Err(_) => unreadable("not UTF-8".into()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), Vec::new()),
        Err(e) => unreadable(e.to_string()),
    }
}

/// Parses `config.toml` text. `home` resolves a relative `output_dir`.
pub(crate) fn parse(text: &str, home: Option<&Path>) -> (Config, Vec<Warning>) {
    let mut config = Config::default();
    let mut w = Vec::new();
    let root: toml::Table = match text.parse() {
        Ok(root) => root,
        Err(e) => {
            let reason = e
                .to_string()
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            return (config, vec![Warning::Unreadable { reason }]);
        }
    };
    for (name, value) in &root {
        let known = ["general", "repair", "fonts", "custody", "ui"];
        let Some(table) = value.as_table().filter(|_| known.contains(&name.as_str())) else {
            if known.contains(&name.as_str()) {
                bad(&mut w, name.clone(), "a table");
            } else {
                w.push(Warning::UnknownKey { key: name.clone() });
            }
            continue;
        };
        for (key, v) in table {
            let full = format!("{name}.{key}");
            let mut f = Field {
                key: full,
                value: v,
                w: &mut w,
            };
            let known = match (name.as_str(), key.as_str()) {
                ("general", "output_dir") => {
                    if let Some(dir) = f.string() {
                        config.general.output_dir = output_dir(&dir, home, f.w);
                    }
                    true
                }
                ("general", "default_page_size") => {
                    f.set(&mut config.general.default_page_size, page_size, PAGE_SIZE)
                }
                ("repair", "auto_accept_confidence") => f.set(
                    &mut config.repair.auto_accept_confidence,
                    confidence,
                    "a number from 0 to 1",
                ),
                ("repair", "max_font_candidates") => f.set(
                    &mut config.repair.max_font_candidates,
                    |v| u32::try_from(v.as_integer()?).ok(),
                    "a whole number",
                ),
                ("repair", "extract_unplaceable_images") => {
                    f.flag(&mut config.repair.extract_unplaceable_images)
                }
                ("repair", "salvage_work") => f.count(&mut config.repair.salvage_work),
                ("repair", "salvage_deep_work") => f.count(&mut config.repair.salvage_deep_work),
                ("repair", "salvage_deep_pool") => f.count(&mut config.repair.salvage_deep_pool),
                ("repair", "max_search_stream") => f.count(&mut config.repair.max_search_stream),
                ("fonts", "source") => f.set(
                    &mut config.fonts.source,
                    |v| match v.as_str()? {
                        "bundled" => Some(FontSourcePolicy::Bundled),
                        "system" => Some(FontSourcePolicy::BundledThenSystem),
                        _ => None,
                    },
                    "\"bundled\" or \"system\"",
                ),
                ("fonts", "prompt_unresolved") => f.flag(&mut config.fonts.prompt_unresolved),
                ("fonts", "unreproducible") => f.set(
                    &mut config.fonts.unreproducible,
                    |v| match v.as_str()? {
                        "ask" => Some(UnreproduciblePolicy::Ask),
                        "substitute_generic" => Some(UnreproduciblePolicy::SubstituteGeneric),
                        "text_only" => Some(UnreproduciblePolicy::TextOnly),
                        _ => None,
                    },
                    "\"ask\", \"substitute_generic\" or \"text_only\"",
                ),
                ("custody", "enabled") => f.flag(&mut config.custody.enabled),
                ("custody", "hashes") => f.set(
                    &mut config.custody.hashes,
                    custody_hashes,
                    "a list of \"sha256\", \"sha1\", \"md5\"",
                ),
                ("custody", "ask_case_details") => f.flag(&mut config.custody.ask_case_details),
                ("custody", "log_path") => f.set(
                    &mut config.custody.log_path,
                    |v| {
                        v.as_str()
                            .map(|s| (!s.is_empty()).then(|| PathBuf::from(s)))
                    },
                    "a path or \"\"",
                ),
                ("ui", "theme") => f.set(
                    &mut config.ui.theme,
                    |v| v.as_str().map(str::to_string),
                    "a string",
                ),
                ("ui", "mouse") => f.flag(&mut config.ui.mouse),
                ("ui", "layout") => f.set(
                    &mut config.ui.layout,
                    |v| match v.as_str()? {
                        "auto" => Some(Layout::Auto),
                        "full" => Some(Layout::Full),
                        "widget" => Some(Layout::Widget),
                        _ => None,
                    },
                    "\"auto\", \"full\" or \"widget\"",
                ),
                ("ui", "request_resize") => f.flag(&mut config.ui.request_resize),
                _ => false,
            };
            if !known {
                f.w.push(Warning::UnknownKey { key: f.key });
            }
        }
    }
    (config, w)
}

/// One `section.key = value` being read.
struct Field<'a> {
    key: String,
    value: &'a toml::Value,
    w: &'a mut Vec<Warning>,
}

impl Field<'_> {
    /// Stores `parse(value)` in `slot`, or warns and leaves the default.
    /// Returns `true`: the key is known.
    fn set<T>(
        &mut self,
        slot: &mut T,
        parse: impl Fn(&toml::Value) -> Option<T>,
        expected: &'static str,
    ) -> bool {
        match parse(self.value) {
            Some(v) => *slot = v,
            None => bad(self.w, self.key.clone(), expected),
        }
        true
    }

    fn flag(&mut self, slot: &mut bool) -> bool {
        self.set(slot, toml::Value::as_bool, "true or false")
    }

    fn count(&mut self, slot: &mut u64) -> bool {
        self.set(
            slot,
            |v| u64::try_from(v.as_integer()?).ok(),
            "a whole number",
        )
    }

    fn string(&mut self) -> Option<String> {
        let s = self.value.as_str().map(str::to_string);
        if s.is_none() {
            bad(self.w, self.key.clone(), "a string");
        }
        s
    }
}

fn bad(w: &mut Vec<Warning>, key: String, expected: &'static str) {
    w.push(Warning::BadValue { key, expected });
}

/// `""` is beside the input; a relative path is resolved against `home`.
fn output_dir(dir: &str, home: Option<&Path>, w: &mut Vec<Warning>) -> OutputDir {
    if dir.is_empty() {
        return OutputDir::Beside;
    }
    let given = PathBuf::from(dir);
    if given.is_absolute() {
        return OutputDir::Dir(given);
    }
    match home {
        Some(home) => {
            let resolved = home.join(&given);
            w.push(Warning::RelativeOutputDir {
                given,
                resolved: resolved.clone(),
            });
            OutputDir::Dir(resolved)
        }
        None => {
            bad(w, "general.output_dir".into(), "an absolute path or \"\"");
            OutputDir::Beside
        }
    }
}

const PAGE_SIZE: &str = "\"A4\", \"Letter\" or \"<width>x<height>\" in points";

fn page_size(v: &toml::Value) -> Option<PageSize> {
    let s = v.as_str()?;
    if s.eq_ignore_ascii_case("a4") {
        return Some(PageSize::A4);
    }
    if s.eq_ignore_ascii_case("letter") {
        return Some(PageSize::Letter);
    }
    let (w, h) = s.split_once('x')?;
    let (w_pt, h_pt) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w_pt > 0 && h_pt > 0).then_some(PageSize::Custom { w_pt, h_pt })
}

fn page_size_name(size: PageSize) -> String {
    match size {
        PageSize::A4 => "A4".into(),
        PageSize::Letter => "Letter".into(),
        PageSize::Custom { w_pt, h_pt } => format!("{w_pt}x{h_pt}"),
    }
}

/// The ratio as the `f64` nearest to it. A decimal ratio (`den` a power of
/// ten, which is all [`confidence`] makes) is written out as its exact decimal
/// and parsed, so it round-trips through [`confidence`] whenever it has at most
/// 15 significant digits. Any other ratio (`1/3`) is only approximated and does
/// not round-trip.
fn ratio_f64(r: Ratio) -> f64 {
    let digits = (0..=18u32).find(|&k| 10u64.checked_pow(k) == Some(r.den));
    match digits {
        Some(k) => {
            let den = 10u64.pow(k);
            let text = format!(
                "{}.{:0width$}",
                r.num / den,
                r.num % den,
                width = k.max(1) as usize
            );
            text.parse().unwrap_or(0.0)
        }
        None if r.den == 0 => 0.0,
        None => r.num as f64 / r.den as f64,
    }
}

/// A number in `[0, 1]` as the exact decimal it was written as: TOML gives an
/// `f64`, whose shortest round-trip form (`0.35`) is read digit by digit into
/// `35/100`, so no float reaches the settings snapshot.
fn confidence(v: &toml::Value) -> Option<Ratio> {
    if let Some(n) = v.as_integer() {
        return (0..=1).contains(&n).then_some(Ratio {
            num: n as u64,
            den: 1,
        });
    }
    let f = v.as_float()?;
    if !(0.0..=1.0).contains(&f) {
        return None;
    }
    // `-0.0` formats as "-0", which the digit reader below refuses.
    if f == 0.0 {
        return Some(Ratio { num: 0, den: 1 });
    }
    let text = format!("{f}");
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    if frac.len() > 18 {
        return None;
    }
    let den = 10u64.pow(frac.len() as u32);
    let frac = if frac.is_empty() {
        0
    } else {
        frac.parse::<u64>().ok()?
    };
    let num = int
        .parse::<u64>()
        .ok()?
        .checked_mul(den)?
        .checked_add(frac)?;
    Some(Ratio { num, den })
}

fn custody_hashes(v: &toml::Value) -> Option<Vec<CustodyHash>> {
    v.as_array()?
        .iter()
        .map(|h| match h.as_str()? {
            "sha256" => Some(CustodyHash::Sha256),
            "sha1" => Some(CustodyHash::Sha1),
            "md5" => Some(CustodyHash::Md5),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TD:589-617 as written, minus `offline` (D-050), plus the amended knobs.
    const DEFAULT_TOML: &str = r#"
[general]
output_dir = ""
default_page_size = "A4"

[repair]
auto_accept_confidence = 0.35
max_font_candidates = 5
extract_unplaceable_images = true
salvage_work = 700000000
salvage_deep_work = 10000000000
salvage_deep_pool = 80000000000
max_search_stream = 4194304

[fonts]
source = "bundled"
prompt_unresolved = true
unreproducible = "ask"

[custody]
enabled = false
hashes = ["sha256", "sha1", "md5"]
ask_case_details = true
log_path = ""

[ui]
theme = "DarkBerry Blackwater"
mouse = true
layout = "auto"
request_resize = true
"#;

    fn home() -> Option<&'static Path> {
        Some(Path::new("/home/ana"))
    }

    fn unknown(key: &str) -> Warning {
        Warning::UnknownKey { key: key.into() }
    }

    #[test]
    fn the_default_config_parses_to_the_defaults() {
        let (config, warnings) = parse(DEFAULT_TOML, home());
        assert_eq!(warnings, []);
        assert_eq!(config, Config::default());
        assert_eq!(parse("", home()), (Config::default(), vec![]));
    }

    #[test]
    fn defaults_match_the_engine_defaults() {
        // D-041, D-056, D-072: the knobs and the engine's own defaults agree.
        assert_eq!(Config::default().repair_options(), RepairOptions::default());
    }

    #[test]
    fn knobs_reach_the_repair_options() {
        let text = "[repair]\nsalvage_work = 1\nsalvage_deep_work = 2\n\
                    salvage_deep_pool = 3\nmax_search_stream = 4\n\
                    auto_accept_confidence = 0.5\n\
                    [fonts]\nsource = \"system\"\nunreproducible = \"text_only\"\n\
                    [general]\ndefault_page_size = \"Letter\"\n";
        let (config, warnings) = parse(text, home());
        assert_eq!(warnings, []);
        let opts = config.repair_options();
        let budget = opts.analyze.salvage_budget;
        assert_eq!(
            (
                budget.work,
                budget.deep_work,
                budget.deep_pool,
                budget.max_search_stream
            ),
            (1, 2, 3, 4)
        );
        assert_eq!(opts.auto_accept_confidence, Ratio { num: 1, den: 2 });
        assert_eq!(opts.font_policy, FontSourcePolicy::BundledThenSystem);
        assert_eq!(opts.unreproducible, UnreproduciblePolicy::TextOnly);
        assert_eq!(opts.default_page_size, PageSize::Letter);
        assert_eq!(opts.analyze.threads, AnalyzeOptions::default().threads);
    }

    #[test]
    fn unknown_keys_warn_and_change_nothing() {
        let text = "[general]\noffline = true\n\
                    [repair]\nmax_stream = 16777216\nsalvage_budget_ms = 2000\n\
                    [catapi]\nkey = \"x\"\n";
        let (config, warnings) = parse(text, home());
        assert_eq!(config, Config::default());
        assert_eq!(
            warnings,
            [
                unknown("catapi"),
                unknown("general.offline"),
                unknown("repair.max_stream"),
                unknown("repair.salvage_budget_ms"),
            ]
        );
        assert_eq!(
            warnings[1].to_string(),
            "config: unknown key `general.offline`, ignored"
        );
    }

    #[test]
    fn bad_values_warn_and_keep_the_default() {
        let text = "general = 3\n[repair]\nsalvage_work = -1\nmouse = 1\n\
                    auto_accept_confidence = 1.5\n\
                    [fonts]\nunreproducible = \"guess\"\n\
                    [ui]\nmouse = \"yes\"\n";
        let (config, warnings) = parse(text, home());
        assert_eq!(config, Config::default());
        let keys: Vec<_> = warnings
            .iter()
            .map(|w| match w {
                Warning::BadValue { key, .. } => format!("bad {key}"),
                Warning::UnknownKey { key } => format!("unknown {key}"),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            keys,
            [
                "bad fonts.unreproducible",
                "bad general",
                "bad repair.auto_accept_confidence",
                "unknown repair.mouse",
                "bad repair.salvage_work",
                "bad ui.mouse",
            ]
        );
    }

    #[test]
    fn a_file_that_is_not_toml_gives_the_defaults_and_one_warning() {
        let (config, warnings) = parse("[general\noutput_dir = ", home());
        assert_eq!(config, Config::default());
        assert!(matches!(warnings[..], [Warning::Unreadable { .. }]));
    }

    #[test]
    fn output_dir_empty_means_beside() {
        assert_eq!(Config::default().general.output_dir, OutputDir::Beside);
        let (config, _) = parse("[general]\noutput_dir = \"\"\n", home());
        assert_eq!(config.general.output_dir, OutputDir::Beside);
    }

    #[test]
    fn output_dir_round_trips() {
        let dir = std::env::temp_dir().join("pdfpundit out");
        let mut config = Config::default();
        config.general.output_dir = OutputDir::Dir(dir.clone());
        let (back, warnings) = parse(&config.to_toml(), home());
        assert_eq!(warnings, []);
        assert_eq!(back.general.output_dir, OutputDir::Dir(dir));
        assert_eq!(back, config);
    }

    #[test]
    fn a_relative_output_dir_is_resolved_against_home_with_a_warning() {
        let (config, warnings) = parse("[general]\noutput_dir = \"cases/out\"\n", home());
        let resolved = Path::new("/home/ana").join("cases/out");
        assert_eq!(config.general.output_dir, OutputDir::Dir(resolved.clone()));
        assert_eq!(
            warnings,
            [Warning::RelativeOutputDir {
                given: PathBuf::from("cases/out"),
                resolved
            }]
        );
        let (config, warnings) = parse("[general]\noutput_dir = \"cases/out\"\n", None);
        assert_eq!(config.general.output_dir, OutputDir::Beside);
        assert!(matches!(warnings[..], [Warning::BadValue { .. }]));
    }

    #[test]
    fn every_setting_round_trips() {
        let config = Config {
            general: General {
                output_dir: OutputDir::Beside,
                default_page_size: PageSize::Custom {
                    w_pt: 420,
                    h_pt: 595,
                },
            },
            repair: Repair {
                auto_accept_confidence: Ratio { num: 7, den: 10 },
                max_font_candidates: 9,
                extract_unplaceable_images: false,
                salvage_work: 1,
                salvage_deep_work: 2,
                salvage_deep_pool: 3,
                max_search_stream: 4,
            },
            fonts: Fonts {
                source: FontSourcePolicy::BundledThenSystem,
                prompt_unresolved: false,
                unreproducible: UnreproduciblePolicy::SubstituteGeneric,
            },
            custody: Custody {
                enabled: true,
                hashes: vec![CustodyHash::Md5],
                ask_case_details: false,
                log_path: Some(PathBuf::from("/var/log/custody.log")),
            },
            ui: Ui {
                theme: "Mono Ink".into(),
                mouse: false,
                layout: Layout::Widget,
                request_resize: false,
            },
        };
        assert_eq!(parse(&config.to_toml(), home()), (config, vec![]));
        let default = Config::default();
        assert_eq!(parse(&default.to_toml(), home()), (default, vec![]));
    }

    #[test]
    fn confidence_is_read_as_the_decimal_written() {
        let value = |s: &str| toml::Value::from(s.parse::<f64>().unwrap());
        assert_eq!(
            confidence(&value("0.35")),
            Some(Ratio { num: 35, den: 100 })
        );
        assert_eq!(confidence(&value("1.0")), Some(Ratio { num: 1, den: 1 }));
        assert_eq!(
            confidence(&toml::Value::Integer(0)),
            Some(Ratio { num: 0, den: 1 })
        );
        assert_eq!(confidence(&value("-0.1")), None);
        assert_eq!(confidence(&value("-0.0")), Some(Ratio { num: 0, den: 1 }));
        assert_eq!(confidence(&value("0.0")), Some(Ratio { num: 0, den: 1 }));
        for r in [
            Ratio { num: 35, den: 100 },
            Ratio { num: 0, den: 1 },
            Ratio { num: 1, den: 1 },
            Ratio { num: 7, den: 1000 },
            Ratio {
                num: 123_456_789_012_345,
                den: 1_000_000_000_000_000,
            },
        ] {
            // `Ratio` compares by value.
            assert_eq!(confidence(&toml::Value::Float(ratio_f64(r))), Some(r));
        }
        assert_eq!(confidence(&toml::Value::Integer(2)), None);
    }

    #[test]
    fn load_reads_the_config_dir_and_reports_the_path() {
        let root = std::env::temp_dir().join(format!("pdfpundit-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dirs = AppDirs {
            config_dir: root.clone(),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            documents_dir: None,
        };
        // Missing: the defaults, silently.
        let (config, warnings, path) = Config::load(&dirs);
        assert_eq!((config, warnings), (Config::default(), vec![]));
        assert_eq!(path, root.join("config.toml"));

        std::fs::write(
            &path,
            "[ui]\nlayout = \"full\"\n[general]\noffline = false\n",
        )
        .unwrap();
        let (config, warnings, _) = Config::load(&dirs);
        assert_eq!(config.ui.layout, Layout::Full);
        assert_eq!(warnings, [unknown("general.offline")]);

        std::fs::write(&path, b"\xff\xfe").unwrap();
        let (config, warnings, _) = Config::load(&dirs);
        assert_eq!(config, Config::default());
        assert!(matches!(warnings[..], [Warning::Unreadable { .. }]));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
