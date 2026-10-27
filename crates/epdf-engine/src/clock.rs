//! G1. Injected time, and PDF date-string conversion.
//!
//! Replaces `services/src/shared/pdf-date.ts` and every ambient `Date.now()` /
//! `new Date()` in the service layer (`SecurityReader.ts`,
//! `writeAnnotationBase.ts`, `PieceInfoAccessor.ts`, `applyMetadataPatch.ts`).
//!
//! Time is injected rather than read from the environment because the
//! cross-language corpus cannot freeze a result that depends on when it ran.
//! Corpus runners supply a [`FixedClock`]; hosts supply [`SystemClock`].
//!
//! PDF stores timestamps as ASCII shaped like `D:YYYYMMDDHHmmSSOHH'mm'`, where
//! `O` is `+`, `-` or `Z` (ISO 32000 §7.9.4). The engine's DTO wire format is
//! ISO 8601 (`IsoDateTime.ts`), so readers convert PDF to ISO and writers
//! convert back. A date keeps the offset it was written with in both
//! directions: `D:20170712214438-07'00'` reads `2017-07-12T21:44:38-07:00`
//! and writes back as it was. Nothing is normalized to UTC.

use crate::error::{EngineError, EngineErrorCode};

/// An instant plus the UTC offset to render it at.
///
/// The counterpart of a `Date` passed to `formatPdfDate`, which writes it in
/// UTC. The offset is carried so a test clock can stamp another zone;
/// [`SystemClock`] always reports zero, which writes what upstream writes.
/// See [`format_pdf_timestamp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    pub epoch_millis: i64,
    /// Minutes east of UTC. `-480` is UTC-08:00.
    pub utc_offset_minutes: i32,
}

impl Timestamp {
    pub fn utc(epoch_millis: i64) -> Self {
        Self { epoch_millis, utc_offset_minutes: 0 }
    }
}

/// The engine's only source of "now".
pub trait Clock: std::fmt::Debug {
    fn now(&self) -> Timestamp;
}

/// Reads the host clock. Always reports a UTC offset of zero: the engine
/// stamps in UTC and lets the host facade re-render for display.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(|e| -(e.duration().as_millis() as i64));
        Timestamp::utc(millis)
    }
}

/// A clock that does not move, for conformance corpus runs and tests.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

/// Broken-down UTC civil time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Civil {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

/// Days from 1970-01-01 to a proleptic Gregorian date, by Howard Hinnant's
/// `days_from_civil`. Valid for any year the i64 holds; no month tables, no
/// leap-year special cases at the call site.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The inverse, Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn civil_from_epoch_millis(epoch_millis: i64) -> Civil {
    // Floor division, so instants before 1970 land on the right day rather
    // than truncating toward zero into the next one.
    let days = epoch_millis.div_euclid(86_400_000);
    let rem = epoch_millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (rem / 3_600_000) as u32,
        minute: (rem / 60_000 % 60) as u32,
        second: (rem / 1_000 % 60) as u32,
    }
}

/// Whether the fields name a real date and time: no February 31st, no hour
/// 25. `validClock` in `pdf-date.ts`.
///
/// Validation is by construction rather than by rollover: `20250231` is
/// rejected, not silently read as March 3rd, because `days_from_civil`
/// round-trips through [`civil_from_days`] and the comparison catches it.
fn valid_clock(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> bool {
    (1..=12).contains(&month)
        && day >= 1
        && hour <= 23
        && minute <= 59
        && second <= 59
        && civil_from_days(days_from_civil(year, month, day)) == (year, month, day)
}

/// The value of a run of ASCII digits. Only called on bytes already checked
/// to be digits, and never on more than four of them.
fn digits_value(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |value, digit| value * 10 + u32::from(digit - b'0'))
}

fn all_digits(bytes: &[u8]) -> bool {
    bytes.iter().all(u8::is_ascii_digit)
}

/// The text of bytes already checked to be ASCII.
fn ascii(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("checked ASCII")
}

