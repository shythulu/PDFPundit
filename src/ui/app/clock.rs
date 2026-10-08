//! The status bar's local time of day. std has UTC only, so on Unix the
//! offset comes from the system zone file, `/etc/localtime` (TZif, RFC 8536,
//! including the POSIX-TZ footer that governs after the last transition in a
//! "slim" file); `TZ` is not read (plan §3.1 lists the variables the app
//! reads). Windows asks `GetLocalTime`. Display only: nothing here reaches an
//! artefact.
// The zone-file reader serves Unix only; on Windows only its tests use it.
#![cfg_attr(windows, allow(dead_code))]

/// `(hour, minute)` at `unix` seconds, local time.
#[cfg(unix)]
pub fn local_hm(unix: u64) -> (u8, u8) {
    let t = i64::try_from(unix).unwrap_or(i64::MAX);
    let offset = std::fs::read("/etc/localtime")
        .ok()
        .and_then(|zone| tzif_offset(&zone, t))
        .unwrap_or(0);
    hm(t.saturating_add(i64::from(offset)))
}

/// `(hour, minute)` now, local time; Windows keeps the zone itself.
#[cfg(windows)]
pub fn local_hm(_unix: u64) -> (u8, u8) {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut st = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    // SAFETY: `st` is a valid, writable SYSTEMTIME for the whole call.
    unsafe { GetLocalTime(&mut st) };
    (
        u8::try_from(st.wHour).unwrap_or(0),
        u8::try_from(st.wMinute).unwrap_or(0),
    )
}

/// The time of day of `local` seconds.
fn hm(local: i64) -> (u8, u8) {
    let s = local.rem_euclid(86_400);
    // Below 24 and 60, so the casts are exact.
    ((s / 3600) as u8, (s / 60 % 60) as u8)
}

/// TZif's six counts, in file order.
struct Counts {
    isut: usize,
    isstd: usize,
    leap: usize,
    time: usize,
    types: usize,
    chars: usize,
}

const HEADER: usize = 44;

impl Counts {
    /// The header at the start of `b`, and its version byte.
    fn parse(b: &[u8]) -> Option<(Counts, u8)> {
        if b.get(..4)? != b"TZif" {
            return None;
        }
        let n = |i: usize| -> Option<usize> {
            let at = 20 + 4 * i;
            let v = u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?);
            usize::try_from(v).ok()
        };
        let c = Counts {
            isut: n(0)?,
            isstd: n(1)?,
            leap: n(2)?,
            time: n(3)?,
            types: n(4)?,
            chars: n(5)?,
        };
        Some((c, *b.get(4)?))
    }

    /// The data block's length with `t`-byte times.
    fn data_len(&self, t: usize) -> usize {
        self.time * (t + 1)
            + self.types * 6
            + self.chars
            + self.leap * (t + 4)
            + self.isstd
            + self.isut
    }
}

/// Seconds east of UTC at `t` in the TZif zone `zone`.
fn tzif_offset(zone: &[u8], t: i64) -> Option<i32> {
    let (v1, version) = Counts::parse(zone)?;
    let (c, body, wide) = if version >= b'2' {
        let rest = zone.get(HEADER + v1.data_len(4)..)?;
        (Counts::parse(rest)?.0, rest.get(HEADER..)?, true)
    } else {
        (v1, zone.get(HEADER..)?, false)
    };
    let tw = if wide { 8 } else { 4 };
    let times = body.get(..c.time * tw)?;
    let index = body.get(c.time * tw..c.time * (tw + 1))?;
    let types = body.get(c.time * (tw + 1)..c.time * (tw + 1) + c.types * 6)?;
    let at = |i: usize| -> i64 {
        let b = &times[i * tw..(i + 1) * tw];
        if wide {
            b.try_into().map_or(0, i64::from_be_bytes)
        } else {
            b.try_into().map_or(0, |n| i64::from(i32::from_be_bytes(n)))
        }
    };
    let utoff = |ty: usize| -> Option<i32> {
        Some(i32::from_be_bytes(
            types.get(ty * 6..ty * 6 + 4)?.try_into().ok()?,
        ))
    };
    // Transitions are in ascending order: count those at or before `t`.
    let passed = (0..c.time).take_while(|&i| at(i) <= t).count();
    if passed == c.time
        && wide
        && let Some(rule) = body.get(c.data_len(8)..).and_then(footer)
    {
        return Some(rule.offset_at(t));
    }
    match passed {
        0 => utoff(0),
        n => utoff(usize::from(*index.get(n - 1)?)),
    }
}

