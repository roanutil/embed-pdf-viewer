//! The native handle's requests and answers: each kernel path as the job
//! upstream's local document handle builds (`Local*Service.ts`), sent to a
//! real `epdf-engine` host, and the answer as the service returns it.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_engine::auth::scope::IdentityClaims;
use epdf_engine::host::{Host, HostResponse};
use epdf_engine::session::authority::SessionAuthority;
use epdf_ffi::KernelThread;
use epdf_kernel::engine::{DocumentHandle, Engine};
use epdf_viewer_host::engine_worker::EngineWorker;
use epdf_viewer_host::host_table::HostTable;
use epdf_viewer_host::native_engine::NativeEngine;
use epdf_viewer_host::translate::{
    Client, Decode, Decoded, LOCAL_PATHS, PATHS, decode, maps, translate,
};
use futures::channel::oneshot;
use serde_json::{Value, json};

const FORM_FIELDS: &str = "tooling/engine-conformance/fixtures/form_fields.pdf";
const ANNOTATION_KINDS: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";
const ENGINE_FIXTURES: &str = "packages/engine/main/test/fixtures";

/// A handle's side of a request, allowing everything.
fn client() -> Client {
    let authority =
        SessionAuthority::new(&["*".to_string()], IdentityClaims::default(), Default::default())
            .unwrap()
            .to_wire();
    Client { authority, allows: Box::new(|_| true) }
}

/// A handle that may only do `allowed`.
fn client_allowing(allowed: &'static [&'static str]) -> Client {
    Client { authority: client().authority, allows: Box::new(|c| allowed.contains(&c)) }
}

fn answer(json: &str) -> HostResponse {
    HostResponse { json: json.into(), payloads: Vec::new() }
}

/// A host with `fixture` open as `d`.
fn host_with(fixture: &str) -> Host {
    let mut host = Host::new();
    let open = json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": "d",
                       "scope": ["*"], "reserveObjectNumbers": 64 });
    let opened = host.call(&open.to_string(), vec![test_support::fixture(fixture)]);
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    host
}

/// One kernel call on `host`'s document `d`, as the native handle sends it.
fn run(
    host: &mut Host,
    path: &str,
    pon: Option<u32>,
    args: &[Value],
) -> Result<Decoded, epdf_kernel::Error> {
    let call = translate("d", path, pon, args, 7, &client())?;
    decode(&call.decode, host.call(&call.request.to_string(), call.payloads))
}

fn first_page(host: &mut Host) -> u32 {
    let list = run(host, "pages.list", None, &[]).unwrap().value;
    list["pages"][0]["ref"]["objectNumber"].as_u64().unwrap() as u32
}

// ── requests ──────────────────────────────────────────────────────────────

#[test]
fn a_read_goes_out_as_its_wire_kind_with_its_envelope() {
    let call = translate("d1", "metadata.get", None, &[], 7, &client()).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "metadata.read", "effect": "read", "jobId": 7, "docId": "d1" })
    );
    assert!(matches!(call.decode, Decode::Field("metadata")));
}

#[test]
fn a_write_carries_its_op_id_and_a_change_its_authority() {
    // `opIdOf(options)`: the caller's, else a fresh one.
    let call = translate(
        "d",
        "metadata.update",
        None,
        &[json!({ "title": "T" }), json!({ "opId": "op-1" })],
        8,
        &client(),
    )
    .unwrap();
    assert_eq!(call.request["opId"], "op-1");
    assert_eq!(call.request["effect"], "write");
    assert_eq!(call.request["patch"], json!({ "title": "T" }));
    let fresh = translate("d", "metadata.update", None, &[json!({})], 9, &client()).unwrap();
    assert!(fresh.request["opId"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(fresh.request.get("authority").is_none(), "metadata.update takes no authority");
    // `document.apply` names the handle's `guard.changeAuthority()`.
    let apply = translate("d", "apply", None, &[json!({ "ops": [] })], 10, &client()).unwrap();
    assert_eq!(apply.request["kind"], "document.apply");
    assert_eq!(apply.request["authority"], client().authority);
}

#[test]
fn a_page_chain_carries_its_page() {
    let call = translate(
        "d1",
        "page.render.image",
        Some(12),
        &[json!({ "scale": 2, "format": "png" })],
        8,
        &client(),
    )
    .unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.render", "effect": "read", "jobId": 8, "docId": "d1",
            "page": { "kind": "objectNumber", "objectNumber": 12 },
            "options": { "scale": 2 }
        })
    );
    assert!(matches!(call.decode, Decode::Raster));
}