/// `'?` in the offset patterns: one apostrophe, if there is one.
fn skip_apostrophe(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"'").unwrap_or(bytes)
}

/// A PDF date as ISO 8601, keeping its offset. `pdfDateToIso` in
/// `pdf-date.ts`.
///
/// A zero offset (`Z`, `+00'00'`) reads as `Z`, any other offset reads as
/// `±HH:mm`, and a date without one reads without one: PDF says the relation
/// to UTC is then unknown, and none is invented. Fields the date leaves out
/// take the spec's defaults (month and day 1, the rest 0). The `D:` prefix
/// and the apostrophes of the offset are optional, as readers in the wild
/// treat them. There are no milliseconds: PDF dates have whole seconds.
///
/// `None` when the text isn't a date. The grammar is upstream's regex:
///
/// ```text
/// ^(?:D:)?(\d{4})(\d{2})?(\d{2})?(\d{2})?(\d{2})?(\d{2})?
///   (?:([Zz])(?:00'?00'?)?|([+-])(\d{2})(?:'?(\d{2})'?)?)?$
/// ```
///
/// The digit groups fill left to right, so the run of digits is 4 to 14
/// long and even, and what follows it is the offset.
pub fn pdf_date_to_iso(pdf: &str) -> Option<String> {
    // `String.prototype.trim` strips the BOM as well as Unicode white space.
    let text = pdf.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let bytes = text.as_bytes();
    let bytes = bytes.strip_prefix(b"D:").unwrap_or(bytes);
    let run = bytes.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if !(4..=14).contains(&run) || run % 2 != 0 {
        return None;
    }
    let (clock, zone) = bytes.split_at(run);

    // Each field is the two digits at its place, or the spec's default.
    let field = |at: usize, default: &'static str| {
        clock.get(at..at + 2).map_or(default, |digits| ascii(digits))
    };
    let year = ascii(&clock[..4]);
    let (month, day) = (field(4, "01"), field(6, "01"));
    let (hour, minute, second) = (field(8, "00"), field(10, "00"), field(12, "00"));
    let number = |text: &str| digits_value(text.as_bytes());
    if !valid_clock(
        i64::from(number(year)),
        number(month),
        number(day),
        number(hour),
        number(minute),
        number(second),
    ) {
        return None;
    }

    let offset = match zone {
        [] => String::new(),
        [b'Z' | b'z', rest @ ..] => {
            // `(?:00'?00'?)?`: a zero offset may follow the `Z`.
            if !rest.is_empty() {
                let rest = rest.strip_prefix(b"00")?;
                let rest = skip_apostrophe(rest).strip_prefix(b"00")?;
                if !skip_apostrophe(rest).is_empty() {
                    return None;
                }
            }
            "Z".into()
        }
        [sign @ (b'+' | b'-'), rest @ ..] => {
            let (hours, rest) = (rest.get(..2)?, &rest[2..]);
            if !all_digits(hours) {
                return None;
            }
            // `(?:'?(\d{2})'?)?`: minutes are optional, and so is each
            // apostrophe around them, but an apostrophe alone is not.
            let minutes: &[u8] = if rest.is_empty() {
                b"00"
            } else {
                let rest = skip_apostrophe(rest);
                let minutes = rest.get(..2).filter(|minutes| all_digits(minutes))?;
                if !skip_apostrophe(&rest[2..]).is_empty() {
                    return None;
                }
                minutes
            };
            let (h, m) = (digits_value(hours), digits_value(minutes));
            if h > 23 || m > 59 {
                return None;
            }
            if h == 0 && m == 0 {
                "Z".into()
            } else {
                format!("{}{}:{}", *sign as char, ascii(hours), ascii(minutes))
            }
        }
        _ => return None,
    };
    Some(format!("{year}-{month}-{day}T{hour}:{minute}:{second}{offset}"))
}

