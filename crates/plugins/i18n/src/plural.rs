//! `pluralCategory` from `translate.ts`: `new Intl.PluralRules(locale).select(count)`,
//! with the `count === 1` guess when the constructor throws.
//!
//! `Intl.PluralRules` is three steps, and each has a JavaScript answer this
//! module reproduces:
//!
//! 1. The tag must be a structurally valid Unicode BCP 47 locale identifier
//!    (ECMA-402 `IsStructurallyValidLanguageTag`), or the constructor throws
//!    a `RangeError`. [`parse`] is that grammar.
//! 2. The canonical tag is matched against the locales V8 supports; one it
//!    does not support resolves to the default locale, `en-US` under the
//!    generator. [`crate::plural_data`] holds V8's alias and support tables.
//! 3. `select` formats the number with the default digit options (at most
//!    three fraction digits, rounding half away from zero, no trailing
//!    zeros) and evaluates the locale's CLDR rules over its operands the way
//!    ICU4C does ([`operands`], [`evaluate`]). The rules are CLDR's, as data
//!    in [`crate::plural_data`], found by the resolved locale with ICU's
//!    truncation fallback (`pt-PT` has rules of its own, `pt-BR` uses
//!    `pt`'s); a locale CLDR has no rules for selects `other`, as ICU's root
//!    does.

use crate::js::shortest_digits;
use crate::plural_data::{LANGUAGE_ALIASES, RULES, SUPPORTED_LANGUAGES};

/// The parts of a valid tag that plural rules can depend on, lowercased.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageTag {
    pub language: String,
    pub script: Option<String>,
    pub region: Option<String>,
    pub variants: Vec<String>,
}

fn alpha(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn digit(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_digit())
}

fn is_language(s: &str) -> bool {
    alpha(s) && matches!(s.len(), 2 | 3 | 5..=8)
}

fn is_script(s: &str) -> bool {
    s.len() == 4 && alpha(s)
}

fn is_region(s: &str) -> bool {
    (s.len() == 2 && alpha(s)) || (s.len() == 3 && digit(s))
}

fn is_variant(s: &str) -> bool {
    matches!(s.len(), 5..=8) || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit())
}

/// `unicode_language_subtag (-script)? (-region)? (-variant)*` from `at`,
/// with no variant twice. Returns the parsed id and where it stopped.
fn language_id(subtags: &[String], mut at: usize) -> Option<(LanguageTag, usize)> {
    let language = subtags.get(at).filter(|s| is_language(s))?.clone();
    at += 1;
    let script = subtags.get(at).filter(|s| is_script(s)).cloned();
    at += usize::from(script.is_some());
    let region = subtags.get(at).filter(|s| is_region(s)).cloned();
    at += usize::from(region.is_some());
    let mut variants: Vec<String> = Vec::new();
    while let Some(variant) = subtags.get(at).filter(|s| is_variant(s)) {
        if variants.contains(variant) {
            return None;
        }
        variants.push(variant.clone());
        at += 1;
    }
    Some((LanguageTag { language, script, region, variants }, at))
}

/// `-u-`: `(attribute)* (key (type)*)*`, at least one subtag.
fn unicode_extension(body: &[String]) -> bool {
    if body.is_empty() {
        return false;
    }
    let mut keywords = false;
    for subtag in body {
        if subtag.len() == 2 {
            if !subtag.as_bytes()[1].is_ascii_alphabetic() {
                return false;
            }
            keywords = true;
        } else if !keywords && !(3..=8).contains(&subtag.len()) {
            return false;
        }
    }
    true
}

/// `-t-`: `tlang? (tkey tvalue+)*`, at least one of them.
fn transformed_extension(body: &[String]) -> bool {
    if body.is_empty() {
        return false;
    }
    let mut at = 0;
    if is_language(&body[0]) {
        match language_id(body, 0) {
            Some((_, end)) => at = end,
            None => return false,
        }
    }
    while at < body.len() {
        let key = body[at].as_bytes();
        if !(key.len() == 2 && key[0].is_ascii_alphabetic() && key[1].is_ascii_digit()) {
            return false;
        }
        at += 1;
        let values = body[at..].iter().take_while(|s| (3..=8).contains(&s.len())).count();
        if values == 0 {
            return false;
        }
        at += values;
    }
    true
}

