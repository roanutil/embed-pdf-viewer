//! G3's gate: the Rust scope resolver agrees with the TypeScript one.
//!
//! `multiplatform-port-order.md` G3 calls `auth/scope` a deliberate duplicate.
//! Web keeps the TypeScript resolver and its enforcement callers; native
//! permission enforcement has no other owner, so the rule exists twice on
//! purpose and no deletion at Stage C is implied.
//!
//! Two copies of an authorization rule drift. Drift here is not a wrong pixel,
//! it is a token that grants more than it should on one platform than on the
//! other, and neither side's unit tests would notice. So the copies are pinned
//! together by the vectors under
//! `tooling/engine-conformance/vectors/scope/`, which are what the shipping
//! TypeScript actually returns. Regenerate them, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run scope
//!
//! There is no known-divergence list, unlike the search dialect gate. A search
//! result that is subtly different is a worse search; a permission result that
//! is subtly different is a security bug, and "we already knew about that one"
//! is not a thing to write down about one.

use std::collections::BTreeSet;

use epdf_engine::auth::scope::{
    CollabAction, CollabEntity, CollabTarget, DocCapability, FieldGroupRight, FieldWriteAction,
    IdentityClaims, PdfBits, check_any_field_action, check_capability, check_collab,
    check_field_action, check_set_group, decode_pdf_bits, expanded_capabilities, field_rights_of,
    parse_all, parse_scope,
};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

fn read(name: &str) -> String {
    let path = test_support::corpus::vectors_dir("scope").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// --- parse.json

#[derive(Debug, Deserialize)]
struct ParseFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: String,
    cases: Vec<ParseCase>,
}

#[derive(Debug, Deserialize)]
struct ParseCase {
    name: String,
    covers: String,
    scope: String,
    valid: bool,
    /// The parsed shape TypeScript produced. Compared field by field rather
    /// than as a whole, because the two languages spell the shape
    /// differently: `{kind: 'capability', name}` against an enum variant.
    #[serde(default)]
    parsed: Option<serde_json::Value>,
}

