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
//! always ISO 8601, so readers convert PDF to ISO and writers convert back.

/// An instant plus the UTC offset to render it at.
///
/// The offset is carried explicitly, which is the one deliberate difference
/// from `formatPdfDate`. That function reads the host's local zone through
/// `Date.prototype.getTimezoneOffset`, and there is no portable equivalent
/// across a WASM worker, an iOS process and a Linux server, nor any way to
/// freeze it for the corpus. So the clock supplies it and the formatter uses
/// what it is given.
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
    milli: u32,
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
        milli: (rem % 1_000) as u32,
    }
}

/// Parses `D:YYYYMMDDHHmmSSOHH'mm'` to an ISO 8601 instant, or `None` when the
/// input is malformed.
///
/// PDF says an omitted timezone means the relationship to UTC is unknown. The
/// wire format needs an absolute instant, so this keeps the pre-existing
/// fallback of reading an omitted zone as UTC.
///
/// Field validation is by construction rather than by rollover: `20250231`
/// is rejected, not silently read as March 3rd, because `days_from_civil`
/// round-trips through [`civil_from_days`] and the comparison catches it.
pub fn pdf_date_to_iso(pdf: &str) -> Option<String> {
    let rest = pdf.strip_prefix("D:")?;
    let bytes = rest.as_bytes();
    if bytes.len() < 14 || !bytes[..14].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let num = |from: usize, to: usize| rest[from..to].parse::<i64>().ok();
    let year = num(0, 4)?;
    let month = num(4, 6)? as u32;
    let day = num(6, 8)? as u32;
    let hour = num(8, 10)? as u32;
    let minute = num(10, 12)? as u32;
    let second = num(12, 14)? as u32;

    if !(1..=12).contains(&month) || day < 1 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    // Rejects a day past the end of its month, including February 29th in a
    // non-leap year, without a month-length table.
    if civil_from_days(days) != (year, month, day) {
        return None;
    }

    let offset_minutes = match &rest[14..] {
        "" | "Z" => 0,
        tz => {
            let tz = tz.as_bytes();
            // OHH'mm' is seven bytes: sign, two digits, quote, two digits,
            // quote. Anything else is malformed, including a missing trailing
            // quote, which is common in the wild but not what this parser
            // accepts.
            if tz.len() != 7 || tz[3] != b'\'' || tz[6] != b'\'' {
                return None;
            }
            let sign = match tz[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hours: i64 = rest[15..17].parse().ok()?;
            let minutes: i64 = rest[18..20].parse().ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 60 + minutes)
        }
    };

    let wall_clock_millis = days
        .checked_mul(86_400_000)?
        .checked_add((hour as i64 * 3600 + minute as i64 * 60 + second as i64) * 1000)?;
    let epoch_millis = wall_clock_millis.checked_sub(offset_minutes * 60_000)?;
    Some(iso_8601(epoch_millis))
}

/// Renders an instant the way `Date.prototype.toISOString` does: always UTC,
/// always milliseconds, and the expanded `±YYYYYY` year form outside
/// `0000..=9999`.
pub fn iso_8601(epoch_millis: i64) -> String {
    let c = civil_from_epoch_millis(epoch_millis);
    let year = if (0..=9999).contains(&c.year) {
        format!("{:04}", c.year)
    } else if c.year > 9999 {
        format!("+{:06}", c.year)
    } else {
        format!("-{:06}", -c.year)
    };
    format!(
        "{year}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        c.month, c.day, c.hour, c.minute, c.second, c.milli
    )
}

/// Turns a validated ISO 8601 UTC string back into an instant.
///
/// The inverse of [`iso_8601`], and only for strings [`is_iso_8601_utc`] has
/// accepted: the wire carries dates in that form and a patch writing one to
/// `/CreationDate` has to convert it. `None` for anything else, which by the
/// time this is reached means the validator and this function disagree.
pub fn iso_8601_to_timestamp(value: &str) -> Option<Timestamp> {
    if !is_iso_8601_utc(value) {
        return None;
    }
    let bytes = value.as_bytes();
    let number = |from: usize, to: usize| -> Option<i64> {
        std::str::from_utf8(&bytes[from..to]).ok()?.parse().ok()
    };
    let year = number(0, 4)?;
    let month = number(5, 7)? as u32;
    let day = number(8, 10)? as u32;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    // The fraction is optional and is whatever sits between the seconds and
    // the `Z`. Only the first three digits are milliseconds; a longer one is
    // truncated rather than rounded, which is what a millisecond clock can
    // represent.
    let milli = if bytes.get(19) == Some(&b'.') {
        let end = value.len() - 1;
        let digits = &value[20..end];
        let mut millis = 0i64;
        for (index, digit) in digits.bytes().take(3).enumerate() {
            millis += i64::from(digit - b'0') * 10i64.pow(2 - index as u32);
        }
        millis
    } else {
        0
    };
    let days = days_from_civil(year, month, day);
    Some(Timestamp::utc(
        days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000 + milli,
    ))
}