#[test]
fn a_page_chain_without_a_page_is_invalid_input() {
    let error = translate("d1", "page.text.get", None, &[], 9, &client()).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_unmapped_path_is_unsupported_not_a_panic() {
    let error = translate("d1", "scripts.compile", None, &[], 10, &client()).err().unwrap();
    assert_eq!(error.code(), Some("unsupported"));
}

#[test]
fn a_whole_javascript_number_goes_out_as_an_integer_and_undefined_is_left_out() {
    let call = translate(
        "d",
        "pages.flatten",
        None,
        &[
            json!([{ "kind": "objectNumber", "objectNumber": 7.0 }]),
            json!({ "usage": "print", "x": { "$undefined": true } }),
        ],
        3,
        &client(),
    )
    .unwrap();
    assert_eq!(call.request["pages"], json!([{ "kind": "objectNumber", "objectNumber": 7 }]));
    assert!(call.request["pages"][0]["objectNumber"].is_u64());
    assert_eq!(call.request["usage"], "print");
    // Flatten's default usage is `display`.
    let call = translate("d", "pages.flatten", None, &[json!([])], 4, &client()).unwrap();
    assert_eq!(call.request["usage"], "display");
    assert_eq!(call.request["effect"], "contentWrite");
}

#[test]
fn bytes_inside_a_request_travel_as_payloads() {
    // `wirePack`'s transfer list: each `Uint8Array` a payload the request names.
    let draft = json!({ "subtype": "stamp", "box": { "x": 0, "y": 0, "width": 10, "height": 10 } });
    let options =
        json!({ "resources": { "appearance": { "$bytes": "AAEC" } }, "objectNumber": 41.0 });
    let call =
        translate("d", "page.annotations.create", Some(3), &[draft.clone(), options], 5, &client())
            .unwrap();
    assert_eq!(call.request["draft"], draft);
    assert_eq!(call.request["resources"], json!({ "appearance": { "payload": 0 } }));
    assert_eq!(call.request["objectNumber"], 41);
    assert_eq!(call.payloads, vec![vec![0, 1, 2]]);
}

#[test]
fn a_bytes_value_that_does_not_decode_is_invalid_input() {
    let bad = json!({ "ops": [{ "type": "pages.insert", "source": { "$bytes": "%%%" } }] });
    let error = translate("d", "apply", None, &[bad], 6, &client()).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn a_download_defaults_to_an_incremental_save() {
    // `DEFAULT_PDF_SAVE_MODE` (`dto/PdfSaveMode.ts:3`).
    let call = translate("d", "download", None, &[], 1, &client()).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "document.saveBuffer", "effect": "snapshot", "jobId": 1, "docId": "d", "mode": "incremental" })
    );
    let call =
        translate("d", "download", None, &[json!({ "mode": "rewrite" })], 1, &client()).unwrap();
    assert_eq!(call.request["mode"], "rewrite");
    let error =
        translate("d", "download", None, &[json!({ "mode": "flat" })], 1, &client()).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_extract_resolves_its_flatten_against_the_handles_rights() {
    // `resolvePageExtract` (`dto/PageExtract.ts:59-89`).
    let pages = json!([{ "kind": "objectNumber", "objectNumber": 3 }]);
    let call =
        translate("d", "pages.extract", None, std::slice::from_ref(&pages), 1, &client()).unwrap();
    assert!(call.request.get("flatten").is_none());
    let error = translate(
        "d",
        "pages.extract",
        None,
        &[pages.clone(), json!({ "includeAnnotations": true })],
        1,
        &client(),
    )
    .err()
    .unwrap();
    assert_eq!(error.code(), Some("InvalidArg"));
    let call = translate(
        "d",
        "pages.extract",
        None,
        &[pages.clone(), json!({ "flatten": { "usage": "print" } })],
        1,
        &client(),
    )
    .unwrap();
    assert_eq!(
        call.request["flatten"],
        json!({ "usage": "print", "includeAnnotations": true, "includeFormFields": true })
    );
    // `doc.download.flattened` alone takes a flattened copy, and the layers
    // the handle may not read are left out.
    let only_flattened = client_allowing(&["doc.download.flattened"]);
    let call = translate(
        "d",
        "pages.extract",
        None,
        &[pages.clone(), json!({ "flatten": {} })],
        1,
        &only_flattened,
    )
    .unwrap();
    assert_eq!(
        call.request["flatten"],
        json!({ "usage": "display", "includeAnnotations": false, "includeFormFields": false })
    );
    let error =
        translate("d", "pages.extract", None, std::slice::from_ref(&pages), 1, &only_flattened)
            .err()
            .unwrap();
    assert_eq!(error.member("required"), Some(&json!("doc.download")));
    let error = translate(
        "d",
        "pages.extract",
        None,
        &[pages, json!({ "flatten": {}, "includeAnnotations": true })],
        1,
        &only_flattened,
    )
    .err()
    .unwrap();
    assert_eq!(error.member("required"), Some(&json!("doc.annotate.read")));
}

#[test]
fn a_resource_download_picks_its_kind_by_role() {
    let target = json!({ "kind": "objectNumber", "page": { "kind": "objectNumber", "objectNumber": 3 }, "objectNumber": 9 });
    let file = translate(
        "d",
        "page.annotations.downloadResource",
        Some(3),
        &[target.clone(), json!("file")],
        1,
        &client(),
    )
    .unwrap();
    assert_eq!(file.request["kind"], "annotations.readFile");
    assert_eq!(file.request["maxDecodedBytes"], 256 * 1024 * 1024);
    let appearance = translate(
        "d",
        "page.annotations.downloadResource",
        Some(3),
        &[target, json!("appearance")],
        1,
        &client(),
    )
    .unwrap();
    assert_eq!(appearance.request["kind"], "annotations.readAppearance");
    assert!(appearance.request.get("maxDecodedBytes").is_none());
}

#[test]
fn every_routed_path_is_listed_and_namespaces_answer() {
    for path in PATHS {
        let pon = path.starts_with("page.").then_some(3);
        let routed = translate("d", path, pon, &[json!({}), json!({}), json!({})], 1, &client());
        // A path whose arguments these are not still routes: only its
        // bytes may be missing.
        if let Err(error) = &routed {
            assert_eq!(error.code(), Some("invalid-input"), "{path}: {error}");
        }
        assert!(maps(path), "{path}");
    }
    for path in LOCAL_PATHS {
        assert!(maps(path), "{path}");
    }
    for namespace in
        ["page.measure", "redaction", "signatures", "pages", "render", "attachments", "outline"]
    {
        assert!(maps(namespace), "{namespace}");
    }
    // Not a prefix match, and not the cloud's verbs.
    assert!(!maps("page.measur"));
    assert!(!maps("forms.submit"), "submits reach the actions plugin's handler");
}

// ── answers ───────────────────────────────────────────────────────────────