/// An ISO 8601 date and time as a PDF date, in its own offset.
/// `formatPdfDate` in `pdf-date.ts`, for a string.
///
/// A string with an offset keeps it (`-07:00` writes `-07'00'`), one without
/// writes none, and `Z` writes `+00'00'` rather than `Z`, which some readers
/// don't accept. A fraction of a second is dropped: PDF dates have whole
/// seconds. Seconds may be left out, and write as `00`.
///
/// `InvalidArg` when the text isn't a date and time. The grammar is
/// upstream's regex, which takes a lower-case `z` and requires the colon in
/// an offset, but does not range-check the offset's digits:
///
/// ```text
/// ^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?
///   (?:([Zz])|([+-])(\d{2}):(\d{2}))?$
/// ```
pub fn format_pdf_date(value: &str) -> Result<String, EngineError> {
    parse_pdf_date_input(value.as_bytes()).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("not an ISO 8601 date and time: '{value}'"),
        )
    })
}

fn parse_pdf_date_input(bytes: &[u8]) -> Option<String> {
    // YYYY-MM-DDTHH:MM is the shortest form.
    let head = bytes.get(..16)?;
    let digits_at =
        |ranges: &[(usize, usize)]| ranges.iter().all(|&(a, b)| all_digits(&head[a..b]));
    if !(digits_at(&[(0, 4), (5, 7), (8, 10), (11, 13), (14, 16)])
        && head[4] == b'-'
        && head[7] == b'-'
        && head[10] == b'T'
        && head[13] == b':')
    {
        return None;
    }
    let mut rest = &bytes[16..];

    // `(?::(\d{2})(?:\.\d+)?)?`
    let mut second: &[u8] = b"00";
    if let Some(after) = rest.strip_prefix(b":") {
        second = after.get(..2).filter(|digits| all_digits(digits))?;
        rest = &after[2..];
        if let Some(fraction) = rest.strip_prefix(b".") {
            let length = fraction.iter().take_while(|byte| byte.is_ascii_digit()).count();
            if length == 0 {
                return None;
            }
            rest = &fraction[length..];
        }
    }

    // `(?:([Zz])|([+-])(\d{2}):(\d{2}))?`
    let offset = match rest {
        [] => String::new(),
        [b'Z' | b'z'] => "+00'00'".into(),
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] if all_digits(&[*h1, *h2, *m1, *m2]) => {
            format!(
                "{}{}{}'{}{}'",
                *sign as char, *h1 as char, *h2 as char, *m1 as char, *m2 as char
            )
        }
        _ => return None,
    };

    let (year, month, day) = (&head[0..4], &head[5..7], &head[8..10]);
    let (hour, minute) = (&head[11..13], &head[14..16]);
    if !valid_clock(
        i64::from(digits_value(year)),
        digits_value(month),
        digits_value(day),
        digits_value(hour),
        digits_value(minute),
        digits_value(second),
    ) {
        return None;
    }
    Some(format!(
        "D:{}{}{}{}{}{}{offset}",
        ascii(year),
        ascii(month),
        ascii(day),
        ascii(hour),
        ascii(minute),
        ascii(second)
    ))
}

/// An instant as a PDF date. `formatPdfDate` in `pdf-date.ts`, for a `Date`.
///
/// Upstream writes a `Date` in UTC. [`SystemClock`] always reports a zero
/// offset, so a stamp from it writes `+00'00'` the same way; a
/// [`Timestamp`] that carries another offset writes the wall clock at that
/// offset. A fraction of a second is dropped.
///
/// `None` when the instant does not have a four-digit year, which PDF date
/// syntax cannot express. Upstream's `RangeError` for an invalid `Date` has
/// no counterpart: every `Timestamp` is a valid instant.
pub fn format_pdf_timestamp(at: Timestamp) -> Option<String> {
    let local = at.epoch_millis.checked_add(at.utc_offset_minutes as i64 * 60_000)?;
    let c = civil_from_epoch_millis(local);
    if !(0..=9999).contains(&c.year) {
        return None;
    }
    let sign = if at.utc_offset_minutes >= 0 { '+' } else { '-' };
    let abs = at.utc_offset_minutes.unsigned_abs();
    Some(format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}{sign}{:02}'{:02}'",
        c.year,
        c.month,
        c.day,
        c.hour,
        c.minute,
        c.second,
        abs / 60,
        abs % 60
    ))
}