/// ECMA-402 `IsStructurallyValidLanguageTag`. `None` is the `RangeError`.
pub fn parse(tag: &str) -> Option<LanguageTag> {
    if tag.is_empty() || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    let subtags: Vec<String> = tag.split('-').map(str::to_ascii_lowercase).collect();
    if subtags.iter().any(|s| s.is_empty() || s.len() > 8) {
        return None;
    }
    let (id, mut at) = language_id(&subtags, 0)?;
    let mut singletons: Vec<&str> = Vec::new();
    while at < subtags.len() {
        let singleton = subtags[at].as_str();
        if singleton.len() != 1 {
            return None;
        }
        if singleton == "x" {
            // Private use runs to the end: one to eight characters each.
            return (at + 1 < subtags.len()).then_some(id);
        }
        if singletons.contains(&singleton) {
            return None;
        }
        singletons.push(singleton);
        let end = subtags[at + 1..]
            .iter()
            .position(|s| s.len() == 1)
            .map_or(subtags.len(), |p| at + 1 + p);
        let body = &subtags[at + 1..end];
        let valid = match singleton {
            "u" => unicode_extension(body),
            "t" => transformed_extension(body),
            _ => !body.is_empty() && body.iter().all(|s| s.len() >= 2),
        };
        if !valid {
            return None;
        }
        at = end;
    }
    Some(id)
}

/// The tag after V8's canonicalization and locale matching: an alias
/// replaced by its canonical form, and a language V8 has no plural data
/// for replaced by the default locale's.
pub fn resolve(tag: &LanguageTag) -> LanguageTag {
    let mut tag = tag.clone();
    if let Ok(i) = LANGUAGE_ALIASES.binary_search_by(|(alias, _)| alias.cmp(&tag.language.as_str()))
    {
        let mut canonical = LANGUAGE_ALIASES[i].1.split('-');
        tag.language = canonical.next().unwrap_or_default().to_string();
        for part in canonical {
            if is_script(part) && tag.script.is_none() {
                tag.script = Some(part.to_ascii_lowercase());
            } else if is_region(part) && tag.region.is_none() {
                tag.region = Some(part.to_ascii_lowercase());
            }
        }
    }
    if SUPPORTED_LANGUAGES.binary_search(&tag.language.as_str()).is_err() {
        return LanguageTag {
            language: "en".into(),
            script: None,
            region: Some("us".into()),
            variants: Vec::new(),
        };
    }
    tag
}

/// The rules for a resolved tag: the longest `language[-script][-region]...`
/// prefix CLDR has rules for.
fn rules_for(tag: &LanguageTag) -> Option<&'static [(&'static str, &'static str)]> {
    let mut parts: Vec<&str> = vec![tag.language.as_str()];
    parts.extend(tag.script.as_deref());
    parts.extend(tag.region.as_deref());
    parts.extend(tag.variants.iter().map(String::as_str));
    while !parts.is_empty() {
        let id = parts.join("-");
        if let Ok(i) = RULES.binary_search_by(|(locale, _)| locale.cmp(&id.as_str())) {
            return Some(RULES[i].1);
        }
        parts.pop();
    }
    None
}

/// ICU4C's plural operands of a formatted number, as the doubles its rules
/// compare.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Operands {
    /// The absolute value.
    pub n: f64,
    /// The integer digits, truncated to the lowest eighteen.
    pub i: f64,
    /// Visible fraction digits (trailing zeros are never visible here).
    pub v: f64,
    /// The fraction digits as an integer.
    pub f: f64,
}