/// The POSIX TZ string between the footer's two newlines.
fn footer(b: &[u8]) -> Option<PosixTz> {
    let text = std::str::from_utf8(b).ok()?;
    let line = text.strip_prefix('\n')?.split('\n').next()?;
    PosixTz::parse(line)
}

/// A day rule: `Jn` (1-365, February 29 never counted), `n` (0-365), or
/// `Mm.w.d` (day `d` of week `w` of month `m`; week 5 is the last).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    Julian1(i64),
    Julian0(i64),
    Month { m: u32, w: i64, d: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dst {
    /// Seconds east of UTC in summer.
    offset: i32,
    start: (Rule, i64),
    end: (Rule, i64),
}

/// A POSIX TZ string: `std offset [dst [offset] [,start[/time],end[/time]]]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PosixTz {
    /// Seconds east of UTC in winter (POSIX writes west as positive).
    std: i32,
    dst: Option<Dst>,
}

impl PosixTz {
    fn parse(s: &str) -> Option<PosixTz> {
        let mut p = Cursor(s.as_bytes());
        p.name()?;
        let std = -p.hms()?;
        if p.0.is_empty() {
            return Some(PosixTz { std, dst: None });
        }
        p.name()?;
        let offset = match p.0.first() {
            Some(b',') | None => std + 3600,
            Some(_) => -p.hms()?,
        };
        let (start, end) = if p.0.is_empty() {
            // POSIX leaves the default rule to the implementation; this is
            // the United States' since 2007, as glibc and tzcode use.
            (
                (Rule::Month { m: 3, w: 2, d: 0 }, 7200),
                (Rule::Month { m: 11, w: 1, d: 0 }, 7200),
            )
        } else {
            p.eat(b',')?;
            let start = p.rule()?;
            p.eat(b',')?;
            (start, p.rule()?)
        };
        if !p.0.is_empty() {
            return None;
        }
        Some(PosixTz {
            std,
            dst: Some(Dst { offset, start, end }),
        })
    }

    /// Seconds east of UTC at `t`.
    fn offset_at(&self, t: i64) -> i32 {
        let Some(dst) = self.dst else {
            return self.std;
        };
        let year = year_of(t.saturating_add(i64::from(self.std)));
        // Each rule's time is local: the start in winter time, the end in
        // summer time.
        let start = rule_day(year, dst.start.0) * 86_400 + dst.start.1 - i64::from(self.std);
        let end = rule_day(year, dst.end.0) * 86_400 + dst.end.1 - i64::from(dst.offset);
        let summer = if start < end {
            start <= t && t < end
        } else {
            // Southern hemisphere: summer spans the new year.
            !(end <= t && t < start)
        };
        if summer { dst.offset } else { self.std }
    }
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn eat(&mut self, b: u8) -> Option<()> {
        let (&first, rest) = self.0.split_first()?;
        (first == b).then(|| self.0 = rest)
    }

    /// A zone abbreviation: `<…>` or three or more letters.
    fn name(&mut self) -> Option<()> {
        if self.eat(b'<').is_some() {
            let end = self.0.iter().position(|&b| b == b'>')?;
            self.0 = &self.0[end + 1..];
            return Some(());
        }
        let n = self
            .0
            .iter()
            .take_while(|b| b.is_ascii_alphabetic())
            .count();
        if n < 3 {
            return None;
        }
        self.0 = &self.0[n..];
        Some(())
    }

    fn number(&mut self) -> Option<i64> {
        let n = self.0.iter().take_while(|b| b.is_ascii_digit()).count();
        if n == 0 || n > 4 {
            return None;
        }
        let v = std::str::from_utf8(&self.0[..n]).ok()?.parse().ok()?;
        self.0 = &self.0[n..];
        Some(v)
    }

