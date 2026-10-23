//! Two jobs.
//!
//! The faces: `packages/core/fonts/src/standard/` holds the 14 standard faces
//! as generated TypeScript (`faces.generated.ts` for the layout numbers,
//! `programs/<font>.generated.ts` for each OpenType program in base64). That
//! output of `scripts/generate-standard-fonts.mjs` is the one copy of the
//! data; this script reads it and writes `$OUT_DIR/faces.rs`, so the Rust and
//! the TypeScript can't drift apart. It reads the generator's exact layout and
//! panics on any line it doesn't expect, rather than guess.
//!
//! The rpath: re-emitted for this crate's own test binaries. The conformance
//! harness (`harness` feature) depends on `plugin-conformance`, whose fake
//! engine links libembedpdf through `epdf-engine`. Cargo does not propagate
//! `cargo:rustc-link-arg` to a dependent package's test targets, so without
//! this the tests link and then fail at load with
//! `Library not loaded: @rpath/libembedpdf.dylib`. Without the harness the
//! crate is pure and links nothing, so it needs no rpath.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// `STANDARD_FONTS`, in its order: the order of `faces.generated.ts` and of
/// `StandardFont::ALL`.
const FONTS: [&str; 14] = [
    "courier",
    "courier-bold",
    "courier-bold-oblique",
    "courier-oblique",
    "helvetica",
    "helvetica-bold",
    "helvetica-bold-oblique",
    "helvetica-oblique",
    "times-roman",
    "times-bold",
    "times-bold-italic",
    "times-italic",
    "symbol",
    "zapf-dingbats",
];

fn main() {
    if std::env::var_os("CARGO_FEATURE_HARNESS").is_some() {
        build_support::emit_rpath_only();
    }
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    // The nearest ancestor holding the package, as `build_support::lib_root`
    // finds `packages/engine/runtime`, so the crate's depth doesn't matter.
    let standard = manifest
        .ancestors()
        .map(|dir| dir.join("packages/core/fonts/src/standard"))
        .find(|dir| dir.is_dir())
        .unwrap_or_else(|| {
            panic!("no packages/core/fonts/src/standard above {}", manifest.display())
        });
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("faces.rs"), faces_rs(&standard)).expect("write faces.rs");
}

struct Face {
    name: String,
    ascent: i32,
    descent: i32,
    program_hash: String,
    /// `None` for `WIN_ANSI`.
    code_points: Option<Vec<u32>>,
    advances: Vec<u32>,
}

fn read(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn numbers<T: std::str::FromStr>(list: &str, what: &str) -> Vec<T> {
    let inner = list
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or_else(|| panic!("{what}: not an array literal"));
    inner
        .split(',')
        .map(|n| n.parse().unwrap_or_else(|_| panic!("{what}: not a number: {n:?}")))
        .collect()
}

fn member<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.strip_prefix("    ")?.strip_prefix(key)?.strip_prefix(": ")?.strip_suffix(',')
}

fn quoted(text: &str, what: &str) -> String {
    text.strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or_else(|| panic!("{what}: not a quoted string: {text:?}"))
        .to_string()
}

fn parse_faces(text: &str) -> (Vec<u32>, Vec<Face>) {
    let mut win_ansi = None;
    let mut faces: Vec<Face> = Vec::new();
    let mut in_faces = false;
    for line in text.lines() {
        if let Some(list) = line
            .strip_prefix("const WIN_ANSI: readonly number[] = ")
            .and_then(|rest| rest.strip_suffix(';'))
        {
            win_ansi = Some(numbers(list, "WIN_ANSI"));
            continue;
        }
        if line.starts_with("export const FACES") {
            in_faces = true;
            continue;
        }
        if !in_faces {
            continue;
        }
        if line == "};" {
            in_faces = false;
        } else if let Some(name) =
            line.strip_prefix("  '").and_then(|rest| rest.strip_suffix("': {"))
        {
            faces.push(Face {
                name: name.to_string(),
                ascent: i32::MIN,
                descent: i32::MIN,
                program_hash: String::new(),
                code_points: None,
                advances: Vec::new(),
            });
        } else if line == "  }," {
        } else {
            let face = faces.last_mut().unwrap_or_else(|| panic!("member outside a face: {line}"));
            let what = format!("{}: {line}", face.name);
            if let Some(n) = member(line, "ascent") {
                face.ascent = n.parse().unwrap_or_else(|_| panic!("{what}"));
            } else if let Some(n) = member(line, "descent") {
                face.descent = n.parse().unwrap_or_else(|_| panic!("{what}"));
            } else if let Some(hash) = member(line, "programHash") {
                face.program_hash = quoted(hash, &what);
            } else if let Some(list) = member(line, "codePoints") {
                face.code_points = (list != "WIN_ANSI").then(|| numbers(list, &what));
            } else if let Some(list) = member(line, "advances") {
                face.advances = numbers(list, &what);
            } else {
                panic!("faces.generated.ts: unexpected line: {line}");
            }
        }
    }
    (win_ansi.expect("faces.generated.ts: no WIN_ANSI"), faces)
}

fn program(standard: &Path, name: &str) -> String {
    let text = read(&standard.join(format!("programs/{name}.generated.ts")));
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("export default ")?.strip_suffix(';'))
        .unwrap_or_else(|| panic!("programs/{name}: no default export"));
    let base64 = quoted(line, name);
    let alphabet = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=');
    assert!(
        base64.len().is_multiple_of(4) && base64.bytes().all(alphabet),
        "programs/{name}: not padded base64"
    );
    base64
}

fn faces_rs(standard: &Path) -> String {
    let (win_ansi, faces) = parse_faces(&read(&standard.join("faces.generated.ts")));
    let names: Vec<&str> = faces.iter().map(|face| face.name.as_str()).collect();
    assert_eq!(names, FONTS, "faces.generated.ts: the faces or their order changed");
    let mut out = String::from(
        "// Generated by build.rs from packages/core/fonts/src/standard. Don't edit.\n\n",
    );
    writeln!(out, "const WIN_ANSI: &[u32] = &{win_ansi:?};\n").unwrap();
    out.push_str("pub(crate) static FACES: [FaceData; 14] = [\n");
    for face in &faces {
        let count = face.code_points.as_ref().map_or(win_ansi.len(), Vec::len);
        assert!(
            face.ascent != i32::MIN && face.descent != i32::MIN && face.program_hash.len() == 64,
            "{}: a member is missing",
            face.name
        );
        assert_eq!(face.advances.len(), count, "{}: advances and code points", face.name);
        let code_points = match &face.code_points {
            None => "WIN_ANSI".to_string(),
            Some(list) => format!("&{list:?}"),
        };
        writeln!(
            out,
            "    FaceData {{\n        ascent: {},\n        descent: {},\n        program_hash: {:?},\n        code_points: {code_points},\n        advances: &{:?},\n        program: {:?},\n    }},",
            face.ascent,
            face.descent,
            face.program_hash,
            face.advances,
            program(standard, &face.name),
        )
        .unwrap();
    }
    out.push_str("];\n");
    out
}