#[test]
fn a_write_answers_its_result_and_hands_back_its_events() {
    let single = r#"{"result":{"result":{"metadata":{"title":"T"}},"mutationSequence":1,"event":{"type":"metadata.updated","metadata":{"title":"T"}}}}"#;
    let decoded = decode(&Decode::Mutation, answer(single)).unwrap();
    assert_eq!(decoded.value, json!({ "metadata": { "title": "T" } }));
    assert_eq!(decoded.events.len(), 1);
    assert_eq!(decoded.events[0]["type"], "metadata.updated");
    let many = r#"{"result":{"result":{"items":[]},"replayed":false,"mutationSequence":2,"events":[{"type":"a"},{"type":"b"}]}}"#;
    let decoded = decode(&Decode::Mutation, answer(many)).unwrap();
    assert_eq!(
        decoded.events.iter().map(|e| e["type"].clone()).collect::<Vec<_>>(),
        vec![json!("a"), json!("b")]
    );
}

#[test]
fn a_notice_beside_a_result_that_is_not_a_write_is_handed_back() {
    // `signatures.cancel` answers `{result}` and the host's
    // `signatures.cancelled` notice beside it, not inside a write's result.
    let json = r#"{"result":{"result":{"status":"cancelled"}},"event":{"type":"signatures.cancelled","signingId":"s"}}"#;
    let decoded = decode(&Decode::Inner, answer(json)).unwrap();
    assert_eq!(decoded.value, json!({ "status": "cancelled" }));
    assert_eq!(decoded.events, vec![json!({ "type": "signatures.cancelled", "signingId": "s" })]);
}

#[test]
fn prepare_answers_its_digest_from_its_payload_beside_the_notice() {
    // `signatures.prepare` (`host.rs`, `answer_with_notice`): the digest is
    // payload 0, and `signatures.prepared` rides beside the result.
    let response = HostResponse {
        json: r#"{"result":{"result":{"signingId":"s","digest":{"payload":0},"digestAlgorithm":"sha256"}},"event":{"type":"signatures.prepared","signingId":"s"}}"#.into(),
        payloads: vec![vec![1, 2, 3]],
    };
    let decoded = decode(&Decode::Inner, response).unwrap();
    assert_eq!(decoded.value["signingId"], "s");
    assert_eq!(decoded.value["digestAlgorithm"], "sha256");
    assert_eq!(decoded.value["digest"], epdf_kernel::bytes::Bytes(vec![1, 2, 3]).to_json());
    assert_eq!(decoded.events.len(), 1);
    assert_eq!(decoded.events[0]["type"], "signatures.prepared");
}