/// Formats an instant as a PDF date string.
///
/// Emits the offset in PDF's `OHH'mm'` form, where the apostrophe after the
/// hour is literal and the sign is always present. UTC is emitted as `+00'00'`
/// rather than `Z`: the shorthand is legal but not universally supported by
/// readers.
///
/// `None` when the instant does not have a four-digit year, which PDF date
/// syntax cannot express.
pub fn format_pdf_date(at: Timestamp) -> Option<String> {
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

/// Accepts exactly the strings `z.string().datetime()` accepts in
/// `wire/schemas.ts`: `YYYY-MM-DDTHH:MM:SS[.fraction]Z`, four-digit year, a
/// real calendar date, and a literal `Z`.
///
/// The wire carries ISO 8601 for `/CreationDate` and `/ModDate`, and the zod
/// schema is the shipping validator for it. A Rust type that took any string
/// there would accept `+02:00` offsets, `2025-02-30`, and `not-a-date`, which
/// the schema refuses: the two would disagree about whether a request is well
/// formed, and only one of them runs on mobile.
///
/// Deliberately narrow, matching zod rather than ISO 8601 at large: no offsets
/// (zod's default has `offset: false`), no `local` form, no leap second, no
/// expanded year. `iso_8601` never emits any of those either.
pub fn is_iso_8601_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    // YYYY-MM-DDTHH:MM:SS plus the mandatory Z is the shortest accepted form.
    if bytes.len() < 20 || bytes[bytes.len() - 1] != b'Z' {
        return false;
    }
    let digits = |from: usize, to: usize| bytes[from..to].iter().all(u8::is_ascii_digit);
    if !(digits(0, 4)
        && bytes[4] == b'-'
        && digits(5, 7)
        && bytes[7] == b'-'
        && digits(8, 10)
        && bytes[10] == b'T'
        && digits(11, 13)
        && bytes[13] == b':'
        && digits(14, 16)
        && bytes[16] == b':'
        && digits(17, 19))
    {
        return false;
    }

    // Anything between the seconds and the Z must be a dot and at least one
    // digit. zod's `(\.\d+)?` allows any precision, so this does too.
    let fraction = &bytes[19..bytes.len() - 1];
    if !fraction.is_empty()
        && (fraction[0] != b'.'
            || fraction.len() < 2
            || !fraction[1..].iter().all(u8::is_ascii_digit))
    {
        return false;
    }

    let num = |from: usize, to: usize| value[from..to].parse::<i64>().unwrap_or(-1);
    let (year, month, day) = (num(0, 4), num(5, 7), num(8, 10));
    let (hour, minute, second) = (num(11, 13), num(14, 16), num(17, 19));
    if !(1..=12).contains(&month) || day < 1 || hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    // Rejects a day past the end of its month, February 29th in a non-leap
    // year included, without a month-length table.
    civil_from_days(days_from_civil(year, month as u32, day as u32))
        == (year, month as u32, day as u32)
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

    #[test]
    fn parses_a_pdf_date_with_each_timezone_form() {
        assert_eq!(pdf_date_to_iso("D:20240102030405Z"), Some("2024-01-02T03:04:05.000Z".into()));
        assert_eq!(pdf_date_to_iso("D:20240102030405"), Some("2024-01-02T03:04:05.000Z".into()));
        // +05'30' means the wall clock is ahead of UTC, so the instant is
        // earlier than the digits read.
        assert_eq!(
            pdf_date_to_iso("D:20240102030405+05'30'"),
            Some("2024-01-01T21:34:05.000Z".into())
        );
        assert_eq!(
            pdf_date_to_iso("D:20240102030405-08'00'"),
            Some("2024-01-02T11:04:05.000Z".into())
        );
    }

    #[test]
    fn rejects_dates_javascript_would_silently_roll_over() {
        // The TypeScript reader constructs the fields then compares them back,
        // for exactly this. February 31st is not March 3rd.
        assert_eq!(pdf_date_to_iso("D:20250231000000"), None);
        assert_eq!(pdf_date_to_iso("D:20230229000000"), None);
        assert_eq!(pdf_date_to_iso("D:20240229000000"), Some("2024-02-29T00:00:00.000Z".into()));
        assert_eq!(pdf_date_to_iso("D:20240001000000"), None);
        assert_eq!(pdf_date_to_iso("D:20241301000000"), None);
        assert_eq!(pdf_date_to_iso("D:20240100000000"), None);
        assert_eq!(pdf_date_to_iso("D:20240102240000"), None);
        assert_eq!(pdf_date_to_iso("D:20240102006000"), None);
    }

    #[test]
    fn rejects_malformed_shapes_rather_than_guessing() {
        assert_eq!(pdf_date_to_iso("20240102030405"), None); // no D: prefix
        assert_eq!(pdf_date_to_iso("D:202401020304"), None); // too short
        assert_eq!(pdf_date_to_iso("D:2024010203040x"), None); // non-digit
        assert_eq!(pdf_date_to_iso("D:20240102030405+05'30"), None); // unterminated
        assert_eq!(pdf_date_to_iso("D:20240102030405+0530"), None); // no quotes
        assert_eq!(pdf_date_to_iso("D:20240102030405*05'30'"), None); // bad sign
        assert_eq!(pdf_date_to_iso("D:20240102030405+25'00'"), None); // hour > 23
        assert_eq!(pdf_date_to_iso("D:20240102030405+05'60'"), None); // minute > 59
    }

    #[test]
    fn formats_utc_as_an_explicit_zero_offset_not_z() {
        let at = Timestamp::utc(1_704_164_645_000); // 2024-01-02T03:04:05Z
        assert_eq!(format_pdf_date(at).unwrap(), "D:20240102030405+00'00'");
    }

    #[test]
    fn formats_at_the_offset_it_is_given() {
        let at = Timestamp { epoch_millis: 1_704_164_645_000, utc_offset_minutes: -480 };
        // 03:04:05Z is 19:04:05 the previous day at UTC-08:00.
        assert_eq!(format_pdf_date(at).unwrap(), "D:20240101190405-08'00'");

        let half_hour = Timestamp { epoch_millis: 1_704_164_645_000, utc_offset_minutes: 330 };
        assert_eq!(format_pdf_date(half_hour).unwrap(), "D:20240102083405+05'30'");
    }

    #[test]
    fn round_trips_a_formatted_date_back_through_the_parser() {
        let at = Timestamp { epoch_millis: 1_704_164_645_000, utc_offset_minutes: 330 };
        let pdf = format_pdf_date(at).unwrap();
        assert_eq!(pdf_date_to_iso(&pdf), Some(iso_8601(at.epoch_millis)));
    }

    #[test]
    fn refuses_years_pdf_date_syntax_cannot_express() {
        // Year 10000 and year -1 both need more than four digits.
        assert_eq!(format_pdf_date(Timestamp::utc(253_402_300_800_000)), None);
        assert_eq!(format_pdf_date(Timestamp::utc(-62_167_219_200_001)), None);
    }

    #[test]
    fn iso_renders_pre_epoch_instants_on_the_right_day() {
        // Truncating division would put this on 1970-01-01.
        assert_eq!(iso_8601(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(iso_8601(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_8601(253_402_300_800_000), "+010000-01-01T00:00:00.000Z");
    }

    #[test]
    fn a_fixed_clock_does_not_move() {
        let clock = FixedClock(Timestamp::utc(42));
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now().epoch_millis, 42);
    }

    #[test]
    fn iso_8601_validation_accepts_what_the_zod_schema_accepts() {
        for accepted in [
            "2026-09-22T08:00:00Z",
            "2026-09-22T08:00:00.000Z",
            "2026-09-22T08:00:00.123456Z",
            "2024-02-29T00:00:00Z",
            "0000-01-01T00:00:00Z",
            "9999-12-31T23:59:59Z",
        ] {
            assert!(is_iso_8601_utc(accepted), "{accepted}");
        }
        // What iso_8601 itself emits must always be accepted, or a value this
        // engine produced would fail validation on the way back in.
        assert!(is_iso_8601_utc(&iso_8601(0)));
        assert!(is_iso_8601_utc(&iso_8601(1_789_000_000_000)));
    }

    #[test]
    fn iso_8601_validation_rejects_what_the_zod_schema_rejects() {
        for rejected in [
            "",
            "not-a-date",
            // Offsets: zod's datetime() defaults to offset: false.
            "2026-09-22T08:00:00+02:00",
            "2026-09-22T08:00:00-0200",
            // No zone at all.
            "2026-09-22T08:00:00",
            // Impossible calendar dates.
            "2025-02-30T00:00:00Z",
            "2025-02-29T00:00:00Z",
            "2025-04-31T00:00:00Z",
            "2025-13-01T00:00:00Z",
            "2025-00-10T00:00:00Z",
            "2025-01-00T00:00:00Z",
            // Out-of-range clock fields, leap second included.
            "2025-01-01T24:00:00Z",
            "2025-01-01T00:60:00Z",
            "2025-01-01T00:00:60Z",
            // A dot with no digits after it.
            "2025-01-01T00:00:00.Z",
            // Separators in the wrong place.
            "2025/01/01T00:00:00Z",
            "2025-01-01 00:00:00Z",
            // Lower-case zone, and a trailing space.
            "2025-01-01T00:00:00z",
            "2025-01-01T00:00:00Z ",
            // Expanded years: iso_8601 emits these outside 0000..=9999, and
            // the schema refuses them. Documented rather than fixed here.
            "+010000-01-01T00:00:00Z",
            "-000001-01-01T00:00:00Z",
        ] {
            assert!(!is_iso_8601_utc(rejected), "{rejected}");
        }
    }
}
