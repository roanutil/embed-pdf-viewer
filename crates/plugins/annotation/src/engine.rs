//! The pure parts of `@embedpdf/engine-core/runtime` this plugin calls that
//! `epdf-annotation-core` doesn't carry: list positions
//! (`mutation/ListPosition.ts`), what a delete takes with it
//! (`annotation/relationships.ts`), colors (`annotation/color.ts`), the page
//! box of a file rect (`geometry/pageSpace.ts`) and the date order comments
//! sort by (`dto/IsoDateTime.ts`). The plugin links no engine, so they are
//! transcribed here; every value stays the engine's JSON.

use std::collections::{HashMap, HashSet};

use epdf_kernel::Error;
use serde_json::{Value, json};

use crate::js::{key_of, page_number};

// ── list positions ──────────────────────────────────────────────────────────

/// `anchorOf(position)`: the neighbour a position names, or `None` for
/// `'start'` and `'end'`.
pub fn anchor_of(position: &Value) -> Option<&Value> {
    match position {
        Value::String(_) => None,
        Value::Object(o) => o.get("before").or_else(|| o.get("after")),
        _ => None,
    }
}

fn engine_error(code: &str, message: String, field: &str) -> Error {
    Error::engine(code, message).with("details", json!({ "field": field }))
}

/// `positionIndex(order, position, keyOf)`.
fn position_index<T>(
    order: &[T],
    position: &Value,
    key: &dyn Fn(&T) -> String,
    key_value: &dyn Fn(&Value) -> String,
) -> Result<usize, Error> {
    match position.as_str() {
        Some("start") => return Ok(0),
        Some("end") => return Ok(order.len()),
        _ => {}
    }
    let anchor = anchor_of(position).cloned().unwrap_or(Value::Null);
    let wanted = key_value(&anchor);
    let Some(at) = order.iter().position(|item| key(item) == wanted) else {
        return Err(engine_error(
            "NotFound",
            format!("no neighbour {wanted} in the list"),
            "position",
        ));
    };
    Ok(if position.get("before").is_some() { at } else { at + 1 })
}

/// `reorderedList(order, moved, position, keyOf)` over string keys: the
/// order once `moved` go to `position`, together and in the order given.
pub fn reordered_list(
    order: &[String],
    moved: &[String],
    position: &Value,
) -> Result<Vec<String>, Error> {
    let moved_keys: HashSet<&String> = moved.iter().collect();
    if moved_keys.len() != moved.len() {
        return Err(engine_error("InvalidArg", "a row is named twice".into(), "refs"));
    }
    let present: HashSet<&String> = order.iter().collect();
    if let Some(missing) = moved.iter().find(|k| !present.contains(k)) {
        return Err(engine_error("NotFound", format!("no row {missing} in the list"), "refs"));
    }
    if let Some(anchor) = anchor_of(position).and_then(Value::as_str)
        && moved_keys.contains(&anchor.to_string())
    {
        return Err(engine_error(
            "InvalidArg",
            format!("the neighbour {anchor} is one of the rows that move"),
            "position",
        ));
    }
    let staying: Vec<String> = order.iter().filter(|k| !moved_keys.contains(k)).cloned().collect();
    let at = position_index(&staying, position, &|k: &String| k.clone(), &|v: &Value| {
        v.as_str().map(str::to_string).unwrap_or_else(|| "undefined".into())
    })?;
    let mut out = staying[..at].to_vec();
    out.extend(moved.iter().cloned());
    out.extend(staying[at..].iter().cloned());
    Ok(out)
}

/// `reorderPart(list, inPart, order, keyOf)` with items that are their own
/// keys: the items `in_part` picks take `order`'s order in their slots.
pub fn reorder_part(
    list: &[String],
    in_part: &dyn Fn(&str) -> bool,
    order: &[String],
) -> Vec<String> {
    let part: Vec<&String> = list.iter().filter(|k| in_part(k)).collect();
    let in_set: HashSet<&String> = part.iter().copied().collect();
    let mut named: Vec<String> = Vec::new();
    for key in order {
        if in_set.contains(key) {
            named.push(key.clone());
        }
    }
    let placed: HashSet<String> = named.iter().cloned().collect();
    let mut next = named;
    next.extend(part.iter().filter(|k| !placed.contains(k.as_str())).map(|k| (*k).clone()));
    let mut slot = 0;
    list.iter()
        .map(|item| {
            if in_part(item) {
                let out = next[slot].clone();
                slot += 1;
                out
            } else {
                item.clone()
            }
        })
        .collect()
}