#[test]
fn a_read_answers_its_member_and_a_missing_one_is_an_error() {
    let decoded =
        decode(&Decode::Field("metadata"), answer(r#"{"result":{"metadata":{"title":"T"}}}"#))
            .unwrap();
    assert_eq!(decoded.value, json!({ "title": "T" }));
    assert!(decode(&Decode::Field("metadata"), answer(r#"{"result":{"title":"T"}}"#)).is_err());
    let only =
        decode(&Decode::Only("viewports"), answer(r#"{"result":{"page":{},"viewports":[]}}"#))
            .unwrap();
    assert_eq!(only.value, json!({ "viewports": [] }));
}

#[test]
fn a_host_error_is_an_engine_error_with_its_details() {
    let json = r#"{"error":{"code":"InvalidArg","message":"bad","details":{"field":"count"}}}"#;
    let error = decode(&Decode::Result, answer(json)).err().unwrap();
    assert_eq!(error.code(), Some("InvalidArg"));
    assert_eq!(error.member("details"), Some(&json!({ "field": "count" })));
}

#[test]
fn payloads_in_an_answer_come_back_as_bytes() {
    let response = HostResponse {
        json: r#"{"result":{"bundle":{"format":"x","resources":{"r0":{"payload":0}}}}}"#.into(),
        payloads: vec![vec![7, 8]],
    };
    let bundle = decode(&Decode::Field("bundle"), response).unwrap().value;
    assert_eq!(bundle["resources"]["r0"], epdf_kernel::bytes::Bytes(vec![7, 8]).to_json());
}

#[test]
fn a_raster_decodes_to_the_raw_pixels_the_render_plugin_reads() {
    let response = HostResponse {
        json: r#"{"result":{"page":{"kind":"objectNumber","objectNumber":12},"area":{"x":0,"y":0,"width":2,"height":1},"raster":{"width":2,"height":1,"stride":8,"color":"rgba8","premultipliedAlpha":false,"data":{"payload":0}}}}"#.into(),
        payloads: vec![vec![9; 8]],
    };
    let image = decode(&Decode::Raster, response).unwrap().value;
    assert_eq!(image["width"], 2);
    assert_eq!(image["source"], json!({ "kind": "pixels", "$byteLength": 8 }));
    let id = image["$host"].as_u64().unwrap();
    assert_eq!(HostTable::take(id), Some(vec![9; 8]));
    // Pixels that do not fill the raster are an error.
    let short = HostResponse {
        json: r#"{"result":{"page":{},"area":{},"raster":{"width":2,"height":1,"stride":8,"data":{"payload":0}}}}"#.into(),
        payloads: vec![vec![0; 4]],
    };
    assert!(decode(&Decode::Raster, short).is_err());
}

// ── against the host ──────────────────────────────────────────────────────

#[test]
fn reads_answer_as_their_services_return() {
    let mut host = host_with(test_support::REPORT);
    let list = run(&mut host, "pages.list", None, &[]).unwrap().value;
    assert_eq!(list["pages"].as_array().map_or(0, Vec::len), 14);
    let page = first_page(&mut host);
    let metadata = run(&mut host, "metadata.get", None, &[]).unwrap().value;
    assert!(metadata.get("createdAt").is_some() && metadata.get("custom").is_none(), "{metadata}");
    let custom = run(&mut host, "metadata.custom.get", None, &[]).unwrap().value;
    assert!(custom.is_object(), "{custom}");
    let actions = run(&mut host, "actions.get", None, &[]).unwrap().value;
    assert!(actions.get("nameTreeScripts").is_some(), "{actions}");
    let outline = run(&mut host, "outline.get", None, &[]).unwrap().value;
    assert!(outline.get("items").is_some(), "{outline}");
    let attachments = run(&mut host, "attachments.list", None, &[]).unwrap().value;
    assert!(attachments["attachments"].is_array(), "{attachments}");
    let text = run(&mut host, "page.text.get", Some(page), &[]).unwrap().value;
    assert!(text.get("text").is_some(), "{text}");
    // `page.text.layout`: the page's geometry for `createTextLayout`.
    let layout = run(&mut host, "page.text.layout", Some(page), &[]).unwrap().value;
    assert!(layout["$textLayout"]["runs"].is_array(), "{layout}");
    let listing = run(&mut host, "page.annotations.list", Some(page), &[]).unwrap().value;
    assert!(listing["annotations"].is_array(), "{listing}");
    let all = run(&mut host, "annotations.list", None, &[json!({})]).unwrap().value;
    assert!(all["annotations"].is_array(), "{all}");
    let viewports = run(&mut host, "page.measure.listViewports", Some(page), &[]).unwrap().value;
    assert_eq!(viewports, json!({ "viewports": [] }));
    let piece = run(&mut host, "pieceInfo.get", None, &[json!("App")]).unwrap().value;
    assert_eq!(piece, Value::Null);
    let version = run(&mut host, "document.version", None, &[]).unwrap().value;
    assert!(version.get("sha256").is_some(), "{version}");
}

#[test]
fn a_search_answers_its_slice() {
    let mut host = host_with(test_support::REPORT);
    let request = json!({ "text": "Report", "snippets": true });
    let slice = run(&mut host, "search.query", None, &[request]).unwrap().value;
    let matches = slice["matches"].as_array().expect("matches");
    assert!(!matches.is_empty(), "{slice}");
    assert_eq!(matches[0]["page"]["kind"], "objectNumber");
    // Named quads (`@embedpdf/geometry`'s `Quad`).
    assert!(matches[0]["segments"][0]["quad"].get("upperLeft").is_some(), "{slice}");
}

#[test]
fn a_metadata_write_answers_its_result_and_its_event() {
    let mut host = host_with(test_support::REPORT);
    let decoded = run(&mut host, "metadata.update", None, &[json!({ "title": "Native" })]).unwrap();
    assert_eq!(decoded.value["metadata"]["title"], "Native");
    assert_eq!(decoded.events.len(), 1);
    assert_eq!(decoded.events[0]["type"], "metadata.updated");
    let custom =
        run(&mut host, "metadata.custom.update", None, &[json!({ "Department": "Legal" })])
            .unwrap();
    assert_eq!(custom.value["custom"]["Department"], "Legal");
    assert_eq!(custom.events[0]["type"], "metadata.customUpdated");
    assert_eq!(run(&mut host, "metadata.get", None, &[]).unwrap().value["title"], "Native");
}

#[test]
fn a_page_render_answers_raw_pixels() {
    let mut host = host_with(test_support::REPORT);
    let page = first_page(&mut host);
    let options = json!({ "viewport": { "kind": "scale", "scale": 0.25 }, "format": "png" });
    let image = run(&mut host, "page.render.image", Some(page), &[options]).unwrap().value;
    assert_eq!(image["source"]["kind"], "pixels");
    let id = image["$host"].as_u64().unwrap();
    assert!(HostTable::take(id).is_some_and(|pixels| !pixels.is_empty()));
}

#[test]
fn an_apply_runs_a_page_change_and_answers_its_items_and_events() {
    let mut host = host_with(test_support::REPORT);
    let page = first_page(&mut host);
    let change = json!({ "ops": [{ "type": "pages.rotate", "pages": [{ "kind": "objectNumber", "objectNumber": page as f64 }], "rotation": 90.0 }] });
    let decoded = run(&mut host, "apply", None, &[change]).unwrap();
    assert_eq!(decoded.value["items"][0]["type"], "pages.rotate");
    assert_eq!(decoded.events[0]["type"], "pages.rotated");
    assert!(decoded.events[0]["origin"]["tx"].is_object(), "{}", decoded.events[0]);
    let list = run(&mut host, "pages.list", None, &[]).unwrap().value;
    assert_eq!(list["pages"][0]["rotation"], 90);
}

#[test]
fn an_annotation_create_carries_its_draft_and_answers_its_annotation() {
    let mut host = host_with(test_support::REPORT);
    let page = first_page(&mut host);
    let draft = json!({ "subtype": "square", "box": { "x": 10, "y": 20, "width": 30, "height": 40 }, "color": "#ff0000" });
    let decoded = run(&mut host, "page.annotations.create", Some(page), &[draft]).unwrap();
    assert_eq!(decoded.value["annotation"]["subtype"], "square");
    assert_eq!(decoded.events[0]["type"], "annotations.created");
    let listed = run(&mut host, "page.annotations.list", Some(page), &[]).unwrap().value;
    assert!(listed["annotations"].as_array().unwrap().iter().any(|a| a["subtype"] == "square"));
}

#[test]
fn a_scale_write_and_its_clearing_reach_the_page() {
    let mut host = host_with(test_support::REPORT);
    let page = first_page(&mut host);
    // `PdfMeasure | null`: clearing sends an explicit `null`.
    let call = translate("d", "page.measure.setScale", Some(page), &[], 2, &client()).unwrap();
    assert_eq!(call.request["measure"], Value::Null);
    let decoded = run(&mut host, "page.measure.setScale", Some(page), &[Value::Null]).unwrap();
    assert_eq!(decoded.events[0]["type"], "pages.scaleSet", "{:?}", decoded.events);
}

#[test]
fn a_form_reads_exports_and_imports_its_bundle() {
    let mut host = host_with(FORM_FIELDS);
    let form = run(&mut host, "forms.list", None, &[]).unwrap().value;
    assert!(form["fields"].as_array().is_some_and(|f| !f.is_empty()), "{form}");
    assert!(form["widgets"].is_array(), "{form}");
    let bundle = run(&mut host, "forms.export", None, &[json!({})]).unwrap().value;
    assert!(bundle.get("resources").is_some(), "{bundle}");
    let imported = run(&mut host, "forms.import", None, &[bundle, json!({})]).unwrap();
    assert!(imported.value.is_object(), "{}", imported.value);
    let repaired =
        run(&mut host, "forms.repair", None, &[json!({ "bakeAppearances": false })]).unwrap();
    assert!(repaired.value.is_object(), "{}", repaired.value);
}

#[test]
fn an_extract_and_a_download_park_their_file_in_the_host_table() {
    let mut host = host_with(test_support::REPORT);
    let page = first_page(&mut host);
    let pages = json!([{ "kind": "objectNumber", "objectNumber": page }]);
    let extracted = run(&mut host, "pages.extract", None, &[pages]).unwrap().value;
    let bytes = epdf_kernel::bytes::Bytes::from_json(&extracted).map(|b| b.0);
    let id = extracted["$hostBytes"].as_u64().unwrap();
    assert!(HostTable::take(id).is_some_and(|pdf| pdf.starts_with(b"%PDF")));
    drop(bytes);
    let saved = run(&mut host, "download", None, &[]).unwrap().value;
    let id = saved["$hostBytes"].as_u64().unwrap();
    assert!(HostTable::take(id).is_some_and(|pdf| pdf.starts_with(b"%PDF")));
}

#[test]
fn named_pages_and_piece_info_write_on_their_holders() {
    let mut host = host_with(test_support::REPORT);
    let page = first_page(&mut host);
    let named = run(
        &mut host,
        "pages.setName",
        None,
        &[json!({ "name": "cover", "page": { "kind": "objectNumber", "objectNumber": page } })],
    )
    .unwrap();
    assert!(named.value.is_object(), "{}", named.value);
    let removed = run(&mut host, "pages.removeName", None, &[json!({ "name": "cover" })]).unwrap();
    assert!(removed.value.is_object(), "{}", removed.value);
    let patch = json!({ "Private": "x" });
    let updated =
        run(&mut host, "page.pieceInfo.update", Some(page), &[json!("App"), patch]).unwrap();
    assert!(updated.value.get("pieceInfo").is_some(), "{}", updated.value);
    assert!(updated.events.is_empty(), "pieceInfo publishes nothing");
    let read = run(&mut host, "page.pieceInfo.get", Some(page), &[json!("App")]).unwrap().value;
    assert!(!read.is_null(), "{read}");
    assert_eq!(run(&mut host, "pieceInfo.get", None, &[json!("App")]).unwrap().value, Value::Null);
}

#[test]
fn signature_reads_and_a_visual_fill_reach_the_host() {
    let mut host = host_with(&format!("{ENGINE_FIXTURES}/unsigned_sigfield.pdf"));
    let snapshot = run(&mut host, "signatures.list", None, &[]).unwrap().value;
    assert!(snapshot.get("signatures").is_some(), "{snapshot}");
    let artwork = test_support::fixture(&format!("{ENGINE_FIXTURES}/signature_artwork.pdf"));
    let field = json!({ "kind": "fqn", "name": "sig" });
    let appearance = json!({ "pdf": epdf_kernel::bytes::Bytes(artwork).to_json() });
    let call = translate(
        "d",
        "forms.setSignatureAppearance",
        None,
        &[field.clone(), appearance.clone()],
        2,
        &client(),
    )
    .unwrap();
    assert_eq!(call.payloads.len(), 1, "the PDF is payload 0");
    assert_eq!(call.request["authority"], client().authority);
    let decoded =
        run(&mut host, "forms.setSignatureAppearance", None, &[field, appearance]).unwrap();
    assert_eq!(decoded.value["field"]["name"], "sig", "{}", decoded.value);
    assert_eq!(decoded.events[0]["type"], "forms.updated");
    // A cancel of nothing parked answers its status and no notice.
    let cancel = run(&mut host, "signatures.cancel", None, &[json!("nothing-parked")]).unwrap();
    assert_eq!(cancel.value["status"], "unknown");
    assert!(cancel.events.is_empty());
}

#[test]
fn complete_moves_its_cms_into_payload_zero() {
    let input = json!({ "signingId": "s", "cms": { "$bytes": "MAA=" }, "expectedVersion": { "editsVersion": 0.0 } });
    let call = translate("d", "signatures.complete", None, &[input], 1, &client()).unwrap();
    assert_eq!(call.payloads, vec![vec![0x30, 0]]);
    assert!(call.request["input"].get("cms").is_none());
    assert_eq!(call.request["input"]["expectedVersion"]["editsVersion"], 0);
    assert!(call.request["opId"].is_string());
    assert_eq!(call.request["effect"], "contentWrite");
}

#[test]
fn an_encoded_render_on_the_asset_engine_asks_for_png() {
    let call = epdf_viewer_host::translate::translate_encoded_render(
        "d",
        Some(3),
        &[json!({ "format": "png" })],
        1,
    )
    .unwrap();
    assert_eq!(call.request["kind"], "pages.renderEncoded");
    assert_eq!(call.request["encode"], json!({ "format": "png" }));
    let error = epdf_viewer_host::translate::translate_encoded_render(
        "d",
        Some(3),
        &[json!({ "format": "webp" })],
        1,
    )
    .err()
    .unwrap();
    assert_eq!(error.code(), Some("unsupported"));
}

// ── the engine trait over real PDFium ─────────────────────────────────────

/// Run `job` on a kernel thread over an engine on `worker`, and wait for what
/// it sends.
fn on_kernel<T: Send + 'static>(
    worker: EngineWorker,
    job: impl FnOnce(&NativeEngine, oneshot::Sender<T>) + Send + 'static,
) -> T {
    let thread = KernelThread::spawn(|post| Ok(NativeEngine::new(worker, post, None))).unwrap();
    let answer = futures::executor::block_on(thread.call(move |engine: &mut NativeEngine| {
        let (tx, rx) = oneshot::channel();
        job(engine, tx);
        rx
    }))
    .unwrap();
    futures::executor::block_on(answer).unwrap()
}

fn open_input(fixture: &str) -> Value {
    json!({ "$host": HostTable::put(test_support::fixture(fixture)) })
}

/// Open `fixture` and hand the document to `then`.
fn with_document<T: Send + 'static>(
    fixture: &'static str,
    then: impl FnOnce(Rc<dyn DocumentHandle>, oneshot::Sender<T>) + 'static + Send,
) -> T {
    on_kernel(EngineWorker::spawn(), move |engine, tx| {
        assert!(engine.is_local(), "the native engine is the local engine");
        engine.open(open_input(fixture), None).on_settle(move |doc| {
            then(doc.as_ref().ok().cloned().expect("opened"), tx);
        });
    })
}

#[test]
fn the_native_engine_opens_a_pdf_and_lists_its_pages() {
    let (list, text, index, missing, local) = with_document(test_support::REPORT, |doc, tx| {
        doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
            let list = list.as_ref().unwrap().clone();
            let first = list["pages"][0]["ref"]["objectNumber"].as_u64().unwrap() as u32;
            let index = doc.page_index(first);
            let missing = doc.page_index(u32::MAX).err().and_then(|e| e.code().map(str::to_owned));
            let local = doc.is_local();
            doc.call("page.text.get", Some(first), vec![]).on_settle(move |text| {
                let _ = tx.send((list, text.clone(), index, missing, local));
            });
        });
    });
    assert_eq!(list["pages"].as_array().map_or(0, Vec::len), 14);
    assert!(text.is_ok(), "a page read reaches the host: {text:?}");
    assert_eq!(index.ok(), Some(0));
    assert_eq!(missing.as_deref(), Some("NotFound"));
    assert!(local);
}

#[test]
fn a_write_publishes_its_event_before_it_settles() {
    let (result, events) = with_document(test_support::REPORT, |doc, tx| {
        let events = Rc::new(RefCell::new(Vec::<Value>::new()));
        let seen = events.clone();
        let off =
            doc.subscribe(Rc::new(move |event: &Value| seen.borrow_mut().push(event.clone())));
        doc.call("metadata.update", None, vec![json!({ "title": "Native" })]).on_settle(
            move |result| {
                let delivered = events.borrow().clone();
                off.call();
                let _ = tx.send((result.clone(), delivered));
            },
        );
    });
    assert_eq!(result.unwrap()["metadata"]["title"], "Native");
    assert_eq!(events.len(), 1, "one event, delivered before the call resolved");
    assert_eq!(events[0]["type"], "metadata.updated");
}

#[test]
fn the_open_reserves_object_numbers_and_take_tops_the_pool_up() {
    // `LocalObjectNumberPool`: 64 numbers as the document opens.
    let (held, taken, after) = with_document(test_support::REPORT, |doc, tx| {
        let held = doc.held_object_numbers();
        let taken: Vec<Option<u32>> = (0..40).map(|_| doc.take_object_number().unwrap()).collect();
        let after = doc.held_object_numbers();
        let _ = tx.send((held, taken, after));
    });
    assert_eq!(held, 64);
    assert!(taken.iter().all(Option::is_some));
    let numbers: Vec<u32> = taken.into_iter().flatten().collect();
    assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1), "{numbers:?}");
    assert_eq!(after, 24);
}

#[test]
fn reserve_asks_only_when_the_pool_is_short() {
    let (small, big, held, zero) = with_document(test_support::REPORT, |doc, tx| {
        let small = doc.reserve_object_numbers(10);
        let small_settled = small.peek().map(|r| r.is_ok());
        let big = doc.reserve_object_numbers(100);
        let zero = doc
            .reserve_object_numbers(0)
            .peek()
            .and_then(|r| r.err())
            .and_then(|e| e.code().map(str::to_owned));
        let doc2 = doc.clone();
        big.on_settle(move |result| {
            let _ = tx.send((small_settled, result.is_ok(), doc2.held_object_numbers(), zero));
        });
    });
    assert_eq!(small, Some(true), "already held: no request");
    assert!(big);
    assert!(held >= 100, "{held}");
    assert_eq!(zero.as_deref(), Some("InvalidArg"));
}

#[test]
fn a_page_verb_is_one_apply_and_answers_its_items_members() {
    // `pageVerbs` (`engine/pageVerbs.ts`): `rotate` answers `{layout, meta}`.
    let (answer, events) = with_document(test_support::REPORT, |doc, tx| {
        let events = Rc::new(RefCell::new(Vec::<Value>::new()));
        let seen = events.clone();
        let _off = doc
            .subscribe(Rc::new(move |event: &Value| seen.borrow_mut().push(event["type"].clone())));
        let first = json!({ "kind": "objectNumber", "objectNumber": 3 });
        let list = doc.clone().call("pages.list", None, vec![]);
        let doc2 = doc.clone();
        list.on_settle(move |list| {
            let first = list.as_ref().map(|l| l["pages"][0]["ref"].clone()).unwrap_or(first);
            doc2.call("pages.rotate", None, vec![json!([first]), json!(90)]).on_settle(
                move |answer| {
                    let _ = tx.send((answer.clone(), events.borrow().clone()));
                },
            );
        });
    });
    let answer = answer.unwrap();
    assert!(answer.get("layout").is_some() && answer.get("meta").is_some(), "{answer}");
    assert_eq!(answer.as_object().unwrap().len(), 2, "{answer}");
    assert_eq!(events, vec![json!("pages.rotated")]);
}

#[test]
fn the_render_policy_is_the_local_engines_default() {
    let policy = with_document(test_support::REPORT, |doc, tx| {
        doc.call("render.getPolicy", None, vec![]).on_settle(move |p| {
            let _ = tx.send(p.clone().ok());
        });
    });
    assert_eq!(policy, Some(json!({ "kind": "continuous" })));
}

#[test]
fn a_locked_open_parks_and_unlocks_with_its_password() {
    // `LocalEngine.openResult` and `LocalDocumentSecurityService.unlock`: a
    // wrong password at open parks the document with the prompt's
    // `incorrect`; a wrong unlock keeps it parked and incorrect; the right
    // one loads it.
    let (opened, after_wrong, after_right, pages) =
        on_kernel(EngineWorker::spawn(), |engine, tx| {
            let options = json!({ "password": "wrong" });
            engine.open(open_input(test_support::ENCRYPTED), Some(options)).on_settle(move |doc| {
                let doc = doc.as_ref().ok().cloned().expect("a parked open still opens");
                let security = doc.security();
                let opened = security.password_prompt();
                let doc2 = doc.clone();
                security.unlock(json!({ "password": "nope" })).on_settle(move |wrong| {
                    assert!(wrong.is_err());
                    let security = doc2.security();
                    let after_wrong = security.password_prompt();
                    let doc3 = doc2.clone();
                    security.unlock(json!({ "password": "hôtel" })).on_settle(move |right| {
                        assert!(right.is_ok(), "{right:?}");
                        let after_right = doc3.security().password_prompt();
                        doc3.call("pages.list", None, vec![]).on_settle(move |pages| {
                            let _ = tx.send((opened, after_wrong, after_right, pages.clone().ok()));
                        });
                    });
                });
            });
        });
    // The TypeScript's prompts for the same steps (`passwordPromptFromState`
    // under `LocalEngine`, encrypted-hello.pdf): parked, the permissions
    // are unknown, so no hint; the user password opens it with an owner
    // still to upgrade to.
    assert_eq!(opened, json!({ "state": "required", "hint": null, "incorrect": true }));
    assert_eq!(after_wrong, json!({ "state": "required", "hint": null, "incorrect": true }));
    assert_eq!(after_right, json!({ "state": "optional", "hint": "owner" }));
    assert!(pages.is_some_and(|p| p["pages"].as_array().is_some_and(|p| !p.is_empty())));
}

#[test]
fn the_security_state_is_the_typescripts_state_not_the_probe() {
    // `LocalDocumentSecurityService.state`, `securityStateFromProbe` of the
    // host's probe, as the TypeScript answered it for the same files.
    let plain = with_document(test_support::REPORT, |doc, tx| {
        let _ = tx.send(doc.security().current());
    });
    assert_eq!(
        plain,
        json!({ "encryption": { "state": "none", "requiresPassword": false },
            "permissions": { "known": true, "bits": 4_294_967_295_u32, "allAllowed": true,
                "openedAs": "none", "securityHandlerRevision": null, "canUpgradeToOwner": false },
            "access": { "required": false, "reasons": [] } })
    );
    let (parked, unlocked) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(open_input(test_support::ENCRYPTED), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().expect("a parked open still opens");
            let parked = doc.security().current();
            let doc2 = doc.clone();
            doc.security().unlock(json!({ "password": "hôtel" })).on_settle(move |right| {
                assert!(right.is_ok(), "{right:?}");
                let _ = tx.send((parked, doc2.security().current()));
            });
        });
    });
    assert_eq!(parked["permissions"]["known"], false, "{parked}");
    assert_eq!(parked["access"], json!({ "required": true, "reasons": ["password"] }));
    assert_eq!(
        unlocked,
        json!({ "encryption": { "state": "encrypted", "requiresPassword": false },
            "permissions": { "known": true, "bits": 4_294_967_292_u32, "allAllowed": true,
                "openedAs": "user", "securityHandlerRevision": 6, "canUpgradeToOwner": true },
            "access": { "required": false, "reasons": [] } })
    );
}

