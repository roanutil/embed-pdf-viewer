//! Every wire kind reaches an implementation, or is listed as not yet
//! routed, with the sub-task that routes it.
//!
//! Replays `vectors/wire/requests.json`, which the TypeScript compiler
//! produced by reading the `WorkerRequest` union: eighty-nine kinds and the
//! arguments each one carries.
//!
//! # What a dispatcher can get wrong that a compiler cannot catch
//!
//! `Host::call` takes a string and a JSON body. Nothing in its signature
//! says which kinds exist, so a missing arm compiles, a misspelled argument
//! key compiles, and both fail at runtime against a host that has no way to
//! tell a protocol gap from an engine fault. That is the entire reason this
//! file exists.
//!
//! The check is deliberately weak about SUCCESS and strict about ROUTING. An
//! arm is exercised with a body built from the vector's own field names, and
//! what is asserted is that the kind is not refused as unknown (the
//! dispatcher's `InvalidArg` "unknown request kind: …", as `WorkerHost`'s)
//! and does not answer `NotImplemented`: whether the operation then succeeds
//! depends on the fixture, and pinning results is what the other suites do.

use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::features::render::raster::PageRaster;
use epdf_engine::features::server::{EncodedImage, ImageEncoder, RenderEncode};
use epdf_engine::host::{
    AUTHORITY_KINDS, FLOOR_ONLY_KINDS, Host, JobEnvelope, LEGACY_KINDS, Payloads, RequestEffect,
    WRITE_KINDS,
};

const VECTOR: &str = "tooling/engine-conformance/vectors/wire/requests.json";

/// `WorkerHost`'s refusal of a kind it has no case for, which the host
/// matches word for word.
const UNKNOWN_KIND: &str = "unknown request kind: ";
const DOC_ID: &str = "doc-1";

/// A document with annotations, forms, attachments and `/PieceInfo`, so most
/// arms have something real to act on.
const FIXTURE: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";

/// The kinds of upstream's union this host does not route yet, each with the
/// sub-task that routes it. That sub-task removes its rows.
const NOT_YET_ROUTED: &[(&str, &str)] = &[("signatures.cancel", "8.26")];

/// Routed kinds whose arm answers `NotImplemented` for the request this
/// test sends, each with the sub-task that ports it. Task 6 stubbed them
/// against the runtime 1461b0c51 API (`// re-port (Task 8):`); 8.22 ported
/// the last, `forms.import`. The owner removes its row; a row whose kind
/// stops answering `NotImplemented` fails.
const STUBBED: &[(&str, &str)] = &[];

#[derive(Debug)]
struct Case {
    kind: String,
    fields: Vec<(String, bool)>,
    control: bool,
}

impl Case {
    fn has(&self, name: &str) -> bool {
        self.fields.iter().any(|(n, _)| n == name)
    }
}

fn cases() -> Vec<Case> {
    let raw = std::fs::read_to_string(test_support::workspace_root().join(VECTOR))
        .unwrap_or_else(|e| panic!("read {VECTOR}: {e}"));
    let file: serde_json::Value = serde_json::from_str(&raw).expect("the vector parses");
    assert_eq!(file["schemaVersion"], 1, "the vector's envelope changed shape");
    file["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| Case {
            kind: case["kind"].as_str().expect("kind").to_owned(),
            fields: case["fields"]
                .as_array()
                .expect("fields")
                .iter()
                .map(|f| {
                    (
                        f["name"].as_str().expect("name").to_owned(),
                        f["optional"].as_bool().expect("optional"),
                    )
                })
                .collect(),
            control: case["control"].as_bool().expect("control"),
        })
        .collect()
}

/// An encoder that answers empty bytes, so the `*Encoded` kinds reach their
/// arm's end rather than the host's refusal for having no encoder.
struct NullEncoder;

impl ImageEncoder for NullEncoder {
    fn encode(
        &self,
        raster: &PageRaster,
        _encode: &RenderEncode,
    ) -> Result<EncodedImage, EngineError> {
        Ok(EncodedImage {
            content_type: "image/png".into(),
            width: raster.width,
            height: raster.height,
            bytes: Vec::new(),
        })
    }
}