/// Accepts exactly the strings `IsoDateTimeSchema` accepts
/// (`core/src/dto/IsoDateTime.schema.ts`), which is zod 3's
/// `z.string().datetime({ offset: true, local: true })`:
///
/// ```text
/// ^DATE T([01]\d|2[0-3]):[0-5]\d(:[0-5]\d(\.\d+)?)?(Z?|[+-]\d{2}:?\d{2})$
/// ```
///
/// where `DATE` is `YYYY-MM-DD` naming a real Gregorian day (leap years
/// included). Seconds and a fraction are optional, `Z` is upper case only,
/// no zone at all is the `local` form, and an offset may drop its colon. zod
/// does not range-check the offset's digits, so neither does this.
///
/// The schema is the shipping validator on the wire. A Rust type that took
/// any string would accept what zod refuses, and the same request would be
/// well formed on one engine and not on the other.
pub fn is_iso_date_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(head) = bytes.get(..16) else {
        return false;
    };
    let digits_at =
        |ranges: &[(usize, usize)]| ranges.iter().all(|&(a, b)| all_digits(&head[a..b]));
    if !(digits_at(&[(0, 4), (5, 7), (8, 10), (11, 13), (14, 16)])
        && head[4] == b'-'
        && head[7] == b'-'
        && head[10] == b'T'
        && head[13] == b':')
    {
        return false;
    }
    let mut rest = &bytes[16..];
    let mut second = 0;
    if let Some(after) = rest.strip_prefix(b":") {
        let Some(digits) = after.get(..2).filter(|digits| all_digits(digits)) else {
            return false;
        };
        second = digits_value(digits);
        rest = &after[2..];
        if let Some(fraction) = rest.strip_prefix(b".") {
            let length = fraction.iter().take_while(|byte| byte.is_ascii_digit()).count();
            if length == 0 {
                return false;
            }
            rest = &fraction[length..];
        }
    }
    let zone_ok = match rest {
        [] | [b'Z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] | [b'+' | b'-', h1, h2, m1, m2] => {
            all_digits(&[*h1, *h2, *m1, *m2])
        }
        _ => false,
    };
    zone_ok
        && valid_clock(
            i64::from(digits_value(&head[0..4])),
            digits_value(&head[5..7]),
            digits_value(&head[8..10]),
            digits_value(&head[11..13]),
            digits_value(&head[14..16]),
            second,
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_conversions_round_trip_across_epochs_and_leap_years() {
        for (y, m, d) in [
            (1970, 1, 1),
            (1969, 12, 31),
            (2000, 2, 29),
            (1900, 3, 1),
            (2024, 2, 29),
            (9999, 12, 31),
            (1, 1, 1),
        ] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d), "{y}-{m}-{d}");
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
    }

    // `packages/engine/main/test/pdf-date.test.ts`, case for case.

    #[test]
    fn reads_a_pdf_date_keeping_its_offset() {
        for (pdf, expected) in [
            ("D:20260713150000+03'00'", "2026-07-13T15:00:00+03:00"),
            ("D:20260713150000-03'00'", "2026-07-13T15:00:00-03:00"),
            ("D:20260713150000+05'45'", "2026-07-13T15:00:00+05:45"),
            ("D:20260713150000Z", "2026-07-13T15:00:00Z"),
            ("D:20260713150000+00'00'", "2026-07-13T15:00:00Z"),
            ("D:20260713150000Z00'00'", "2026-07-13T15:00:00Z"),
            // No offset: the relation to UTC is unknown, so none is invented.
            ("D:20260713150000", "2026-07-13T15:00:00"),
            // Readers in the wild: no prefix, loose apostrophes, an hours-only offset.
            ("20260713150000Z", "2026-07-13T15:00:00Z"),
            ("D:20260713150000+03'00", "2026-07-13T15:00:00+03:00"),
            ("D:20260713150000-07", "2026-07-13T15:00:00-07:00"),
            // Left-out fields take the spec's defaults.
            ("D:2026", "2026-01-01T00:00:00"),
            ("D:202607131500+02'00'", "2026-07-13T15:00:00+02:00"),
        ] {
            assert_eq!(pdf_date_to_iso(pdf).as_deref(), Some(expected), "{pdf}");
        }
    }

    #[test]
    fn reads_a_malformed_pdf_date_as_none() {
        for pdf in [
            "",
            "not a date",
            "D:20261313150000+03'00'",
            "D:20260231150000+03'00'",
            "D:20250229120000Z",
            "D:20260713250000+03'00'",
            "D:20260713150000+24'00'",
            "D:20260713150000+03'60'",
            "D:20260713150000garbage",
        ] {
            assert_eq!(pdf_date_to_iso(pdf), None, "{pdf:?}");
        }
    }

    #[test]
    fn a_leap_day_is_a_date() {
        assert_eq!(pdf_date_to_iso("D:20240229120000Z").as_deref(), Some("2024-02-29T12:00:00Z"));
    }

    #[test]
    fn writes_an_iso_date_in_its_own_offset() {
        for (iso, expected) in [
            ("2017-07-12T21:44:38-07:00", "D:20170712214438-07'00'"),
            ("2017-07-12T21:44:38+05:30", "D:20170712214438+05'30'"),
            ("2017-07-12T21:44:38Z", "D:20170712214438+00'00'"),
            ("2017-07-12T21:44:38", "D:20170712214438"),
            ("2017-07-12T21:44:38.999Z", "D:20170712214438+00'00'"),
            ("2017-07-12T21:44Z", "D:20170712214400+00'00'"),
        ] {
            assert_eq!(format_pdf_date(iso).unwrap(), expected, "{iso}");
        }
    }

    #[test]
    fn a_read_date_written_back_is_the_same_pdf_date() {
        for pdf in ["D:20170712214438-07'00'", "D:20170712214438+05'30'", "D:20170712214438"] {
            assert_eq!(format_pdf_date(&pdf_date_to_iso(pdf).unwrap()).unwrap(), pdf);
        }
    }

    #[test]
    fn an_instant_is_written_in_utc() {
        // `new Date('2026-07-13T23:59:58.500+02:00')`: the fraction is dropped.
        let at = Timestamp::utc(1_783_979_998_500);
        assert_eq!(format_pdf_timestamp(at).unwrap(), "D:20260713215958+00'00'");
    }

    #[test]
    fn an_invalid_string_is_refused() {
        for value in ["2026-02-30T00:00:00Z", "yesterday"] {
            let error = format_pdf_date(value).unwrap_err();
            assert_eq!(error.code, EngineErrorCode::InvalidArg, "{value}");
            assert!(error.message.contains("not an ISO 8601"), "{value}: {}", error.message);
        }
    }

    // `packages/engine/main/test/dates.test.ts` runs `runDateConformance`
    // against `fixtures/dates.pdf`. These are its dates, at the converter;
    // the readers that surface them adopt it in 8.6, 8.8, 8.12 and 8.19.

    #[test]
    fn the_date_fixtures_annotation_dates_read_with_their_offsets() {
        for (pdf, expected) in [
            // dates-offset
            ("D:20170712214438-07'00'", Some("2017-07-12T21:44:38-07:00")),
            ("D:20170713091500+05'30'", Some("2017-07-13T09:15:00+05:30")),
            // dates-local: no offset reads without one; `Z` stays UTC.
            ("D:20170712214438", Some("2017-07-12T21:44:38")),
            ("D:20170712214438Z", Some("2017-07-12T21:44:38Z")),
            // dates-odd: text that isn't a date, and a year alone.
            ("not a date", None),
            ("D:2017", Some("2017-01-01T00:00:00")),
            // dates-file's /Params, and the embedded file's.
            ("D:20180101120000+01'00'", Some("2018-01-01T12:00:00+01:00")),
            // The Info dictionary.
            ("D:20190203040506+09'00'", Some("2019-02-03T04:05:06+09:00")),
        ] {
            assert_eq!(pdf_date_to_iso(pdf).as_deref(), expected, "{pdf}");
        }
    }

    #[test]
    fn the_date_fixtures_metadata_writes_read_back_as_written() {
        // A string keeps its offset through the write and the read.
        let written = format_pdf_date("2020-01-02T03:04:05+05:30").unwrap();
        assert_eq!(written, "D:20200102030405+05'30'");
        assert_eq!(pdf_date_to_iso(&written).as_deref(), Some("2020-01-02T03:04:05+05:30"));

        // `new Date('2021-06-07T10:09:10.500+02:00')`: an instant, written in
        // UTC, whole seconds.
        let written = format_pdf_timestamp(Timestamp::utc(1_623_053_350_500)).unwrap();
        assert_eq!(pdf_date_to_iso(&written).as_deref(), Some("2021-06-07T08:09:10Z"));
    }

    #[test]
    fn an_engine_stamp_reads_back_in_utc() {
        // An update stamps `/M` itself; the read ends in `Z`.
        let stamp = format_pdf_timestamp(SystemClock.now()).unwrap();
        assert!(pdf_date_to_iso(&stamp).unwrap().ends_with('Z'), "{stamp}");
    }

    #[test]
    fn nothing_normalizes_to_utc() {
        // The same instant in two zones reads as two strings.
        assert_eq!(
            pdf_date_to_iso("D:20170712214438-07'00'").as_deref(),
            Some("2017-07-12T21:44:38-07:00")
        );
        assert_eq!(
            pdf_date_to_iso("D:20170713044438+00'00'").as_deref(),
            Some("2017-07-13T04:44:38Z")
        );
    }

    #[test]
    fn reads_the_offset_spellings_the_grammar_allows() {
        for (pdf, expected) in [
            ("D:20260713150000z", Some("2026-07-13T15:00:00Z")),
            ("D:20260713150000Z0000", Some("2026-07-13T15:00:00Z")),
            ("D:20260713150000+0300", Some("2026-07-13T15:00:00+03:00")),
            ("D:20260713150000+03'00'", Some("2026-07-13T15:00:00+03:00")),
            ("D:20260713150000+0300'", Some("2026-07-13T15:00:00+03:00")),
            ("  D:20260713150000Z\n", Some("2026-07-13T15:00:00Z")),
            ("D:202607", Some("2026-07-01T00:00:00")),
            // An apostrophe alone, an odd digit, a short offset, a `Z` with
            // a non-zero tail, and fifteen digits.
            ("D:20260713150000+03'", None),
            ("D:2026071", None),
            ("D:20260713150000+3", None),
            ("D:20260713150000Z01'00'", None),
            ("D:202607131500001", None),
            ("D:", None),
            ("d:20260713150000Z", None),
        ] {
            assert_eq!(pdf_date_to_iso(pdf).as_deref(), expected, "{pdf:?}");
        }
    }

    #[test]
    fn writes_only_what_the_formatter_grammar_takes() {
        assert_eq!(format_pdf_date("2017-07-12T21:44:38z").unwrap(), "D:20170712214438+00'00'");
        assert_eq!(format_pdf_date("2017-07-12T21:44").unwrap(), "D:20170712214400");
        for value in [
            "2017-07-12T21:44:38+0530",
            "2017-07-12T21:44:38.Z",
            "2017-07-12 21:44:38Z",
            "2017-07-12T21:44:3Z",
            "2017-07-12T24:00:00Z",
            "2017-07-12T21:44:38Z ",
        ] {
            assert!(format_pdf_date(value).is_err(), "{value}");
        }
    }

    #[test]
    fn refuses_years_pdf_date_syntax_cannot_express() {
        // Year 10000 and year -1 both need more than four digits.
        assert_eq!(format_pdf_timestamp(Timestamp::utc(253_402_300_800_000)), None);
        assert_eq!(format_pdf_timestamp(Timestamp::utc(-62_167_219_200_001)), None);
    }

    #[test]
    fn an_instant_before_1970_lands_on_the_right_day() {
        // Truncating division would put this on 1970-01-01.
        assert_eq!(format_pdf_timestamp(Timestamp::utc(-1)).unwrap(), "D:19691231235959+00'00'");
    }

    #[test]
    fn a_timestamp_with_an_offset_writes_its_wall_clock() {
        let at = Timestamp { epoch_millis: 1_704_164_645_000, utc_offset_minutes: -480 };
        // 03:04:05Z is 19:04:05 the previous day at UTC-08:00.
        assert_eq!(format_pdf_timestamp(at).unwrap(), "D:20240101190405-08'00'");
    }

    #[test]
    fn a_fixed_clock_does_not_move() {
        let clock = FixedClock(Timestamp::utc(42));
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now().epoch_millis, 42);
    }

    #[test]
    fn iso_date_time_validation_accepts_what_the_zod_schema_accepts() {
        for accepted in [
            "2026-09-22T08:00:00Z",
            "2026-09-22T08:00:00.000Z",
            "2026-09-22T08:00:00.123456Z",
            "2024-02-29T00:00:00Z",
            "0000-02-29T00:00:00Z",
            "9999-12-31T23:59:59Z",
            // `offset: true`, with or without the colon, digits unchecked.
            "2026-09-22T08:00:00+02:00",
            "2026-09-22T08:00:00-0200",
            "2026-09-22T08:00:00+99:99",
            // `local: true`: no zone at all, and seconds are optional.
            "2026-09-22T08:00:00",
            "2026-09-22T08:00",
            "2026-09-22T08:00Z",
        ] {
            assert!(is_iso_date_time(accepted), "{accepted}");
        }
        // What the reader emits must always be accepted, or a read could not
        // go back into a write.
        for pdf in ["D:20170712214438-07'00'", "D:20170712214438", "D:2017", "D:20170712214438Z"] {
            assert!(is_iso_date_time(&pdf_date_to_iso(pdf).unwrap()), "{pdf}");
        }
    }

    #[test]
    fn iso_date_time_validation_rejects_what_the_zod_schema_rejects() {
        for rejected in [
            "",
            "not-a-date",
            "yesterday",
            // Impossible calendar dates.
            "2025-02-30T00:00:00Z",
            "2025-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2025-04-31T00:00:00Z",
            "2025-13-01T00:00:00Z",
            "2025-00-10T00:00:00Z",
            "2025-01-00T00:00:00Z",
            // Out-of-range clock fields, leap second included.
            "2025-01-01T24:00:00Z",
            "2025-01-01T00:60:00Z",
            "2025-01-01T00:00:60Z",
            // A dot with no digits after it, and a fraction without seconds.
            "2025-01-01T00:00:00.Z",
            "2025-01-01T00:00.5Z",
            // Separators in the wrong place.
            "2025/01/01T00:00:00Z",
            "2025-01-01 00:00:00Z",
            // Lower-case zone, a trailing space, a short or long offset.
            "2025-01-01T00:00:00z",
            "2025-01-01T00:00:00Z ",
            "2025-01-01T00:00:00+02",
            "2025-01-01T00:00:00+02:000",
            "2025-01-01T00:00:00Z+02:00",
            // Expanded years.
            "+010000-01-01T00:00:00Z",
            "-000001-01-01T00:00:00Z",
        ] {
            assert!(!is_iso_date_time(rejected), "{rejected}");
        }
    }
}