/// What a native handle opened under `scope` answers to `calls`, made in
/// order against REPORT's first page: each refusal's `required`, or `ok`.
fn refusals_under(scope: &[&str], calls: Vec<(&'static str, Vec<Value>)>) -> Vec<String> {
    let scope: Vec<String> = scope.iter().map(|s| s.to_string()).collect();
    let worker = EngineWorker::spawn();
    let thread =
        KernelThread::spawn(move |post| Ok(NativeEngine::new(worker, post, Some(scope)))).unwrap();
    let answer = futures::executor::block_on(thread.call(move |engine: &mut NativeEngine| {
        let (tx, rx) = oneshot::channel();
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().expect("opened");
            let doc2 = doc.clone();
            doc.call("pages.list", None, vec![]).on_settle(move |list| {
                let pon = list
                    .as_ref()
                    .ok()
                    .and_then(|l| l["pages"][0]["ref"]["objectNumber"].as_u64())
                    .map(|n| n as u32);
                // A refusal rejects at once, before anything is sent.
                let answers: Vec<String> = calls
                    .into_iter()
                    .map(|(path, args)| {
                        let page = path.starts_with("page.").then_some(pon).flatten();
                        match doc2.call(path, page, args).peek() {
                            Some(Err(e)) => e
                                .member("required")
                                .and_then(Value::as_str)
                                .map_or_else(|| format!("{e:?}"), str::to_owned),
                            _ => "sent".to_owned(),
                        }
                    })
                    .collect();
                let _ = tx.send(answers);
            });
        });
        rx
    }))
    .unwrap();
    futures::executor::block_on(answer).unwrap()
}

