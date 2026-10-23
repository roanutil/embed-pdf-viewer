//! The conformance adapter: the function table `vectors/core/fonts.json`
//! calls by name, with arguments and results in canonical JSON.
//!
//! The names are the ones `tooling/plugin-conformance/src/suites/fonts.ts`
//! records. Three are the suite's wrappers: `standardFontMetrics` asks
//! `advance` for each code point the case passes, `standardFontProgram`
//! records the program's data members, and `standardFontProgramLoad` returns
//! the loaded bytes.
//!
//! The Rust API takes a [`StandardFont`], so an unknown name can't reach it.
//! The TypeScript takes a string its type restricts, and an unknown one throws
//! at the first member it reads off the missing face: `codePoints` in
//! `standardFontMetrics`, `programHash` in everything that builds a program.
//! The table parses the name and answers a miss with that `TypeError`.

use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use plugin_conformance::FunctionTable;
use plugin_conformance::js::{arg, to_value};
use serde::Serialize;
use serde_json::Value;

use crate::base64::decode_base64;
use crate::*;

type Out = Result<Value, Error>;

fn ok<T: Serialize>(value: T) -> Out {
    Ok(to_value(&value))
}

/// The font a case names, or the `TypeError` the TypeScript throws reading
/// `member` off the face it doesn't find.
fn font(args: &[Value], member: &str) -> Result<StandardFont, Error> {
    let name: String = arg(args, 0)?;
    StandardFont::from_name(&name).ok_or_else(|| {
        Error::named(
            "TypeError",
            format!("Cannot read properties of undefined (reading '{member}')"),
        )
    })
}

/// A JavaScript number as a `Map` key among code points: only a non-negative
/// integer can equal one (`-0` equals `0`).
fn code_point(number: f64) -> Option<u32> {
    (number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&number))
        .then_some(number as u32)
}

#[derive(Serialize)]
struct MetricsProbe {
    font: StandardFont,
    ascent: i32,
    descent: i32,
    advances: Vec<Option<u32>>,
}

fn metrics_probe(a: &[Value]) -> Out {
    let metrics = standard_font_metrics(font(a, "codePoints")?);
    let code_points: Vec<f64> = arg(a, 1)?;
    ok(MetricsProbe {
        font: metrics.font,
        ascent: metrics.ascent,
        descent: metrics.descent,
        advances: code_points
            .into_iter()
            .map(|number| code_point(number).and_then(|cp| metrics.advance(cp)))
            .collect(),
    })
}

pub fn functions() -> FunctionTable {
    vec![
        ("standardFontMetrics", metrics_probe),
        ("standardFontProgram", |a| ok(standard_font_program(font(a, "programHash")?))),
        ("standardFontProgramLoad", |a| {
            Ok(Bytes(standard_font_program(font(a, "programHash")?).load().to_vec()).to_json())
        }),
        ("standardFontCss", |a| ok(standard_font_css(font(a, "programHash")?))),
        ("fontProgramFamily", |a| ok(font_program_family(&arg::<String>(a, 0)?))),
        ("decodeBase64", |a| Ok(Bytes(decode_base64(&arg::<String>(a, 0)?)).to_json())),
    ]
}