#[test]
fn the_grammar_accepts_and_refuses_what_typescript_does() {
    let file: ParseFile = serde_json::from_str(&read("parse.json")).expect("parse.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "scope.parse");
    assert!(file.cases.len() >= 50, "the corpus shrank: {} cases", file.cases.len());

    let mut failures = Vec::new();
    for case in &file.cases {
        let actual = parse_scope(&case.scope);
        match (case.valid, &actual) {
            (true, Err(e)) => failures.push(format!(
                "{}: TypeScript accepted {:?} ({}), Rust refused it: {e}",
                case.name, case.scope, case.covers,
            )),
            (false, Ok(parsed)) => failures.push(format!(
                "{}: TypeScript refused {:?} ({}), Rust parsed it as {parsed:?}",
                case.name, case.scope, case.covers,
            )),
            (true, Ok(parsed)) => {
                let expected = case.parsed.as_ref().expect("an accepted case carries its shape");
                if let Some(diff) = shape_difference(expected, parsed) {
                    failures.push(format!(
                        "{}: {:?} parsed differently: {diff}",
                        case.name, case.scope
                    ));
                }
            }
            (false, Err(_)) => {}
        }
    }
    assert!(failures.is_empty(), "scope parse divergence:\n  {}", failures.join("\n  "));
}

/// Compares TypeScript's `ParsedScope` object against Rust's enum.
///
/// Written out rather than driven by a `Deserialize` impl on the Rust enum,
/// because deriving one would make the two shapes agree by construction and
/// the point of this gate is that they agree on purpose.
fn shape_difference(
    expected: &serde_json::Value,
    actual: &epdf_engine::auth::scope::ParsedScope,
) -> Option<String> {
    use epdf_engine::auth::scope::{
        CollabActionPattern, CollabFilter, FieldActionPattern, FieldFilter, ParsedScope,
    };

    let kind = expected.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match (kind, actual) {
        ("wildcard", ParsedScope::Wildcard) => None,
        ("virtual", ParsedScope::Virtual) => {
            let name = expected.get("name").and_then(|v| v.as_str());
            (name != Some("pdf.permissions")).then(|| format!("virtual name {name:?}"))
        }
        ("capability", ParsedScope::Capability(name)) => {
            let want = expected.get("name").and_then(|v| v.as_str()).unwrap_or("");
            (want != name.as_str()).then(|| format!("capability {want} vs {name}"))
        }
        ("collab", ParsedScope::FieldCollab { action, filter }) => {
            let want_entity = expected.get("entity").and_then(|v| v.as_str()).unwrap_or("");
            if want_entity != "fields" {
                return Some(format!("entity {want_entity} vs fields"));
            }
            let want_action = expected.get("action").and_then(|v| v.as_str()).unwrap_or("");
            let got_action = match action {
                FieldActionPattern::Any => "*",
                FieldActionPattern::One(a) => a.as_str(),
            };
            if want_action != got_action {
                return Some(format!("action {want_action} vs {got_action}"));
            }
            let want_filter = expected.get("filter")?;
            let want_kind = want_filter.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let want_value = want_filter.get("groupId").and_then(|v| v.as_str());
            let (got_kind, got_value) = match filter {
                FieldFilter::All => ("all", None),
                FieldFilter::Group(v) => ("group", Some(v.as_str())),
            };
            if want_kind != got_kind || want_value != got_value {
                return Some(format!(
                    "filter {want_kind}={want_value:?} vs {got_kind}={got_value:?}"
                ));
            }
            None
        }
        ("collab", ParsedScope::Collab { entity: _, action, filter }) => {
            let want_entity = expected.get("entity").and_then(|v| v.as_str()).unwrap_or("");
            if want_entity != "annotations" {
                return Some(format!("entity {want_entity}, which Rust cannot represent"));
            }
            let want_action = expected.get("action").and_then(|v| v.as_str()).unwrap_or("");
            let got_action = match action {
                CollabActionPattern::Any => "*",
                CollabActionPattern::One(a) => a.as_str(),
            };
            if want_action != got_action {
                return Some(format!("action {want_action} vs {got_action}"));
            }
            let want_filter = expected.get("filter")?;
            let want_kind = want_filter.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let want_value = want_filter
                .get("userId")
                .or_else(|| want_filter.get("groupId"))
                .and_then(|v| v.as_str());
            let (got_kind, got_value) = match filter {
                CollabFilter::All => ("all", None),
                CollabFilter::SelfOnly => ("self", None),
                CollabFilter::CreatedBy(v) => ("createdBy", Some(v.as_str())),
                CollabFilter::Group(v) => ("group", Some(v.as_str())),
            };
            if want_kind != got_kind || want_value != got_value {
                return Some(format!(
                    "filter {want_kind}={want_value:?} vs {got_kind}={got_value:?}"
                ));
            }
            None
        }
        _ => Some(format!("kind {kind} vs {actual:?}")),
    }
}

// --- resolve.json

#[derive(Debug, Deserialize)]
struct ResolveFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    kind: String,
    capabilities: Vec<String>,
    bits: Vec<BitsEntry>,
    cases: Vec<ResolveCase>,
}