fn open() -> Host {
    let mut host = Host::new().with_encoder(Box::new(NullEncoder));
    let bytes = std::fs::read(test_support::workspace_root().join(FIXTURE))
        .unwrap_or_else(|e| panic!("read {FIXTURE}: {e}"));
    let response = host.call(
        &serde_json::json!({ "kind": "open.fatMem", "docId": DOC_ID }).to_string(),
        vec![bytes],
    );
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("the response");
    assert!(body.get("error").is_none(), "the fixture would not open: {}", response.json);
    host
}

/// The wire's `PageRef` (595bdc6c7).
fn page_ref(object_number: u64) -> serde_json::Value {
    serde_json::json!({ "kind": "objectNumber", "objectNumber": object_number })
}

/// An `AnnotationRef` by object number.
fn annotation_ref(page: u64, object_number: u64) -> serde_json::Value {
    serde_json::json!({ "kind": "objectNumber", "page": page_ref(page),
        "objectNumber": object_number })
}

/// Send one request to `host` and answer the whole envelope.
fn send(host: &mut Host, request: serde_json::Value) -> serde_json::Value {
    let mut request = request;
    request["jobId"] = 2.into();
    request["docId"] = DOC_ID.into();
    let response = host.call(&request.to_string(), Vec::new());
    serde_json::from_str(&response.json).expect("every response is JSON")
}

/// [`send`], through `Host::call_legacy`.
fn send_legacy(host: &mut Host, request: serde_json::Value) -> serde_json::Value {
    let mut request = request;
    request["jobId"] = 2.into();
    request["docId"] = DOC_ID.into();
    let response = host.call_legacy(&request.to_string(), Vec::new());
    serde_json::from_str(&response.json).expect("every response is JSON")
}

/// Something of roughly the right shape for a required argument.
///
/// Not a valid value, and not trying to be: the operation may well refuse
/// it. What it must not do is refuse it as an unknown kind, and a body with
/// the argument missing entirely would be refused earlier than the arm that
/// would have read it.
fn placeholder(name: &str) -> serde_json::Value {
    match name {
        "toIndex" | "destIndex" | "pageIndex" | "count" => serde_json::json!(1),
        "widget" => annotation_ref(1, 1),
        "page" => page_ref(1),
        "pages" => serde_json::json!([page_ref(1)]),
        "refs" => serde_json::json!([annotation_ref(1, 1)]),
        "ref" => serde_json::json!({ "kind": "key", "key": "nothing" }),
        "rotation" => serde_json::json!(90),
        "usage" => serde_json::json!("display"),
        "mode" => serde_json::json!("incremental"),
        "format" => serde_json::json!("fdf"),
        "size" => serde_json::json!({ "width": 612.0, "height": 792.0 }),
        "scope" => serde_json::json!({ "kind": "document" }),
        "request" => serde_json::json!({ "query": { "text": "the" } }),
        "draft" => serde_json::json!({ "subtype": "square",
            "rect": { "left": 0.0, "bottom": 0.0, "right": 1.0, "top": 1.0 } }),
        "patch" => serde_json::json!({}),
        "value" => serde_json::json!({ "kind": "text", "text": "x" }),
        "file" => serde_json::json!({ "name": "a.txt" }),
        "effects" => serde_json::json!([]),
        "encode" => serde_json::json!({ "format": "png" }),
        "weight" => serde_json::json!(400),
        "italic" => serde_json::json!(-1),
        "opId" => serde_json::json!("op-1"),
        "authority" => serde_json::json!({ "identity": {}, "grants": null }),
        "path" | "basePath" => serde_json::json!("/nonexistent/for-this-check.pdf"),
        _ => serde_json::json!("x"),
    }
}

/// The body for one kind: the envelope, plus every required argument. The
/// `effect` is left out, which the host takes until Task 8.27: the vector
/// names the field and not its value.
fn request(case: &Case) -> String {
    let mut body = serde_json::Map::new();
    body.insert("kind".into(), case.kind.as_str().into());
    body.insert("jobId".into(), 1.into());
    body.insert("docId".into(), DOC_ID.into());
    for (name, optional) in &case.fields {
        if *optional || name == "effect" {
            continue;
        }
        body.insert(name.clone(), placeholder(name));
    }
    serde_json::Value::Object(body).to_string()
}