#[test]
fn a_native_create_or_import_takes_the_handles_collab_and_group_checks() {
    // `LocalPageAnnotationsService.create` and `assertMayCreate`: another
    // group than the caller's takes `assertSetGroup`, and every create
    // takes `assertCollab('create')`, before the call is sent; a restoring
    // import takes `doc.annotate.modify` and `doc.annotate.import`.
    let square = |group: Value| {
        json!({ "subtype": "square", "box": { "x": 10, "y": 20, "width": 30, "height": 40 },
            "groupId": group })
    };
    let bundle = |group: &str| {
        json!({ "format": "embedpdf/annotations", "version": 1,
            "items": [{ "data": { "subtype": "square", "groupId": group } }], "resources": {} })
    };
    let stamp = json!({ "attribution": "stamp" });
    let read_only = refusals_under(
        &["doc.open", "doc.annotate.read"],
        vec![
            ("page.annotations.create", vec![square(Value::Null)]),
            ("annotations.import", vec![bundle("legal"), Value::Null]),
            ("annotations.import", vec![json!({ "items": [] }), stamp.clone()]),
        ],
    );
    assert_eq!(read_only, ["annotations:create", "doc.annotate.modify", "annotations:create"]);
    let creator = refusals_under(
        &["doc.open", "doc.annotate.read", "annotations:create:all"],
        vec![
            ("page.annotations.create", vec![square(json!("legal"))]),
            ("annotations.import", vec![bundle("legal"), stamp]),
            ("page.annotations.create", vec![square(Value::Null)]),
        ],
    );
    assert_eq!(
        creator,
        ["annotations:set-group:group=legal", "annotations:set-group:group=legal", "sent"]
    );
}