// ── relationships ───────────────────────────────────────────────────────────

/// `deletedWith(annotations, ref)`: an annotation and everything deleted
/// with it, children before their parent and a popup before what it shows,
/// the annotation last. Indexes into `annotations`; empty when `ref` names
/// none of them.
pub fn deleted_with(annotations: &[&Value], reference: &Value) -> Vec<usize> {
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (i, a) in annotations.iter().enumerate() {
        if let Some(key) = a.get("ref").and_then(key_of) {
            by_key.insert(key, i);
        }
    }
    let Some(target) = key_of(reference).and_then(|k| by_key.get(&k).copied()) else {
        return Vec::new();
    };
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut popups: HashMap<usize, Vec<usize>> = HashMap::new();
    let lookup = |v: Option<&Value>| v.and_then(key_of).and_then(|k| by_key.get(&k).copied());
    for (i, a) in annotations.iter().enumerate() {
        let parent = a.get("reply").filter(|r| !r.is_null()).and_then(|r| lookup(r.get("to")));
        if let Some(p) = parent
            && p != i
        {
            children.entry(p).or_default().push(i);
        }
        let is_popup = a.get("subtype").and_then(Value::as_str) == Some("popup");
        let shows = if is_popup { lookup(a.get("parent").filter(|p| !p.is_null())) } else { None };
        if let Some(s) = shows
            && s != i
        {
            popups.entry(s).or_default().push(i);
        }
        let own = lookup(a.get("popup").filter(|p| !p.is_null()));
        if let Some(o) = own
            && o != i
            && annotations[o].get("subtype").and_then(Value::as_str) == Some("popup")
        {
            popups.entry(i).or_default().push(o);
        }
    }
    let mut members = Vec::new();
    let mut seen = HashSet::new();
    fn visit(
        at: usize,
        children: &HashMap<usize, Vec<usize>>,
        popups: &HashMap<usize, Vec<usize>>,
        seen: &mut HashSet<usize>,
        members: &mut Vec<usize>,
    ) {
        if !seen.insert(at) {
            return;
        }
        for child in children.get(&at).into_iter().flatten() {
            visit(*child, children, popups, seen, members);
        }
        for popup in popups.get(&at).into_iter().flatten() {
            if !seen.insert(*popup) {
                continue;
            }
            members.push(*popup);
        }
        members.push(at);
    }
    visit(target, &children, &popups, &mut seen, &mut members);
    members
}

// ── colors ──────────────────────────────────────────────────────────────────

fn hex2(component: f64) -> String {
    let c = epdf_annotation_core::js::round(component).clamp(0.0, 255.0);
    let c = if c.is_nan() { 0.0 } else { c };
    format!("{:02x}", c as u32)
}

/// `colorOf(r, g, b)`: `'#rrggbb'` from 0..255 components.
pub fn color_of(r: f64, g: f64, b: f64) -> String {
    format!("#{}{}{}", hex2(r), hex2(g), hex2(b))
}

/// `rgbOf(color)`: the components of `'#rrggbb'`. `Number.parseInt` reads
/// the longest hex prefix; nothing hex reads as `NaN`, whose bits are 0.
pub fn rgb_of(color: &str) -> (f64, f64, f64) {
    let digits: String = color.chars().skip(1).take_while(char::is_ascii_hexdigit).collect();
    let value = if digits.is_empty() {
        0
    } else {
        // `>>` and `&` work on the low 32 bits of the number.
        let parsed =
            digits.chars().fold(0f64, |acc, c| acc * 16.0 + c.to_digit(16).unwrap() as f64);
        (parsed % 4_294_967_296.0) as u64 as u32 as i32
    };
    (((value >> 16) & 0xff) as f64, ((value >> 8) & 0xff) as f64, (value & 0xff) as f64)
}

// ── page space ──────────────────────────────────────────────────────────────

/// `pageBoxOf(rect, visible)`: a normalized file rect in page space.
pub fn page_box_of(left: f64, bottom: f64, right: f64, top: f64, crop: &Value) -> Value {
    let n = |k: &str| crop.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    json!({
        "x": num(left - n("left")),
        "y": num(n("top") - top),
        "width": num(right - left),
        "height": num(top - bottom),
    })
}

fn num(x: f64) -> Value {
    epdf_annotation_core::json::num(x)
}

/// `toPageRef(pageObjectNumber)`.
pub fn to_page_ref(page_object_number: f64) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": num(page_object_number) })
}