/// Enough payloads for any arm that wants one.
///
/// A PDF header rather than empty bytes, because an arm that opens its
/// payload as a document should fail on the CONTENT rather than on there
/// being nothing there, and the two look the same from here otherwise.
fn payloads() -> Payloads {
    vec![b"%PDF-1.7\n".to_vec(), b"%PDF-1.7\n".to_vec()]
}

#[test]
fn every_wire_kind_routes_to_an_implementation() {
    let cases = cases();
    assert_eq!(cases.len(), 89, "the vector and the protocol disagree about how many kinds exist");
    for (kind, owner) in NOT_YET_ROUTED {
        assert!(
            cases.iter().any(|c| c.kind == *kind),
            "{kind} (owner {owner}) is listed as not yet routed but is not a wire kind",
        );
    }

    let mut wrong = Vec::new();
    for case in &cases {
        // A control frame steers the worker rather than asking it for a
        // result. They are routed too, and covered by their own tests; what
        // they are not is safe to fire blindly here, since `shutdown`
        // destroys the registry the rest of the loop is using.
        if case.control {
            continue;
        }
        // Both open frames make a session, and the fixture is already open
        // under this id. Firing them would either collide with it or leave a
        // second session behind for the arms that follow.
        if case.kind.starts_with("open.") {
            continue;
        }

        let mut host = open();
        let response = host.call(&request(case), payloads());
        let body: serde_json::Value =
            serde_json::from_str(&response.json).expect("every response is JSON");
        let message = body["error"]["message"].as_str().unwrap_or("");
        let unknown = message.starts_with(UNKNOWN_KIND);
        let listed = NOT_YET_ROUTED.iter().find(|(kind, _)| *kind == case.kind);
        let not_implemented = body["error"]["code"] == "NotImplemented";
        if let Some((kind, owner)) = STUBBED.iter().find(|(kind, _)| *kind == case.kind) {
            if !not_implemented {
                wrong.push(format!("{kind} is ported; remove it from STUBBED ({owner}): {body}"));
            }
            continue;
        }
        match listed {
            // Routed now: the row is stale, and its owner removes it.
            Some((kind, owner)) if !unknown => {
                wrong.push(format!("{kind} routes; remove it from NOT_YET_ROUTED ({owner})"))
            }
            Some(_) => {}
            None if unknown => wrong.push(format!("{}: {message}", case.kind)),
            None if not_implemented => {
                wrong.push(format!("{} answers NotImplemented: {message}", case.kind))
            }
            None => {}
        }
    }

    assert!(
        wrong.is_empty(),
        "{} of {} kinds:\n  {}",
        wrong.len(),
        cases.len(),
        wrong.join("\n  ")
    );
}

#[test]
fn every_removed_kind_is_unknown_to_the_wire_and_known_to_the_legacy_entry() {
    // The kind upstream removed that is still parked (`signatures.abort`
    // renamed `signatures.cancel`, 7bccc4f35) answer as `WorkerHost` answers
    // a kind it has no case for, and reach their old arm only through
    // `call_legacy`, until each owner sub-task retires it. 8.8 retired
    // `attachments.create`, `attachments.delete` and `pieceInfo.clear`; 8.12
    // the three annotation listings; 8.16 `annotations.move`; 8.18 the five
    // page structure kinds, now `document.apply` ops; 8.20
    // `forms.applyEffects`, whose effects are `document.apply` ops; 8.21
    // `forms.attachWidget`, which `forms.addWidget` replaced (d7bd2d277).
    assert_eq!(LEGACY_KINDS.len(), 1);
    let cases = cases();
    for kind in LEGACY_KINDS {
        assert!(!cases.iter().any(|c| c.kind == *kind), "{kind} is in upstream's union");
        let mut host = open();
        let request = serde_json::json!({ "kind": kind, "jobId": 1, "docId": DOC_ID }).to_string();
        let wire: serde_json::Value =
            serde_json::from_str(&host.call(&request, payloads()).json).unwrap();
        assert_eq!(wire["error"]["code"], "InvalidArg", "{kind}: {wire}");
        assert_eq!(wire["error"]["message"], format!("{UNKNOWN_KIND}{kind}"), "{kind}: {wire}");

        let legacy: serde_json::Value =
            serde_json::from_str(&host.call_legacy(&request, payloads()).json).unwrap();
        let message = legacy["error"]["message"].as_str().unwrap_or("");
        assert!(!message.starts_with(UNKNOWN_KIND), "{kind} is unknown to call_legacy: {legacy}");
    }
}