#[test]
fn a_locked_open_without_a_password_is_not_incorrect() {
    let prompt = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(open_input(test_support::ENCRYPTED), None).on_settle(move |doc| {
            let _ = tx.send(doc.as_ref().ok().map(|d| d.security().password_prompt()));
        });
    });
    assert_eq!(prompt, Some(json!({ "state": "required", "hint": null, "incorrect": false })));
}

#[test]
fn an_open_without_bytes_is_invalid_input_and_bytes_json_opens() {
    let (missing, json_open) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        let missing = engine.open(json!({}), None);
        let bytes =
            epdf_kernel::bytes::Bytes(test_support::fixture(test_support::REPORT)).to_json();
        engine.open(json!({ "bytes": bytes }), None).on_settle(move |doc| {
            let code =
                missing.peek().and_then(|r| r.err()).and_then(|e| e.code().map(str::to_owned));
            let _ = tx.send((code, doc.is_ok()));
        });
    });
    assert_eq!(missing.as_deref(), Some("invalid-input"));
    assert!(json_open);
}

#[test]
fn a_working_set_goes_to_the_host_and_answers_nothing() {
    let listed = with_document(test_support::REPORT, |doc, tx| {
        doc.set_working_set(
            "main",
            json!([{ "page": { "kind": "objectNumber", "objectNumber": 3.0 }, "distance": 0.0 }]),
        );
        // The host keeps answering in order after the hint.
        doc.call("pages.list", None, vec![]).on_settle(move |list| {
            let _ = tx.send(list.is_ok());
        });
    });
    assert!(listed);
}