#[derive(Debug, Deserialize)]
struct BitsEntry {
    name: String,
    raw: Option<i64>,
    bits: BitsShape,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct BitsShape {
    bit3: bool,
    bit4: bool,
    bit5: bool,
    bit6: bool,
    bit9: bool,
    bit10: bool,
    bit11: bool,
    bit12: bool,
}

impl From<PdfBits> for BitsShape {
    fn from(b: PdfBits) -> Self {
        Self {
            bit3: b.bit3,
            bit4: b.bit4,
            bit5: b.bit5,
            bit6: b.bit6,
            bit9: b.bit9,
            bit10: b.bit10,
            bit11: b.bit11,
            bit12: b.bit12,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResolveCase {
    name: String,
    covers: String,
    scope: Vec<String>,
    #[serde(default)]
    identity: Option<IdentityShape>,
    results: Vec<PerBits>,
}

#[derive(Debug, Deserialize, Default)]
struct IdentityShape {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    groups: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PerBits {
    bits: String,
    expanded: Vec<String>,
    granted: Vec<String>,
    #[serde(default)]
    collab: Option<Vec<Probe>>,
    #[serde(default, rename = "setGroup")]
    set_group: Option<Vec<Probe>>,
    #[serde(default)]
    fields: Option<Vec<Probe>>,
    #[serde(default, rename = "anyField")]
    any_field: Option<Vec<Probe>>,
    #[serde(default, rename = "fieldRights")]
    field_rights: Option<serde_json::Value>,
    #[serde(default, rename = "fieldSetGroup")]
    field_set_group: Option<Vec<Probe>>,
}

#[derive(Debug, Deserialize)]
struct Probe {
    probe: String,
    allowed: bool,
}

#[test]
fn the_bit_decoder_agrees_with_typescript() {
    // Checked separately from resolution, because a decoder that disagreed
    // would make every expansion below wrong for a reason the expansion
    // assertions would not name.
    let file: ResolveFile = serde_json::from_str(&read("resolve.json")).expect("resolve.json");
    for entry in &file.bits {
        let actual: BitsShape = decode_pdf_bits(entry.raw).into();
        assert_eq!(
            actual, entry.bits,
            "permission word {:?} ({:?}) decoded differently",
            entry.name, entry.raw,
        );
    }
}

#[test]
fn the_closed_capability_set_is_the_same_set() {
    let file: ResolveFile = serde_json::from_str(&read("resolve.json")).expect("resolve.json");
    let expected: Vec<&str> = file.capabilities.iter().map(String::as_str).collect();
    let actual: Vec<&str> = DocCapability::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        actual, expected,
        "the capability sets differ, so one side has a scope string the other cannot express",
    );
}

#[test]
fn resolution_agrees_with_typescript() {
    let file: ResolveFile = serde_json::from_str(&read("resolve.json")).expect("resolve.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "scope.resolve");
    assert!(file.cases.len() >= 20, "the corpus shrank: {} cases", file.cases.len());

    let bits_by_name: std::collections::BTreeMap<&str, PdfBits> =
        file.bits.iter().map(|b| (b.name.as_str(), decode_pdf_bits(b.raw))).collect();

    let mut failures = Vec::new();
    let mut checked = 0usize;

    for case in &file.cases {
        let parsed = match parse_all(&case.scope) {
            Ok(parsed) => parsed,
            Err(e) => {
                failures.push(format!("{}: scope did not parse: {e}", case.name));
                continue;
            }
        };
        let shape = case.identity.as_ref();
        let identity = IdentityClaims {
            user_id: shape.and_then(|i| i.user_id.clone()),
            group_id: shape.and_then(|i| i.group_id.clone()),
            groups: shape.map(|i| i.groups.clone()).unwrap_or_default(),
            display_name: None,
        };

        for per in &case.results {
            let bits = *bits_by_name
                .get(per.bits.as_str())
                .unwrap_or_else(|| panic!("{} names unknown bits {}", case.name, per.bits));
            let where_ = format!("{} [{}] ({})", case.name, per.bits, case.covers);

            let expanded: BTreeSet<&str> =
                expanded_capabilities(&parsed, bits).iter().map(|c| c.as_str()).collect();
            let want: BTreeSet<&str> = per.expanded.iter().map(String::as_str).collect();
            if expanded != want {
                failures
                    .push(format!("{where_}: expanded {:?}, TypeScript said {:?}", expanded, want));
            }
            checked += 1;

            // Over the vector's own list, as the generator filters its
            // `ALL_CAPABILITIES`; the closed-set test pins that list to ours.
            let granted: Vec<&str> = file
                .capabilities
                .iter()
                .map(|name| DocCapability::parse(name).expect("a capability Rust has"))
                .filter(|c| check_capability(*c, &parsed, bits))
                .map(|c| c.as_str())
                .collect();
            let want_granted: Vec<&str> = per.granted.iter().map(String::as_str).collect();
            if granted != want_granted {
                failures.push(format!(
                    "{where_}: granted {:?}, TypeScript said {:?}",
                    granted, want_granted
                ));
            }
            checked += 1;

            for probe in per.collab.iter().flatten() {
                let (action, target) = parse_collab_probe(&probe.probe);
                let allowed = check_collab(action, &target, &parsed, &identity, bits);
                if allowed != probe.allowed {
                    failures.push(format!(
                        "{where_}: collab {} was {allowed}, TypeScript said {}",
                        probe.probe, probe.allowed,
                    ));
                }
                checked += 1;
            }

            for probe in per.set_group.iter().flatten() {
                let (new_group, caller_default) = parse_set_group_probe(&probe.probe);
                let allowed =
                    check_set_group(CollabEntity::Annotations, new_group, caller_default, &parsed);
                if allowed != probe.allowed {
                    failures.push(format!(
                        "{where_}: setGroup {} was {allowed}, TypeScript said {}",
                        probe.probe, probe.allowed,
                    ));
                }
                checked += 1;
            }

            for probe in per.field_set_group.iter().flatten() {
                let (new_group, caller_default) = parse_set_group_probe(&probe.probe);
                let allowed =
                    check_set_group(CollabEntity::Fields, new_group, caller_default, &parsed);
                if allowed != probe.allowed {
                    failures.push(format!(
                        "{where_}: fieldSetGroup {} was {allowed}, TypeScript said {}",
                        probe.probe, probe.allowed,
                    ));
                }
                checked += 1;
            }

            for probe in per.fields.iter().flatten() {
                let (action, group) = probe.probe.split_once(':').expect("probe is action:group");
                let group = (group != "-").then_some(group);
                let allowed = check_field_action(write_action(action), group, &parsed, bits);
                if allowed != probe.allowed {
                    failures.push(format!(
                        "{where_}: field {} was {allowed}, TypeScript said {}",
                        probe.probe, probe.allowed,
                    ));
                }
                checked += 1;
            }

            for probe in per.any_field.iter().flatten() {
                let allowed = check_any_field_action(write_action(&probe.probe), &parsed, bits);
                if allowed != probe.allowed {
                    failures.push(format!(
                        "{where_}: anyField {} was {allowed}, TypeScript said {}",
                        probe.probe, probe.allowed,
                    ));
                }
                checked += 1;
            }

            if let Some(want) = &per.field_rights {
                let rights = field_rights_of(&parsed, bits, identity.group_id.as_deref());
                let got = serde_json::json!({
                    "fill": group_right_json(&rights.fill),
                    "sign": group_right_json(&rights.sign),
                    "setGroup": group_right_json(&rights.set_group),
                });
                if &got != want {
                    failures.push(format!("{where_}: fieldRights {got}, TypeScript said {want}"));
                }
                checked += 1;
            }
        }
    }

    assert!(failures.is_empty(), "scope resolution divergence:\n  {}", failures.join("\n  "));
    // A generator that emitted an empty `results` for every case would make
    // the loop above pass without comparing anything.
    assert!(checked >= 400, "only {checked} comparisons ran; the corpus is not doing its job");
}

fn write_action(name: &str) -> FieldWriteAction {
    match name {
        "fill" => FieldWriteAction::Fill,
        "sign" => FieldWriteAction::Sign,
        other => panic!("unknown field action {other}"),
    }
}

/// `FieldGroupRight` as TypeScript spells it, written out for the same
/// reason as `shape_difference`.
fn group_right_json(right: &FieldGroupRight) -> serde_json::Value {
    match right {
        FieldGroupRight::All => serde_json::json!({ "all": true }),
        FieldGroupRight::Groups(groups) => serde_json::json!({ "all": false, "groups": groups }),
    }
}

/// `"update:u2/finance"` back into an action and a target.
fn parse_collab_probe(probe: &str) -> (CollabAction, CollabTarget) {
    let (action, target) = probe.split_once(':').expect("probe is action:user/group");
    let action = match action {
        "create" => CollabAction::Create,
        "update" => CollabAction::Update,
        "delete" => CollabAction::Delete,
        "set-group" => CollabAction::SetGroup,
        other => panic!("unknown probe action {other}"),
    };
    let (user, group) = target.split_once('/').expect("target is user/group");
    let dash = |s: &str| (s != "-").then(|| s.to_string());
    (action, CollabTarget { user_id: dash(user), group_id: dash(group) })
}

/// `"finance<-legal"` back into a destination group and the caller's default.
fn parse_set_group_probe(probe: &str) -> (&str, Option<&str>) {
    let (new_group, caller) = probe.split_once("<-").expect("probe is new<-default");
    (new_group, (caller != "-").then_some(caller))
}