#[test]
fn the_legacy_entry_knows_no_other_kind() {
    let mut host = open();
    let answer = send_legacy(&mut host, serde_json::json!({ "kind": "pages.list" }));
    assert_eq!(answer["error"]["message"], format!("{UNKNOWN_KIND}pages.list"), "{answer}");
}

#[test]
fn the_envelope_kinds_agree_with_the_vector() {
    // Which kinds carry `WriteJobFields` and an `authority` is read off the
    // TypeScript union by the vector; the host's lists must say the same.
    for case in cases() {
        let kind = case.kind.as_str();
        assert_eq!(case.has("opId"), WRITE_KINDS.contains(&kind), "{kind}: opId");
        assert_eq!(
            case.has("objectNumberFloor"),
            WRITE_KINDS.contains(&kind) || FLOOR_ONLY_KINDS.contains(&kind),
            "{kind}: objectNumberFloor",
        );
        assert_eq!(case.has("authority"), AUTHORITY_KINDS.contains(&kind), "{kind}: authority");
    }
}

#[test]
fn the_page_space_boundary_knows_every_wire_kind_and_no_other() {
    // `page_space::boundary` matches `WireKind` with no catch-all, so a kind
    // it knows is placed as converting or as holding no places. This is what
    // keeps that list the wire's: a kind upstream adds to the union and the
    // vector records fails here until the boundary places it.
    use epdf_engine::page_space::boundary::WireKind;
    let mut vector: Vec<String> = cases().into_iter().map(|c| c.kind).collect();
    vector.sort();
    let mut known: Vec<String> = WireKind::ALL.iter().map(|(k, _)| (*k).to_owned()).collect();
    known.sort();
    assert_eq!(known, vector);
    for kind in LEGACY_KINDS {
        assert_eq!(WireKind::of(kind), None, "{kind} was removed from the wire");
    }
}

#[test]
fn a_file_render_takes_its_target_in_page_space() {
    // `renderOptionsInFileSpace` on the page the render opened: a target in
    // page space ({x, y, width, height}) is read; one in the file's
    // coordinates ({left, bottom, right, top}) is not the shape it takes.
    let mut host = Host::new();
    let path = test_support::workspace_root().join(FIXTURE);
    let render = |host: &mut Host, rect: serde_json::Value| {
        let response = host.call(
            &serde_json::json!({ "kind": "document.renderPageFile", "jobId": 3,
                "path": path, "password": "", "pageIndex": 0,
                "options": { "target": { "kind": "rect", "rect": rect },
                    "viewport": { "kind": "scale", "scale": 0.5 } } })
            .to_string(),
            Vec::new(),
        );
        serde_json::from_str::<serde_json::Value>(&response.json).expect("the response")
    };
    let body = render(&mut host, serde_json::json!({ "x": 0, "y": 0, "width": 100, "height": 40 }));
    assert!(body.get("error").is_none(), "{body}");
    assert_eq!(
        (body["result"]["width"].as_u64(), body["result"]["height"].as_u64()),
        (Some(50), Some(20))
    );
    let body =
        render(&mut host, serde_json::json!({ "left": 0, "bottom": 0, "right": 100, "top": 40 }));
    assert_eq!(body["error"]["code"], "WireFormat", "{body}");
}

#[test]
fn a_write_envelope_is_read_whole() {
    let body = serde_json::json!({ "kind": "annotations.delete", "effect": "write",
        "jobId": 7, "opId": "op-7", "objectNumberFloor": 900,
        "authority": { "identity": { "userId": "u", "displayName": "U" },
            "grants": { "scope": ["*"], "pdfBits": { "bit3": true, "bit4": true, "bit5": true,
                "bit6": true, "bit9": true, "bit10": true, "bit11": true, "bit12": true } } } });
    let envelope = JobEnvelope::parse("annotations.delete", &body).expect("a whole envelope");
    assert_eq!(envelope.effect, Some(RequestEffect::Write));
    assert_eq!(envelope.job_id.map(|j| j.to_string()), Some("7".into()));
    assert_eq!(envelope.op_id.as_deref(), Some("op-7"));
    assert_eq!(envelope.object_number_floor, Some(900));
    let authority = envelope.authority.expect("an authority");
    assert_eq!(authority.identity.user_id.as_deref(), Some("u"));
    assert_eq!(authority.grants.expect("grants").scope, ["*"]);
    assert_eq!(authority.protection, None);
}

