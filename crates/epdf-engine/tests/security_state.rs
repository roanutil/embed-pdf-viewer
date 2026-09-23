//! G3: the Rust security derivation agrees with the TypeScript one.
//!
//! The derivation half of G3's "open, wrong password, permission, close,
//! cancel traces" gate. Pure, so it needs no document and no fixture per
//! combination of bits; the half that reads a real encrypted file is separate.
//!
//! The vectors under `tooling/engine-conformance/vectors/security/` are what
//! `buildSecurityInfo`, `openedAsFromCode`, `advisoryFromPdfBits` and
//! `securityStateFromProbe` produce. Regenerate them, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run security

use epdf_engine::auth::scope::{DocCapability, ParsedScope, check_capability, decode_pdf_bits};
use epdf_engine::features::security::state::{
    ALL_STANDARD_SECURITY_PERMISSIONS, OpenedAs, PermissionAdvisory, SecurityProbeInfo,
    SecurityState, StateOptions, advisory_from_pdf_bits, build_security_info,
    has_all_standard_security_permissions, normalize_u32, password_blocked,
    security_state_from_probe, unknown_security, unsupported_security,
};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    probed_at: i64,
    all_standard_security_permissions: u32,
    build: Vec<BuildCase>,
    all_allowed: Vec<AllAllowedCase>,
    opened_as_from_code: Vec<OpenedAsCase>,
    advisory: Vec<AdvisoryCase>,
    state: Vec<StateCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildCase {
    name: String,
    covers: String,
    opened_as: OpenedAs,
    permissions_bits: u32,
    security_handler_revision: Option<i32>,
    all_allowed: bool,
    info: SecurityProbeInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AllAllowedCase {
    name: String,
    covers: String,
    raw: i64,
    normalized: u32,
    all_allowed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenedAsCase {
    name: String,
    code: i32,
    opened_as: OpenedAs,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdvisoryCase {
    name: String,
    covers: String,
    raw: Option<i64>,
    advisory: PermissionAdvisory,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateCase {
    name: String,
    covers: String,
    probe: String,
    #[serde(default)]
    access_endpoint: Option<String>,
    #[serde(default)]
    cdn_required: bool,
    probe_info: SecurityProbeInfo,
    state: SecurityState,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("security").join("state.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("state.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "security.state");
    file
}

#[test]
fn the_all_allowed_mask_is_the_same_constant() {
    // 0xFFFFFFFC and not 0xFFFFFFFF. Comparing against the latter reports
    // every owner-unlocked document as restricted, and nothing downstream
    // would notice, so the constant is pinned rather than the behaviour alone.
    let file = load();
    assert_eq!(ALL_STANDARD_SECURITY_PERMISSIONS, file.all_standard_security_permissions);
}

#[test]
fn the_permission_word_normalizes_the_same_way_in_both_signs() {
    // The one place the two languages genuinely differ in type rather than in
    // rule. JavaScript computes permission words as signed 32-bit ints and
    // launders them through `u32()` at each boundary; Rust takes `u32` and
    // converts once at the FFI edge. Both spellings of each word are in the
    // corpus so the conversion is pinned rather than assumed to be a no-op.
    let file = load();
    assert!(file.all_allowed.len() >= 8, "the corpus shrank");
    for case in &file.all_allowed {
        assert_eq!(
            normalize_u32(case.raw),
            case.normalized,
            "{}: {} normalized ({})",
            case.name,
            case.raw,
            case.covers,
        );
        assert_eq!(
            has_all_standard_security_permissions(normalize_u32(case.raw)),
            case.all_allowed,
            "{}: {}",
            case.name,
            case.covers,
        );
    }
}

#[test]
fn building_a_probe_agrees_with_typescript() {
    let file = load();
    assert!(file.build.len() >= 10, "the corpus shrank");
    for case in &file.build {
        assert_eq!(
            has_all_standard_security_permissions(case.permissions_bits),
            case.all_allowed,
            "{}: all-allowed for {:#010x} ({})",
            case.name,
            case.permissions_bits,
            case.covers,
        );
        let actual = build_security_info(
            case.opened_as,
            case.permissions_bits,
            case.security_handler_revision,
            file.probed_at,
        );
        assert_eq!(actual, case.info, "{}: {}", case.name, case.covers);
    }
}

#[test]
fn the_unbuilt_probe_shapes_agree_with_typescript() {
    // password-blocked, unsupported and unknown are not built by
    // `buildSecurityInfo`; only a cold probe produces them. They reach the
    // vectors through the state cases, which is where they are checked.
    let file = load();
    let mut seen = 0;
    for case in &file.state {
        let actual = match case.probe.as_str() {
            "password-blocked" => password_blocked(file.probed_at),
            "unsupported" => unsupported_security(file.probed_at),
            "unknown" => unknown_security(file.probed_at),
            _ => continue,
        };
        assert_eq!(actual, case.probe_info, "{}: {}", case.name, case.covers);
        seen += 1;
    }
    assert!(seen >= 3, "only {seen} unbuilt shapes were checked");
}

#[test]
fn opened_as_from_code_agrees_with_typescript() {
    let file = load();
    for case in &file.opened_as_from_code {
        assert_eq!(
            OpenedAs::from_code(case.code),
            case.opened_as,
            "{}: code {}",
            case.name,
            case.code,
        );
    }
}

#[test]
fn the_advisory_agrees_with_typescript() {
    let file = load();
    for case in &file.advisory {
        let bits = decode_pdf_bits(case.raw);
        assert_eq!(advisory_from_pdf_bits(bits), case.advisory, "{}: {}", case.name, case.covers,);
    }
}

#[test]
fn the_advisory_and_the_scope_resolver_tell_the_same_story() {
    // `document-security-state.ts` says the advisory "MUST mirror the bit
    // combinations the scope resolver uses for pdf.permissions expansion". A
    // comment cannot enforce that, and the two live in different packages on
    // both sides, so the pairing is checked here: an advisory that says "you
    // can print in high quality" where the resolver denies `doc.print.high`
    // is a UI that lies about the document.
    //
    // Only the capabilities that have a bit behind them are paired. The
    // advisory has no field for the cloud-only ones, which is correct: there
    // is nothing about them for a badge to advise.
    let file = load();
    let virtual_scope = [ParsedScope::Virtual];
    for case in &file.advisory {
        let bits = decode_pdf_bits(case.raw);
        let advisory = advisory_from_pdf_bits(bits);
        let granted = |c: DocCapability| check_capability(c, &virtual_scope, bits);

        let pairs = [
            ("canPrint", advisory.can_print, granted(DocCapability::DocPrint)),
            ("canPrintHigh", advisory.can_print_high, granted(DocCapability::DocPrintHigh)),
            ("canCopy", advisory.can_copy, granted(DocCapability::DocTextCopy)),
            ("canAnnotate", advisory.can_annotate, granted(DocCapability::DocAnnotateModify)),
            ("canFillForms", advisory.can_fill_forms, granted(DocCapability::DocFormsFill)),
            ("canModifyForms", advisory.can_modify_forms, granted(DocCapability::DocFormsModify)),
            ("canModifyPages", advisory.can_modify_pages, granted(DocCapability::DocPagesModify)),
            ("canAssemble", advisory.can_assemble, granted(DocCapability::DocPagesAssemble)),
        ];
        for (field, advised, resolved) in pairs {
            assert_eq!(
                advised, resolved,
                "{}: advisory {field} says {advised}, the resolver says {resolved} ({})",
                case.name, case.covers,
            );
        }
    }
}

#[test]
fn folding_a_probe_into_state_agrees_with_typescript() {
    let file = load();
    assert!(file.state.len() >= 10, "the corpus shrank");
    for case in &file.state {
        let opts = StateOptions {
            access_endpoint: case.access_endpoint.clone(),
            cdn_required: case.cdn_required,
        };
        let actual = security_state_from_probe(&case.probe_info, &opts);
        assert_eq!(actual, case.state, "{}: {}", case.name, case.covers);
    }
}
