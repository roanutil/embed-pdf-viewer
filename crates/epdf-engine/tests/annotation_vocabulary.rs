//! G7's pure gate: the Rust annotation vocabulary decodes what TypeScript does.
//!
//! Every codec under `features::annotation` is a transcription of a table in
//! `services/src/features/annotations/internal/`. A transcription is exactly
//! the kind of work that is easy to do almost right, and almost right here is
//! invisible: a line ending one code off still draws an arrow, a blend mode
//! one code off still composites, a `/State` that fails to normalize still
//! round-trips through its own reader. Nothing throws and nothing looks wrong
//! until a document written by one implementation is read by the other.
//!
//! So the tables are pinned to what the shipping TypeScript returns, over a
//! code range that runs past both ends of every enum. Regenerate, deliberately,
//! with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation

use epdf_engine::features::annotation::flags::{ANNOT_FLAG_BITS, AnnotationFlags};
use epdf_engine::features::annotation::intent::{
    CaretIntent, FreeTextIntent, InkIntent, StrikeoutIntent,
};
use epdf_engine::features::annotation::state::{
    state_from_pdf, state_model_from_pdf, state_model_to_pdf, state_to_pdf,
};
use epdf_engine::features::annotation::subtype::AnnotationSubtype;
use epdf_engine::features::annotation::vocabulary::{
    AnnotationBorderStyle, AnnotationColorType, AnnotationReplyType, BlendMode, FileAttachmentIcon,
    LineEnding, NoteIcon, STAMP_NAMES, StandardFont, TextAlignment, is_standard_stamp_name,
};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Deserialize)]
struct VocabularyFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: String,
    #[serde(rename = "codeRange")]
    code_range: CodeRange,
    #[serde(rename = "colorType")]
    color_type: std::collections::BTreeMap<String, i32>,
    #[serde(rename = "intentProbes")]
    intent_probes: Vec<Option<String>>,
    #[serde(rename = "nameProbes")]
    name_probes: Vec<String>,
    #[serde(rename = "flagBits")]
    flag_bits: std::collections::BTreeMap<String, u32>,
    codecs: Vec<Codec>,
    intents: Vec<Intent>,
    names: Vec<NameMap>,
    states: Vec<StateCase>,
    flags: Vec<FlagCase>,
}

#[derive(Debug, Deserialize)]
struct CodeRange {
    from: i32,
    to: i32,
}

#[derive(Debug, Deserialize)]
struct Codec {
    name: String,
    /// What the codec reads, for a human opening the vector file. Unused here.
    #[allow(dead_code)]
    of: String,
    decoded: Vec<Option<String>>,
    encoded: Vec<(String, i32)>,
}

#[derive(Debug, Deserialize)]
struct Intent {
    name: String,
    decoded: Vec<Option<String>>,
    encoded: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct NameMap {
    name: String,
    /// What the map reads, for a human opening the vector file. Unused here.
    #[allow(dead_code)]
    of: String,
    decoded: Vec<Option<String>>,
    encoded: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct StateCase {
    probe: String,
    #[serde(rename = "stateToPdf")]
    state_to_pdf: String,
    #[serde(rename = "stateFromPdf")]
    state_from_pdf: String,
    #[serde(rename = "stateModelToPdf")]
    state_model_to_pdf: String,
    #[serde(rename = "stateModelFromPdf")]
    state_model_from_pdf: String,
}

#[derive(Debug, Deserialize)]
struct FlagCase {
    bits: u32,
    flags: serde_json::Value,
}

fn load() -> VocabularyFile {
    let path = test_support::corpus::vectors_dir("annotation").join("vocabulary.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VocabularyFile = serde_json::from_str(&text).expect("parse vocabulary.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vocabulary.json envelope changed shape");
    assert_eq!(file.kind, "annotation.vocabulary");
    file
}

fn codec<'a>(file: &'a VocabularyFile, name: &str) -> &'a Codec {
    file.codecs
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("vocabulary.json has no codec {name}"))
}

/// Runs one codec's sweep. `decode` answers `None` where the TypeScript map
/// has no entry, which is not the same as a fallback: the `Option` codecs
/// leave the default to their reader.
fn check_sweep(file: &VocabularyFile, name: &str, decode: impl Fn(i32) -> Option<&'static str>) {
    let entry = codec(file, name);
    let expected_len = (file.code_range.to - file.code_range.from + 1) as usize;
    assert_eq!(entry.decoded.len(), expected_len, "{name} sweep is not the declared range");
    for (offset, expected) in entry.decoded.iter().enumerate() {
        let code = file.code_range.from + offset as i32;
        assert_eq!(
            decode(code),
            expected.as_deref(),
            "{name} decoded code {code} differently from TypeScript",
        );
    }
}