#[test]
fn an_absent_envelope_is_the_default() {
    // Until Task 8.27: the kernel's calls and the older tests send none.
    let body = serde_json::json!({ "kind": "annotations.delete" });
    assert_eq!(JobEnvelope::parse("annotations.delete", &body).unwrap(), JobEnvelope::default());
}

#[test]
fn a_field_the_kind_does_not_take_is_ignored() {
    // As `WorkerHost` ignores it: a read carries no op and no authority.
    let body = serde_json::json!({ "kind": "pages.list", "opId": 3, "authority": 4,
        "objectNumberFloor": "x" });
    assert_eq!(JobEnvelope::parse("pages.list", &body).unwrap(), JobEnvelope::default());
}

#[test]
fn a_malformed_envelope_is_a_wire_error() {
    let mut host = open();
    for bad in [
        serde_json::json!({ "kind": "pages.list", "effect": "sometimes" }),
        serde_json::json!({ "kind": "metadata.update", "patch": {}, "opId": 3 }),
        serde_json::json!({ "kind": "metadata.update", "patch": {}, "objectNumberFloor": -1 }),
        serde_json::json!({ "kind": "forms.setValue", "authority": { "grants": null } }),
    ] {
        let mut request = bad.clone();
        request["docId"] = DOC_ID.into();
        let answer: serde_json::Value =
            serde_json::from_str(&host.call(&request.to_string(), Vec::new()).json).unwrap();
        assert_eq!(answer["error"]["code"], "WireFormat", "{bad}: {answer}");
    }
    let job: serde_json::Value = serde_json::from_str(
        &host.call(r#"{"kind":"pages.list","docId":"doc-1","jobId":true}"#, Vec::new()).json,
    )
    .unwrap();
    assert_eq!(job["error"]["code"], "WireFormat", "{job}");
}

#[test]
fn an_abort_names_its_job_by_number() {
    // `WorkerJobId` is a number. Nothing is in flight, so nothing aborts.
    let mut host = open();
    let answer: serde_json::Value =
        serde_json::from_str(&host.call(r#"{"kind":"abort","jobId":41}"#, Vec::new()).json)
            .unwrap();
    assert_eq!(answer["result"]["aborted"], false, "{answer}");
}

#[test]
fn an_unknown_kind_is_refused_rather_than_ignored() {
    // The negative half, so the check above cannot pass by routing
    // everything to the same arm. A kind that is not in the protocol has to
    // come back as the TypeScript refuses it.
    let mut host = open();
    let response = host.call(
        &serde_json::json!({ "kind": "pages.teleport", "docId": DOC_ID }).to_string(),
        Vec::new(),
    );
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("the response");
    assert_eq!(body["error"]["code"], format!("{:?}", EngineErrorCode::InvalidArg));
    assert_eq!(body["error"]["message"], format!("{UNKNOWN_KIND}pages.teleport"));
}

#[test]
fn a_request_that_is_not_json_is_a_wire_error_rather_than_a_panic() {
    // The dispatcher's whole contract is that it always answers. A host that
    // got nothing back could not tell a refused operation from a crashed
    // engine, which is the failure this shape exists to prevent.
    let mut host = Host::new();
    let response = host.call("{not json", Vec::new());
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("still JSON");
    assert_eq!(body["error"]["code"], format!("{:?}", EngineErrorCode::WireFormat));
    assert!(response.payloads.is_empty(), "a failure carried payloads");
}

#[test]
fn a_refusal_carries_the_serialized_engine_error_and_nothing_else() {
    // `WorkerHost` rejects with `serializeError(err)`: `{name: "EngineError",
    // code, message, details?}` and no `cause`. The `name` is what
    // `EngineError.is` sniffs once the value has lost its prototype.
    let mut host = open();
    let missing =
        send(&mut host, serde_json::json!({ "kind": "pages.geometry", "page": page_ref(999_999) }));
    let error = missing["error"].as_object().expect("an error object");
    let mut keys: Vec<&str> = error.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["code", "message", "name"], "{missing}");
    assert_eq!(missing["error"]["name"], "EngineError", "{missing}");
    assert_eq!(missing["error"]["code"], "NotFound", "{missing}");
    assert!(missing["error"]["message"].is_string(), "{missing}");
    assert!(missing.get("result").is_none(), "a refusal carried a result: {missing}");
}

#[test]
fn a_session_operation_without_a_document_names_what_is_missing() {
    // Reported rather than defaulted. An engine that picked "the only open
    // session" would work in every test and fail the moment a host opened
    // two documents.
    let mut host = Host::new();
    let response = host.call(&serde_json::json!({ "kind": "pages.list" }).to_string(), Vec::new());
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("the response");
    assert_eq!(body["error"]["code"], format!("{:?}", EngineErrorCode::WireFormat));
    assert!(
        body["error"]["message"].as_str().unwrap_or("").contains("pages.list"),
        "the refusal does not say which request it was: {}",
        response.json,
    );
}

#[test]
fn a_page_is_addressed_by_the_ref_the_listing_answered() {
    // The round trip the wire exists for: the `ref` a `pages.list` answers is
    // the address every per-page request takes back, unchanged. A host that
    // unpacked the number and sent it flat would be speaking the old wire.
    let mut host = open();
    let listing = send(&mut host, serde_json::json!({ "kind": "pages.list" }));
    let first = listing["result"]["pages"][0]["ref"].clone();
    assert_eq!(first["kind"], "objectNumber", "the listing names no PageRef: {listing}");
    assert!(first["objectNumber"].as_u64().is_some_and(|n| n > 0), "{listing}");

    let text = send(&mut host, serde_json::json!({ "kind": "pages.text", "page": first }));
    assert!(text.get("error").is_none(), "pages.text refused the listing's ref: {text}");

    let annotations =
        send(&mut host, serde_json::json!({ "kind": "annotations.list", "pages": [first] }));
    assert_eq!(
        annotations["result"]["pages"],
        serde_json::json!([first]),
        "the list names its page differently from the request: {annotations}",
    );

    // A page op of `document.apply` takes the listing's ref too.
    let rotated = send(
        &mut host,
        serde_json::json!({ "kind": "document.apply", "change": { "ops": [
            { "type": "pages.rotate", "pages": [first], "rotation": 90 }] } }),
    );
    assert!(rotated.get("error").is_none(), "pages.rotate refused the listing's ref: {rotated}");
    let layout = &rotated["result"]["result"]["items"][0]["layout"]["pages"][0];
    assert_eq!(layout["ref"], first, "the rotated page lost its identity: {rotated}");
    assert_eq!(layout["rotation"], 90, "{rotated}");
}

#[test]
fn a_page_ref_in_another_spelling_is_refused() {
    // `pageObjectNumber` beside the kind is what this host spoke before
    // 595bdc6c7, and a flat number what it spoke before `9f13cea3c`. Read
    // silently, either would keep a stale host working against one engine
    // and failing against the other.
    let mut host = open();
    let flat = send(&mut host, serde_json::json!({ "kind": "pages.text", "pageObjectNumber": 1 }));
    assert_eq!(flat["error"]["code"], format!("{:?}", EngineErrorCode::WireFormat), "{flat}");

    // Kept divergence (`3baec217b`): a malformed PageRef is `InvalidArg`,
    // where the TypeScript worker does not validate it.
    for bad in [
        serde_json::json!(1),
        serde_json::json!({ "kind": "objectNumber", "pageObjectNumber": 1 }),
        serde_json::json!({ "kind": "index", "objectNumber": 1 }),
        serde_json::json!({ "kind": "objectNumber", "objectNumber": 0 }),
        serde_json::json!({ "kind": "objectNumber", "objectNumber": 1.5 }),
    ] {
        let answer = send(&mut host, serde_json::json!({ "kind": "pages.text", "page": bad }));
        assert_eq!(answer["error"]["code"], "InvalidArg", "{bad}: {answer}");
        let pages = send(
            &mut host,
            serde_json::json!({ "kind": "pages.flatten", "pages": [bad], "usage": "display" }),
        );
        assert_eq!(pages["error"]["code"], "InvalidArg", "{bad}: {pages}");
    }
}

#[test]
fn a_ref_to_no_page_is_not_found() {
    // Well formed and naming nothing: `resolvePageRef`'s `NotFound`, not a
    // malformed request.
    let mut host = open();
    let missing =
        send(&mut host, serde_json::json!({ "kind": "pages.geometry", "page": page_ref(999_999) }));
    assert_eq!(missing["error"]["code"], format!("{:?}", EngineErrorCode::NotFound), "{missing}");
}

#[test]
fn an_annotation_is_named_by_object_number_or_base_index_only() {
    // bc0057f3a: the `nm` and `index` kinds are gone from the wire. A
    // `baseIndex` names an inline annotation, and one that names no such
    // annotation on the page is `NotFound`.
    let mut host = open();
    let page =
        send(&mut host, serde_json::json!({ "kind": "pages.list" }))["result"]["pages"][0]["ref"]
            .clone();
    for gone in [
        serde_json::json!({ "kind": "nm", "page": page, "nm": "x" }),
        serde_json::json!({ "kind": "index", "page": page, "index": 0,
            "revision": { "docSessionId": "s", "page": page, "generation": 0 } }),
    ] {
        let answer =
            send(&mut host, serde_json::json!({ "kind": "annotations.delete", "ref": gone }));
        assert_eq!(answer["error"]["code"], "WireFormat", "{gone}: {answer}");
    }
    let nowhere = send(
        &mut host,
        serde_json::json!({ "kind": "annotations.delete",
            "ref": { "kind": "baseIndex", "page": page, "baseIndex": 9_999 } }),
    );
    assert_eq!(nowhere["error"]["code"], "NotFound", "{nowhere}");
}

#[test]
fn a_widget_link_names_its_widget_by_annotation_ref() {
    // `forms.detachWidget` takes the widget's `AnnotationRef` (`40c96e5ab`).
    // A bare number is the old wire, the `nm` kind is gone, and a
    // `baseIndex` cannot join a field, as `FormMutator` refuses it once the
    // field resolves: the field tree holds widgets by reference.
    let mut host = Host::new();
    let response = host.call(
        &serde_json::json!({ "kind": "open.fatMem", "docId": DOC_ID }).to_string(),
        vec![test_support::fixture("tooling/engine-conformance/fixtures/form_fields.pdf")],
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    let field = serde_json::json!({ "kind": "objectNumber", "objectNumber": 9 });
    let bare = send(
        &mut host,
        serde_json::json!({ "kind": "forms.detachWidget", "ref": field, "widget": 9 }),
    );
    assert_eq!(bare["error"]["code"], format!("{:?}", EngineErrorCode::WireFormat), "{bare}");
    let by_name = send(
        &mut host,
        serde_json::json!({ "kind": "forms.detachWidget", "ref": field,
            "widget": { "kind": "nm", "page": page_ref(1), "nm": "w" } }),
    );
    assert_eq!(by_name["error"]["code"], "WireFormat", "{by_name}");
    let by_position = send(
        &mut host,
        serde_json::json!({ "kind": "forms.detachWidget", "ref": field,
            "widget": { "kind": "baseIndex", "page": page_ref(3), "baseIndex": 0 } }),
    );
    assert_eq!(by_position["error"]["code"], "InvalidArg", "{by_position}");
    assert_eq!(
        by_position["error"]["message"], "widget must be addressed by object number",
        "{by_position}"
    );
}

#[test]
fn a_file_render_names_the_page_it_rendered() {
    // `document.renderPageFile` addresses a page by INDEX, because there is
    // no session to have listed it, and answers the `page: PageRef` that
    // index turned out to be, beside the file's page count.
    let mut host = Host::new();
    let path = test_support::workspace_root().join(FIXTURE);
    let response = host.call(
        &serde_json::json!({ "kind": "document.renderPageFile", "jobId": 3,
            "path": path, "password": "", "pageIndex": 0,
            "options": { "scale": 0.1 } })
        .to_string(),
        Vec::new(),
    );
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("the response");
    assert!(body.get("error").is_none(), "{body}");
    assert_eq!(body["result"]["page"]["kind"], "objectNumber", "{body}");
    assert!(body["result"]["page"]["objectNumber"].as_u64().is_some_and(|n| n > 0), "{body}");
    assert!(body["result"]["pageCount"].as_u64().is_some_and(|n| n > 0), "{body}");
    assert_eq!(response.payloads.len(), 1, "the pixels ride as the payload");
}