/// The operands `Intl.PluralRules.prototype.select` hands the rules: the
/// shortest digits of `|count|`, rounded half away from zero to three
/// fraction digits, trailing zeros dropped. `None` for NaN and the
/// infinities, which select `other`.
pub fn operands(count: f64) -> Option<Operands> {
    if !count.is_finite() {
        return None;
    }
    let x = count.abs();
    if x == 0.0 {
        return Some(Operands { n: 0.0, i: 0.0, v: 0.0, f: 0.0 });
    }
    let (digits, exponent) = shortest_digits(x);
    let mut all: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
    // x = 0.<digits> × 10^(exponent + 1): place the digits on a grid with
    // at least one integer digit.
    let mut integer_len = exponent + 1;
    if integer_len <= 0 {
        let mut padded = vec![0u8; (1 - integer_len) as usize];
        padded.extend(all);
        all = padded;
        integer_len = 1;
    }
    if (all.len() as i32) < integer_len {
        all.resize(integer_len as usize, 0);
    }
    let keep = integer_len as usize + 3;
    if all.len() > keep {
        let round_up = all[keep] >= 5;
        all.truncate(keep);
        if round_up {
            let mut i = keep;
            loop {
                if i == 0 {
                    all.insert(0, 1);
                    integer_len += 1;
                    break;
                }
                i -= 1;
                if all[i] == 9 {
                    all[i] = 0;
                } else {
                    all[i] += 1;
                    break;
                }
            }
        }
    }
    while all.len() > integer_len as usize && all.last() == Some(&0) {
        all.pop();
    }
    let (integer, fraction) = all.split_at(integer_len as usize);
    let text = |ds: &[u8]| ds.iter().map(|d| char::from(b'0' + d)).collect::<String>();
    let n = if fraction.is_empty() {
        text(integer)
    } else {
        format!("{}.{}", text(integer), text(fraction))
    };
    let low = &integer[integer.len().saturating_sub(18)..];
    Some(Operands {
        n: n.parse().unwrap_or(0.0),
        i: text(low).parse().unwrap_or(0.0),
        v: fraction.len() as f64,
        f: if fraction.is_empty() { 0.0 } else { text(fraction).parse().unwrap_or(0.0) },
    })
}

fn operand(operands: &Operands, name: &str) -> Option<f64> {
    Some(match name {
        "n" => operands.n,
        "i" => operands.i,
        "v" | "w" => operands.v,
        "f" | "t" => operands.f,
        // No compact notation: the exponent is always zero.
        "e" | "c" => 0.0,
        _ => return None,
    })
}

/// One relation, `operand (% m)? (=|!=) ranges`, as ICU4C's
/// `AndConstraint::isFulfilled` evaluates it: a non-integer operand
/// matches no range.
fn relation(operands: &Operands, text: &str) -> bool {
    let (left, negated, right) = match text.split_once("!=") {
        Some((l, r)) => (l, true, r),
        None => match text.split_once('=') {
            Some((l, r)) => (l, false, r),
            None => return false,
        },
    };
    let (name, modulus) = match left.split_once('%') {
        Some((name, m)) => (name.trim(), m.trim().parse::<f64>().ok()),
        None => (left.trim(), None),
    };
    let Some(mut value) = operand(operands, name) else { return false };
    let mut result = false;
    if value == value.floor() {
        if let Some(m) = modulus {
            value %= m;
        }
        for range in right.trim().split(',') {
            let (low, high) = range.split_once("..").unwrap_or((range, range));
            if let (Ok(low), Ok(high)) = (low.trim().parse::<f64>(), high.trim().parse::<f64>())
                && low <= value
                && value <= high
            {
                result = true;
                break;
            }
        }
    }
    result != negated
}

/// A CLDR condition: `and` binds tighter than `or`; an empty one holds.
pub fn evaluate(operands: &Operands, condition: &str) -> bool {
    if condition.trim().is_empty() {
        return true;
    }
    condition
        .split(" or ")
        .any(|conjunction| conjunction.split(" and ").all(|r| relation(operands, r)))
}

/// CLDR plural category for a count, with a safe fallback when the
/// constructor refuses the locale.
pub fn plural_category(locale: &str, count: f64) -> &'static str {
    let Some(tag) = parse(locale) else {
        return if count == 1.0 { "one" } else { "other" };
    };
    let Some(rules) = rules_for(&resolve(&tag)) else { return "other" };
    let Some(operands) = operands(count) else { return "other" };
    rules
        .iter()
        .find(|(_, condition)| evaluate(&operands, condition))
        .map_or("other", |(category, _)| category)
}