fn check_encode(file: &VocabularyFile, name: &str, encode: impl Fn(&str) -> Option<i32>) {
    let entry = codec(file, name);
    assert!(!entry.encoded.is_empty(), "{name} has no encode table");
    for (wire, expected) in &entry.encoded {
        assert_eq!(
            encode(wire),
            Some(*expected),
            "{name} encoded {wire} differently from TypeScript",
        );
    }
}

#[test]
fn the_integer_codecs_decode_what_typescript_does_across_the_whole_range() {
    let file = load();

    check_sweep(&file, "subtype", |code| Some(AnnotationSubtype::from_code(code).as_str()));
    check_sweep(&file, "blendMode", |code| Some(BlendMode::from_code(code).as_str()));
    check_sweep(&file, "lineEnding", |code| Some(LineEnding::from_code(code).as_str()));
    check_sweep(&file, "borderStyle", |code| Some(AnnotationBorderStyle::from_code(code).as_str()));
    check_sweep(&file, "standardFont", |code| Some(StandardFont::from_code(code).as_str()));
    check_sweep(&file, "textAlignment", |code| Some(TextAlignment::from_code(code).as_str()));
    check_sweep(&file, "replyType", |code| Some(AnnotationReplyType::from_code(code).as_str()));
}

#[test]
fn the_integer_codecs_encode_what_typescript_does() {
    let file = load();

    check_encode(&file, "subtype", |wire| {
        epdf_engine::features::annotation::subtype::ANNOTATION_SUBTYPES
            .into_iter()
            .find(|s| s.as_str() == wire)
            .map(AnnotationSubtype::code)
    });
    check_encode(&file, "blendMode", |wire| {
        BlendMode::ALL.into_iter().find(|m| m.as_str() == wire).map(BlendMode::code)
    });
    check_encode(&file, "lineEnding", |wire| {
        LineEnding::ALL.into_iter().find(|e| e.as_str() == wire).map(LineEnding::code)
    });
    check_encode(&file, "borderStyle", |wire| {
        AnnotationBorderStyle::ALL
            .into_iter()
            .find(|s| s.as_str() == wire)
            .map(AnnotationBorderStyle::code)
    });
    check_encode(&file, "standardFont", |wire| {
        StandardFont::from_family(wire).map(StandardFont::code)
    });
    check_encode(&file, "textAlignment", |wire| {
        TextAlignment::ALL.into_iter().find(|a| a.as_str() == wire).map(TextAlignment::code)
    });
    check_encode(&file, "replyType", |wire| {
        AnnotationReplyType::ALL
            .into_iter()
            .find(|r| r.as_str() == wire)
            .map(AnnotationReplyType::code)
    });
}

#[test]
fn the_it_names_decode_what_typescript_does() {
    let file = load();
    let probes: Vec<Option<&str>> = file.intent_probes.iter().map(Option::as_deref).collect();

    for intent in &file.intents {
        let decoded: Vec<Option<&str>> = probes
            .iter()
            .map(|probe| match intent.name.as_str() {
                "freeTextIntent" => Some(FreeTextIntent::from_name(*probe).as_str()),
                "inkIntent" => InkIntent::from_name(*probe).map(InkIntent::as_str),
                "caretIntent" => CaretIntent::from_name(*probe).map(CaretIntent::as_str),
                "strikeoutIntent" => {
                    StrikeoutIntent::from_name(*probe).map(StrikeoutIntent::as_str)
                }
                other => panic!("vocabulary.json has an intent {other} with no Rust codec"),
            })
            .collect();
        let expected: Vec<Option<&str>> = intent.decoded.iter().map(Option::as_deref).collect();
        assert_eq!(decoded, expected, "{} decoded differently from TypeScript", intent.name);
    }
}

#[test]
fn the_it_names_encode_what_typescript_does() {
    let file = load();

    for intent in &file.intents {
        for (wire, expected) in &intent.encoded {
            let name = match intent.name.as_str() {
                "freeTextIntent" => FreeTextIntent::ALL
                    .into_iter()
                    .find(|i| i.as_str() == wire)
                    .map(FreeTextIntent::name),
                "inkIntent" => {
                    InkIntent::ALL.into_iter().find(|i| i.as_str() == wire).map(InkIntent::name)
                }
                "caretIntent" => {
                    CaretIntent::ALL.into_iter().find(|i| i.as_str() == wire).map(CaretIntent::name)
                }
                "strikeoutIntent" => StrikeoutIntent::ALL
                    .into_iter()
                    .find(|i| i.as_str() == wire)
                    .map(StrikeoutIntent::name),
                other => panic!("vocabulary.json has an intent {other} with no Rust codec"),
            };
            assert_eq!(
                name,
                Some(expected.as_str()),
                "{} encoded {wire} differently from TypeScript",
                intent.name,
            );
        }
    }
}