    /// `[+-]h[h][:mm[:ss]]` in seconds.
    fn hms(&mut self) -> Option<i32> {
        let sign = if self.eat(b'-').is_some() {
            -1
        } else {
            let _ = self.eat(b'+');
            1
        };
        let mut secs = self.number()? * 3600;
        for unit in [60, 1] {
            if self.eat(b':').is_none() {
                break;
            }
            secs += self.number()? * unit;
        }
        i32::try_from(sign * secs).ok()
    }

    /// A day rule and its `/time` (02:00 when absent).
    fn rule(&mut self) -> Option<(Rule, i64)> {
        let rule = if self.eat(b'J').is_some() {
            Rule::Julian1(self.number()?)
        } else if self.eat(b'M').is_some() {
            let m = self.number()?;
            self.eat(b'.')?;
            let w = self.number()?;
            self.eat(b'.')?;
            let d = self.number()?;
            if !(1..=12).contains(&m) || !(1..=5).contains(&w) || !(0..=6).contains(&d) {
                return None;
            }
            Rule::Month {
                m: u32::try_from(m).ok()?,
                w,
                d,
            }
        } else {
            Rule::Julian0(self.number()?)
        };
        let time = if self.eat(b'/').is_some() {
            i64::from(self.hms()?)
        } else {
            7200
        };
        Some((rule, time))
    }
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Days from 1970-01-01 to `y-m-d` (proleptic Gregorian).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * if m > 2 { m - 3 } else { m + 9 } + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year `t` (seconds from 1970) falls in.
fn year_of(t: i64) -> i64 {
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    yoe + era * 400 + i64::from(mp >= 10)
}

/// The day (from 1970) `rule` names in `year`.
fn rule_day(year: i64, rule: Rule) -> i64 {
    let jan1 = days_from_civil(year, 1, 1);
    match rule {
        Rule::Julian1(n) => jan1 + n - 1 + i64::from(is_leap(year) && n >= 60),
        Rule::Julian0(n) => jan1 + n,
        Rule::Month { m, w, d } => {
            let first = days_from_civil(year, m, 1);
            // 1970-01-01 was a Thursday (4; Sunday is 0).
            let weekday = (first + 4).rem_euclid(7);
            let mut day = first + (d - weekday).rem_euclid(7) + 7 * (w - 1);
            let next = if m == 12 {
                days_from_civil(year + 1, 1, 1)
            } else {
                days_from_civil(year, m + 1, 1)
            };
            while day >= next {
                day -= 7;
            }
            day
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unix seconds at `y-m-d h:mm` UTC.
    fn utc(y: i64, m: u32, d: u32, h: i64, min: i64) -> i64 {
        days_from_civil(y, m, d) * 86_400 + h * 3600 + min * 60
    }

    #[test]
    fn civil_days_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(year_of(utc(2026, 12, 31, 23, 59)), 2026);
        assert_eq!(year_of(utc(2027, 1, 1, 0, 0)), 2027);
        assert_eq!(year_of(-1), 1969);
        assert_eq!(year_of(utc(2024, 2, 29, 12, 0)), 2024);
    }

    #[test]
    fn month_rules_find_the_right_sunday() {
        let sunday = |y, m, d| days_from_civil(y, m, d);
        // US 2026: second Sunday in March, first in November.
        let us = Rule::Month { m: 3, w: 2, d: 0 };
        assert_eq!(rule_day(2026, us), sunday(2026, 3, 8));
        assert_eq!(
            rule_day(2026, Rule::Month { m: 11, w: 1, d: 0 }),
            sunday(2026, 11, 1)
        );
        // EU 2026: last Sunday in March and October.
        assert_eq!(
            rule_day(2026, Rule::Month { m: 3, w: 5, d: 0 }),
            sunday(2026, 3, 29)
        );
        assert_eq!(
            rule_day(2026, Rule::Month { m: 10, w: 5, d: 0 }),
            sunday(2026, 10, 25)
        );
        assert_eq!(rule_day(2024, Rule::Julian1(60)), sunday(2024, 3, 1));
        assert_eq!(rule_day(2024, Rule::Julian0(59)), sunday(2024, 2, 29));
    }

    #[test]
    fn posix_strings_give_the_offset_at_an_instant() {
        let pacific = PosixTz::parse("PST8PDT,M3.2.0,M11.1.0").unwrap();
        assert_eq!(pacific.offset_at(utc(2026, 1, 15, 12, 0)), -8 * 3600);
        assert_eq!(pacific.offset_at(utc(2026, 7, 1, 12, 0)), -7 * 3600);
        // 02:00 PST on 8 March is 10:00 UTC.
        assert_eq!(pacific.offset_at(utc(2026, 3, 8, 9, 59)), -8 * 3600);
        assert_eq!(pacific.offset_at(utc(2026, 3, 8, 10, 0)), -7 * 3600);
        // 02:00 PDT on 1 November is 09:00 UTC.
        assert_eq!(pacific.offset_at(utc(2026, 11, 1, 8, 59)), -7 * 3600);
        assert_eq!(pacific.offset_at(utc(2026, 11, 1, 9, 0)), -8 * 3600);

        let sydney = PosixTz::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
        assert_eq!(sydney.offset_at(utc(2026, 1, 15, 0, 0)), 11 * 3600);
        assert_eq!(sydney.offset_at(utc(2026, 7, 1, 0, 0)), 10 * 3600);

        let tehran = PosixTz::parse("<+0330>-3:30").unwrap();
        assert_eq!(tehran.offset_at(0), 3 * 3600 + 1800);
        assert_eq!(PosixTz::parse("UTC0").unwrap().offset_at(0), 0);
        assert_eq!(
            PosixTz::parse("EST5EDT")
                .unwrap()
                .offset_at(utc(2026, 7, 1, 0, 0)),
            -4 * 3600
        );
        for bad in ["", "X5", "PST", "PST8PDT,M13.1.0,M11.1.0", "PST8PDT,M3.2.0"] {
            assert_eq!(PosixTz::parse(bad), None, "{bad:?}");
        }
    }

    /// A version-2 TZif file: one transition to `+3600` at `at`, then the
    /// footer `tz`.
    fn tzif(at: i64, tz: &str) -> Vec<u8> {
        let header = |time: u32, types: u32, chars: u32| {
            let mut h = b"TZif2".to_vec();
            h.extend_from_slice(&[0; 15]);
            for n in [0, 0, 0, time, types, chars] {
                h.extend_from_slice(&u32::to_be_bytes(n));
            }
            h
        };
        let ttinfo = |off: i32| {
            let mut t = off.to_be_bytes().to_vec();
            t.extend_from_slice(&[0, 0]);
            t
        };
        // The version-1 block: no transitions, one type.
        let mut f = header(0, 1, 4);
        f.extend(ttinfo(0));
        f.extend_from_slice(b"UTC\0");
        f.extend(header(1, 2, 4));
        f.extend_from_slice(&at.to_be_bytes());
        f.push(1);
        f.extend(ttinfo(0));
        f.extend(ttinfo(3600));
        f.extend_from_slice(b"UTC\0");
        f.extend_from_slice(format!("\n{tz}\n").as_bytes());
        f
    }

    #[test]
    fn tzif_files_give_offsets_before_after_and_past_the_table() {
        let at = utc(2000, 1, 1, 0, 0);
        let zone = tzif(at, "<+02>-2");
        assert_eq!(tzif_offset(&zone, at - 1), Some(0), "type 0 before");
        // Past the last transition the footer governs.
        assert_eq!(tzif_offset(&zone, at), Some(7200));
        let no_footer = tzif(at, "");
        assert_eq!(tzif_offset(&no_footer, at + 5), Some(3600));
        assert_eq!(tzif_offset(b"TZif2", 0), None);
        assert_eq!(tzif_offset(b"not a zone", 0), None);
    }

    #[test]
    fn the_time_of_day_wraps() {
        assert_eq!(hm(utc(2026, 10, 8, 11, 38)), (11, 38));
        assert_eq!(hm(utc(2026, 10, 8, 0, 5) - 3600), (23, 5));
    }
}