/// `pageRefsEqual(a, b)`: the same page object number.
pub fn page_refs_equal(a: &Value, b: &Value) -> bool {
    match (page_number(a), page_number(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

// ── dates ───────────────────────────────────────────────────────────────────

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `Date.parse` over the ISO 8601 forms an `IsoDateTime` takes
/// (`YYYY-MM-DD`, and with `THH:mm`, seconds, a fraction and `Z` or an
/// offset); anything else is `NaN`. A date-time without an offset reads as
/// UTC, as it does where the vectors were generated.
pub fn date_parse(text: &str) -> f64 {
    fn digits(s: &str, n: usize) -> Option<(i64, &str)> {
        if s.len() < n || !s.as_bytes()[..n].iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some((s[..n].parse().ok()?, &s[n..]))
    }
    let parse = || -> Option<f64> {
        let (year, rest) = digits(text, 4)?;
        let (month, day, mut rest) = match rest.strip_prefix('-') {
            Some(r) => {
                let (month, r) = digits(r, 2)?;
                match r.strip_prefix('-') {
                    Some(r) => {
                        let (day, r) = digits(r, 2)?;
                        (month, day, r)
                    }
                    None => (month, 1, r),
                }
            }
            None => (1, 1, rest),
        };
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let (mut hours, mut minutes, mut seconds, mut millis, mut offset) = (0, 0, 0, 0f64, 0i64);
        if let Some(r) = rest.strip_prefix('T') {
            let (h, r) = digits(r, 2)?;
            let r = r.strip_prefix(':')?;
            let (m, r) = digits(r, 2)?;
            hours = h;
            minutes = m;
            rest = r;
            if let Some(r) = rest.strip_prefix(':') {
                let (s, r) = digits(r, 2)?;
                seconds = s;
                rest = r;
                if let Some(r) = rest.strip_prefix('.') {
                    let len = r.bytes().take_while(u8::is_ascii_digit).count();
                    if len == 0 {
                        return None;
                    }
                    let frac = &r[..len];
                    millis = format!("0.{frac}").parse::<f64>().ok()? * 1000.0;
                    millis = millis.trunc();
                    rest = &r[len..];
                }
            }
            if let Some(r) = rest.strip_prefix('Z') {
                rest = r;
            } else if rest.starts_with('+') || rest.starts_with('-') {
                let sign = if rest.starts_with('-') { -1 } else { 1 };
                let (oh, r) = digits(&rest[1..], 2)?;
                let r = r.strip_prefix(':')?;
                let (om, r) = digits(r, 2)?;
                offset = sign * (oh * 60 + om);
                rest = r;
            }
            if hours > 24 || minutes > 59 || seconds > 59 {
                return None;
            }
        }
        if !rest.is_empty() {
            return None;
        }
        let days = days_from_civil(year, month, day);
        let ms = ((days * 24 + hours) * 60 + minutes - offset) * 60_000 + seconds * 1000;
        Some(ms as f64 + millis)
    };
    parse().unwrap_or(f64::NAN)
}

/// `compareIsoDateTime(a, b)`: `Date.parse(a) - Date.parse(b)`, 0 when either is unreadable.
pub fn compare_iso_date_time(a: &str, b: &str) -> f64 {
    let difference = date_parse(a) - date_parse(b);
    if difference.is_nan() { 0.0 } else { difference }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_parse_reads_iso_forms() {
        assert_eq!(date_parse("1970-01-01T00:00:00Z"), 0.0);
        assert_eq!(date_parse("1970-01-02"), 86_400_000.0);
        assert_eq!(date_parse("1970-01-01T01:00:00+01:00"), 0.0);
        assert_eq!(date_parse("1970-01-01T00:00:00.5Z"), 500.0);
        assert!(date_parse("yesterday").is_nan());
    }

    #[test]
    fn reorder_part_keeps_other_items_in_place() {
        let list: Vec<String> = ["a", "x", "b", "c"].iter().map(|s| s.to_string()).collect();
        let order: Vec<String> = ["c", "a"].iter().map(|s| s.to_string()).collect();
        let out = reorder_part(&list, &|k| k != "x", &order);
        assert_eq!(out, vec!["c", "x", "a", "b"]);
    }

    #[test]
    fn rgb_of_reads_hex() {
        assert_eq!(rgb_of("#ff8000"), (255.0, 128.0, 0.0));
        assert_eq!(color_of(255.0, 128.0, 0.0), "#ff8000");
    }
}