#[test]
fn the_state_spellings_agree_in_both_directions() {
    let file = load();
    assert!(!file.states.is_empty());

    for case in &file.states {
        let probe = &case.probe;
        assert_eq!(state_to_pdf(probe), case.state_to_pdf, "stateToPdf({probe:?})");
        assert_eq!(state_from_pdf(probe), case.state_from_pdf, "stateFromPdf({probe:?})");
        assert_eq!(
            state_model_to_pdf(probe),
            case.state_model_to_pdf,
            "stateModelToPdf({probe:?})",
        );
        assert_eq!(
            state_model_from_pdf(probe),
            case.state_model_from_pdf,
            "stateModelFromPdf({probe:?})",
        );
    }
}

#[test]
fn the_f_word_expands_to_the_same_ten_booleans() {
    let file = load();
    assert!(!file.flags.is_empty());

    for case in &file.flags {
        let flags = serde_json::to_value(AnnotationFlags::from_bits(case.bits)).unwrap();
        assert_eq!(flags, case.flags, "bits {:#x}", case.bits);
    }
}

#[test]
fn the_bit_positions_and_the_color_selector_are_the_same_numbers() {
    let file = load();

    // The flag table, checked as a table rather than through `from_bits`,
    // because `from_bits` could be self-consistently wrong: a reader and a
    // writer that both use position 4 for `noZoom` agree with each other.
    assert_eq!(file.flag_bits.len(), ANNOT_FLAG_BITS.len());
    for (name, bit) in ANNOT_FLAG_BITS {
        assert_eq!(file.flag_bits.get(name), Some(&bit), "flag {name}");
    }

    // `FPDFANNOT_COLORTYPE` never reaches the wire, so nothing downstream
    // would catch a wrong selector: reading `/IC` where `/C` was meant gives
    // a plausible color, not an error.
    assert_eq!(file.color_type.len(), 4);
    for (name, value) in [
        ("Color", AnnotationColorType::Color),
        ("InteriorColor", AnnotationColorType::InteriorColor),
        ("OverlayColor", AnnotationColorType::OverlayColor),
        ("TextColor", AnnotationColorType::TextColor),
    ] {
        assert_eq!(file.color_type.get(name), Some(&(value as i32)), "color type {name}");
    }
}

fn name_map<'a>(file: &'a VocabularyFile, name: &str) -> &'a NameMap {
    file.names
        .iter()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("vocabulary.json has no name map {name}"))
}

/// `decode` answers `None` where the TypeScript map has no entry, leaving the
/// default to the reader, exactly as the integer codecs do.
fn check_names(
    file: &VocabularyFile,
    name: &str,
    decode: impl Fn(&str) -> Option<&'static str>,
    encode: impl Fn(&str) -> Option<&'static str>,
) {
    let entry = name_map(file, name);
    assert_eq!(entry.decoded.len(), file.name_probes.len(), "{name} is not one entry per probe");
    for (probe, expected) in file.name_probes.iter().zip(&entry.decoded) {
        assert_eq!(
            decode(probe),
            expected.as_deref(),
            "{name} decoded /Name {probe:?} differently from TypeScript",
        );
    }
    assert!(!entry.encoded.is_empty(), "{name} has no encode table");
    for (wire, expected) in &entry.encoded {
        assert_eq!(
            encode(wire),
            Some(expected.as_str()),
            "{name} encoded {wire} differently from TypeScript",
        );
    }
}

#[test]
fn the_name_maps_decode_and_encode_what_typescript_does() {
    let file = load();

    check_names(
        &file,
        "noteIcon",
        |pdf| NoteIcon::from_pdf_name(pdf).map(NoteIcon::as_str),
        |wire| NoteIcon::ALL.into_iter().find(|i| i.as_str() == wire).map(NoteIcon::pdf_name),
    );
    check_names(
        &file,
        "fileAttachmentIcon",
        |pdf| FileAttachmentIcon::from_pdf_name(pdf).map(FileAttachmentIcon::as_str),
        |wire| {
            FileAttachmentIcon::ALL
                .into_iter()
                .find(|i| i.as_str() == wire)
                .map(FileAttachmentIcon::pdf_name)
        },
    );
    // A stamp label is written verbatim, so the encode side is the identity
    // over the standard set, and the decode side is membership.
    let standard = |label: &str| STAMP_NAMES.into_iter().find(|n| *n == label);
    check_names(
        &file,
        "stampName",
        |pdf| standard(pdf).filter(|n| is_standard_stamp_name(n)),
        standard,
    );
}