#[test]
fn an_annotation_listing_of_the_kinds_fixture_names_its_refs() {
    let mut host = host_with(ANNOTATION_KINDS);
    let all = run(&mut host, "annotations.list", None, &[json!({})]).unwrap().value;
    let annotations = all["annotations"].as_array().unwrap();
    assert!(!annotations.is_empty());
    for annotation in annotations {
        let kind = annotation["ref"]["kind"].as_str().unwrap();
        assert!(matches!(kind, "objectNumber" | "baseIndex"), "{annotation}");
    }
}

#[test]
fn appearance_pictures_come_back_as_raw_pixels() {
    // `renderAppearanceRasters` (`document/appearanceBatch.ts`): one raster
    // per appearance of the family on the page.
    let mut host = host_with(ANNOTATION_KINDS);
    let page = first_page(&mut host);
    let options = json!({ "viewport": { "kind": "scale", "scale": 1 }, "modes": ["normal"] });
    let call = translate(
        "d",
        "page.annotations.renderAppearances",
        Some(page),
        std::slice::from_ref(&options),
        1,
        &client(),
    )
    .unwrap();
    assert_eq!(call.request["family"], "annotations");
    let forms = translate(
        "d",
        "page.forms.renderAppearances",
        Some(page),
        std::slice::from_ref(&options),
        1,
        &client(),
    )
    .unwrap();
    assert_eq!(forms.request["family"], "widgets");
    let result =
        run(&mut host, "page.annotations.renderAppearances", Some(page), &[options]).unwrap().value;
    let appearances = result["appearances"].as_array().expect("appearances");
    assert!(!appearances.is_empty(), "{result}");
    for appearance in appearances {
        assert!(appearance["ref"].is_object(), "{appearance}");
        assert_eq!(appearance["image"]["source"]["kind"], "pixels", "{appearance}");
        let id = appearance["image"]["$host"].as_u64().unwrap();
        assert!(HostTable::take(id).is_some());
    }
}
