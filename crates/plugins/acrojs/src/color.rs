//! `color.ts`: host-side Acrobat color-array conversions. The prelude inlines
//! the same formulas; the commit sinks and world builders cross the
//! array-to-renderer boundary through these.
//!
//! G to RGB replicates the gray; CMYK to RGB is `1 - min(1, channel + k)`;
//! transparent converts to itself only.

use serde::Serialize;
use serde_json::Value;

use crate::js;
use crate::types::ScriptColorArray;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

/// Acrobat array to RGB components 0..1; `None` for transparent.
pub fn script_color_to_rgb(color: &ScriptColorArray) -> Option<Rgb> {
    match *color {
        ScriptColorArray::T => None,
        ScriptColorArray::G(g) => Some(Rgb { r: g, g, b: g }),
        ScriptColorArray::Rgb(r, g, b) => Some(Rgb { r, g, b }),
        ScriptColorArray::Cmyk(c, m, y, k) => Some(Rgb {
            r: 1.0 - js::min(1.0, c + k),
            g: 1.0 - js::min(1.0, m + k),
            b: 1.0 - js::min(1.0, y + k),
        }),
    }
}

fn clamp01(value: f64) -> f64 {
    js::min(1.0, js::max(0.0, value))
}

/// `Math.round(clamp01(value) * 255).toString(16).padStart(2, '0')`.
fn hex(value: f64) -> String {
    let byte = js::round(clamp01(value) * 255.0);
    let text = if byte.is_nan() { "NaN".to_string() } else { format!("{:x}", byte as u32) };
    format!("{text:0>2}")
}

/// Acrobat array to CSS hex, or `None` for transparent.
pub fn script_color_to_css(color: &ScriptColorArray) -> Option<String> {
    script_color_to_rgb(color).map(|rgb| format!("#{}{}{}", hex(rgb.r), hex(rgb.g), hex(rgb.b)))
}

/// CSS hex (`#rgb`/`#rrggbb`) to an Acrobat RGB array; empty, absent or
/// unparseable is transparent.
pub fn css_to_script_color(css: Option<&str>) -> ScriptColorArray {
    let Some(css) = css.filter(|c| !c.is_empty()) else { return ScriptColorArray::T };
    let trimmed = js::trim(css);
    let raw = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let units: Vec<u16> = raw.encode_utf16().collect();
    // `raw.split('')` splits UTF-16 code units, each doubled.
    let long: Vec<u16> =
        if units.len() == 3 { units.iter().flat_map(|u| [*u, *u]).collect() } else { units };
    let is_hex = |u: &u16| char::from_u32(u32::from(*u)).is_some_and(|c| c.is_ascii_hexdigit());
    if long.len() != 6 || !long.iter().all(is_hex) {
        return ScriptColorArray::T;
    }
    let text = String::from_utf16_lossy(&long);
    let channel = |offset: usize| {
        f64::from(u8::from_str_radix(&text[offset..offset + 2], 16).unwrap_or(0)) / 255.0
    };
    ScriptColorArray::Rgb(channel(0), channel(2), channel(4))
}

/// Structural validation for a value a script assigns. The space is coerced
/// with `String()` before the lookup, as the TypeScript does, and a part must
/// be a finite number (`{"$number": ...}` in canonical JSON is not).
pub fn is_script_color_array(value: &Value) -> bool {
    let Some(items) = value.as_array() else { return false };
    let Some((space, parts)) = items.split_first() else { return false };
    let expected = match js::string_of(space).as_str() {
        "T" => 0,
        "G" => 1,
        "RGB" => 3,
        "CMYK" => 4,
        _ => return false,
    };
    parts.len() == expected && parts.iter().all(|part| part.as_f64().is_some_and(f64::is_finite))
}
