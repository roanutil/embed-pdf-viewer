use std::cell::RefCell;
use std::rc::Rc;

use epdf_engine::host::HostResponse;
use epdf_ffi::KernelThread;
use epdf_kernel::engine::Engine;
use epdf_viewer_host::engine_worker::EngineWorker;
use epdf_viewer_host::host_table::HostTable;
use epdf_viewer_host::native_engine::NativeEngine;
use epdf_viewer_host::translate::{Decode, decode, translate};
use futures::channel::oneshot;
use serde_json::{Value, json};

#[test]
fn metadata_read_becomes_its_wire_kind() {
    let call = translate("d1", "metadata.read", None, &[], 7).unwrap();
    assert_eq!(call.request, json!({"kind": "metadata.read", "jobId": 7, "docId": "d1"}));
    assert!(matches!(call.decode, Decode::Result));
}

#[test]
fn a_page_chain_carries_its_page() {
    let call = translate("d1", "page.render.image", Some(12), &[json!({"scale": 2})], 8).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.render", "jobId": 8, "docId": "d1",
            "page": {"kind": "objectNumber", "objectNumber": 12},
            "options": {"scale": 2}
        })
    );
    assert!(matches!(call.decode, Decode::Raster));
}

#[test]
fn a_page_chain_without_a_page_is_invalid_input() {
    let error = translate("d1", "page.text.read", None, &[], 9).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_unmapped_path_is_unsupported_not_a_panic() {
    let error = translate("d1", "scripts.compile", None, &[], 10).err().unwrap();
    assert_eq!(error.code(), Some("unsupported"));
}

// ── decoding, against envelopes the host emitted for the report fixture ──

fn answer(json: &str) -> HostResponse {
    HostResponse { json: json.into(), payloads: Vec::new() }
}

const METADATA: &str = r#"{"author":null,"created":"2009-04-01T23:39:25.000Z","creator":"TeX","custom":{},"keywords":null,"modified":null,"producer":"pdfeTeX-1.21a","subject":null,"title":"T","trapped":"unknown"}"#;

#[test]
fn a_mutation_answers_its_inner_result_and_hands_back_its_event() {
    let json = format!(
        r#"{{"result":{{"event":{{"metadata":{METADATA},"origin":{{"kind":"local","serverId":null,"sessionId":"sess_1","sub":null,"ts":1790879896560}},"type":"metadata.updated"}},"mutationSequence":1,"result":{{"metadata":{METADATA}}}}}}}"#
    );
    let decoded = decode(&Decode::Mutation, answer(&json)).unwrap();
    let metadata: Value = serde_json::from_str(METADATA).unwrap();
    assert_eq!(decoded.value, json!({ "metadata": metadata }));
    let event = decoded.event.unwrap();
    assert_eq!(event["type"], "metadata.updated");
    assert_eq!(event["metadata"], metadata);
}

#[test]
fn a_page_list_answers_each_page_by_its_ref() {
    let json = r#"{"result":{"pageCount":1,"pages":[{"boxes":{"crop":{"bottom":0.0,"left":0.0,"right":612.0,"top":792.0},"media":{"bottom":0.0,"left":0.0,"right":612.0,"top":792.0}},"index":0,"label":null,"ref":{"kind":"objectNumber","objectNumber":2},"rotation":0,"size":{"height":792.0,"width":612.0},"userUnit":1.0}]}}"#;
    let page = &decode(&Decode::PageList, answer(json)).unwrap().value["pages"][0];
    assert_eq!(page["ref"], json!({"kind": "objectNumber", "pageObjectNumber": 2}));
    assert!(page.get("pageObjectNumber").is_none(), "{page}");
    assert_eq!(page["size"], json!({"height": 792.0, "width": 612.0}));
}

#[test]
fn a_page_without_its_ref_is_an_error() {
    // The flat spelling the host answered before `9f13cea3c`.
    let json = r#"{"result":{"pageCount":1,"pages":[{"index":0,"pageObjectNumber":2}]}}"#;
    assert!(decode(&Decode::PageList, answer(json)).is_err());
}

#[test]
fn a_read_answers_the_bare_result() {
    let json = format!(r#"{{"result":{METADATA}}}"#);
    let decoded = decode(&Decode::Result, answer(&json)).unwrap();
    assert_eq!(decoded.value["title"], "T");
    assert!(decoded.event.is_none());
}

#[test]
fn a_missing_member_is_an_error_not_the_whole_result() {
    let json = format!(r#"{{"result":{{"metadata":{METADATA}}}}}"#);
    assert!(decode(&Decode::Mutation, answer(&json)).is_err());
    assert!(decode(&Decode::Field("security"), answer(&json)).is_err());
    assert!(decode(&Decode::PageList, answer(r#"{"result":{"pageCount":0}}"#)).is_err());
    assert!(decode(&Decode::Result, answer("{}")).is_err());
}

#[test]
fn a_search_request_goes_flat_with_its_start_page_as_a_whole_page_ref() {
    // The plugin's `{query, mode, startPage, budget}` is the host's flattened
    // `SearchScanRequest` (upstream's a10ed2659): the query at the top
    // level, `snippets` for the full mode, `from` and `limit`.
    let request = json!({
        "query": {"text": "the", "matchCase": true},
        "mode": "full",
        "startPage": {"kind": "objectNumber", "pageObjectNumber": 43.0},
        "cursor": "c",
        "skip": 2.0,
        "budget": {"maxPages": 3.0, "maxMatches": 7.0}
    });
    let call = translate("d1", "search.query", None, &[request], 11).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "search.query", "jobId": 11, "docId": "d1",
            "request": {"text": "the", "matchCase": true, "snippets": true,
                "from": {"kind": "objectNumber", "objectNumber": 43},
                "cursor": "c", "skip": 2.0, "limit": {"pages": 3.0, "matches": 7.0}}
        })
    );
    assert!(call.request["request"]["from"]["objectNumber"].is_u64());
    assert!(matches!(call.decode, Decode::SearchSlice));

    let rects = json!({"query": {"text": "the"}, "mode": "rects"});
    let call = translate("d1", "search.query", None, std::slice::from_ref(&rects), 12).unwrap();
    assert_eq!(call.request["request"], json!({"text": "the", "snippets": false}));
    let plain = json!({"query": {"text": "the"}});
    let call = translate("d1", "search.query", None, std::slice::from_ref(&plain), 13).unwrap();
    assert_eq!(call.request["request"], json!({"text": "the", "snippets": true}));
}

#[test]
fn a_search_start_page_that_is_not_a_page_ref_is_invalid_input() {
    let request = json!({"query": {"text": "the"}, "startPage": {"index": 1}});
    let error = translate("d1", "search.query", None, &[request], 13).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
    let error =
        translate("d1", "search.query", None, &[json!({"mode": "full"})], 14).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn a_search_mode_other_than_full_or_rects_is_invalid_input() {
    // Reading an unknown mode as full would hand snippets to a caller that
    // meant the rects tier.
    for mode in [json!("Rects"), json!("snippets"), json!(1), json!(true)] {
        let request = json!({"query": {"text": "the"}, "mode": mode});
        let error = translate("d1", "search.query", None, &[request], 15).err().unwrap();
        assert_eq!(error.code(), Some("invalid-input"), "{mode}");
    }
}

/// The `search.query` answer for "Report" over the report fixture, as
/// `call_host` hands it to `decode`: the kernel's spelling, in the file's
/// coordinates. Recorded from this reader after 878f58585 gave it
/// upstream's loose boxes.
const SEARCH_SLICE: &str = r#"{"result":{"matches":[{"page":{"kind":"objectNumber","objectNumber":202},"charStart":6156,"charCount":6,"segments":[{"quad":{"p1":{"x":354.4305114746094,"y":194.0742950439453},"p2":{"x":375.85125732421875,"y":194.0742950439453},"p3":{"x":354.4305114746094,"y":186.05833435058594},"p4":{"x":375.85125732421875,"y":186.05833435058594}},"rect":{"left":354.4305114746094,"bottom":186.05833435058594,"right":375.85125732421875,"top":194.0742950439453},"advance":1}],"snippet":{"text":"then loads and runs each program 10  times and reports the average time taken by each. We ran 4","matchStart":47,"matchLength":6}}],"nextCursor":null,"scannedPages":14,"totalPages":14}}"#;

#[test]
fn a_search_slice_answers_each_hit_with_its_page() {
    let slice = decode(&Decode::SearchSlice, answer(SEARCH_SLICE)).unwrap().value;
    let hit = &slice["matches"][0];
    assert_eq!(hit["page"], json!({"kind": "objectNumber", "pageObjectNumber": 202}));
    assert_eq!(hit["charStart"], 6156);
    assert_eq!(hit["snippet"]["matchStart"], 47);
    assert_eq!(slice["nextCursor"], Value::Null);
    assert_eq!(slice["totalPages"], 14);
}

#[test]
fn a_search_answer_reaches_the_kernel_in_the_files_coordinates_and_its_spelling() {
    // The host answers upstream's `SearchSlice` in page space (`start`,
    // `count`, named corners, a three-part snippet, `pagesSearched`);
    // `call_host` measures each segment back on its page and respells the
    // slice, so the kernel reads what it read before.
    use epdf_viewer_host::translate::call_host;

    let mut host = epdf_engine::host::Host::new();
    let open = json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d1" });
    let opened =
        call_host(&mut host, &open.to_string(), vec![test_support::fixture(test_support::REPORT)]);
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);

    let request = json!({ "query": { "text": "Report" }, "mode": "full" });
    let call = translate("d1", "search.query", None, &[request], 2).unwrap();
    let wire: Value =
        serde_json::from_str(&host.call(&call.request.to_string(), Vec::new()).json).unwrap();
    let hit = &wire["result"]["matches"][0];
    assert_eq!((&hit["start"], &hit["count"]), (&json!(6156), &json!(6)), "{wire}");
    assert_eq!(hit["snippet"]["match"], "report");
    assert!(hit["segments"][0]["quad"]["upperLeft"].is_object(), "{hit}");
    assert_eq!(wire["result"]["pageCount"], 14);

    let response = call_host(&mut host, &call.request.to_string(), Vec::new());
    let slice = decode(&call.decode, response).unwrap().value;

    let expected = decode(&Decode::SearchSlice, answer(SEARCH_SLICE)).unwrap().value;
    assert_eq!(slice, expected);
}

#[test]
fn a_search_hit_without_its_page_is_an_error() {
    let json = r#"{"result":{"matches":[{"charCount":1,"charStart":0,"segments":[]}],"nextCursor":null,"scannedPages":1,"totalPages":1}}"#;
    assert!(decode(&Decode::SearchSlice, answer(json)).is_err());
    let flat = r#"{"result":{"matches":[{"charCount":1,"charStart":0,"pageObjectNumber":3,"segments":[]}],"nextCursor":null,"scannedPages":1,"totalPages":1}}"#;
    assert!(decode(&Decode::SearchSlice, answer(flat)).is_err());
    assert!(decode(&Decode::SearchSlice, answer(r#"{"result":{"nextCursor":null}}"#)).is_err());
}

#[test]
fn a_render_request_keeps_the_viewport_and_drops_the_encode_options() {
    // What the render plugin sends: the host renders raw pixels on
    // `pages.render` and reads neither `format` nor `quality`.
    let options = json!({
        "viewport": {"kind": "width", "width": 612.0},
        "includeAnnotations": false,
        "format": "png",
        "quality": 0.8
    });
    let call = translate("d1", "page.render.image", Some(2), &[options], 14).unwrap();
    assert_eq!(
        call.request["options"],
        json!({"viewport": {"kind": "width", "width": 612.0}, "includeAnnotations": false})
    );
}

fn raster(json: &str, pixels: Vec<u8>) -> HostResponse {
    HostResponse { json: json.into(), payloads: vec![pixels] }
}

#[test]
fn a_raster_decodes_to_the_raw_pixels_the_render_plugin_reads() {
    let pixels = vec![255, 0, 0, 255, 0, 0, 255, 255];
    let decoded = decode(
        &Decode::Raster,
        raster(r#"{"result":{"width":2,"height":1,"stride":8}}"#, pixels.clone()),
    )
    .unwrap();
    let id = decoded.value["$host"].as_u64().unwrap();
    assert_eq!(
        decoded.value,
        json!({
            "width": 2, "height": 1, "stride": 8,
            "color": "rgba8", "premultipliedAlpha": false,
            "source": {"kind": "pixels", "$byteLength": 8},
            "$host": id
        })
    );
    assert_eq!(HostTable::get(id).as_deref(), Some(&pixels));
    HostTable::release(id);
}

#[test]
fn a_raster_whose_pixels_do_not_fit_is_an_error() {
    let short = raster(r#"{"result":{"width":2,"height":2,"stride":8}}"#, vec![0; 8]);
    assert!(decode(&Decode::Raster, short).is_err());
    let narrow = raster(r#"{"result":{"width":3,"height":1,"stride":8}}"#, vec![0; 8]);
    assert!(decode(&Decode::Raster, narrow).is_err());
    let no_stride = raster(r#"{"result":{"width":2,"height":1}}"#, vec![0; 8]);
    assert!(decode(&Decode::Raster, no_stride).is_err());
    assert!(
        decode(&Decode::Raster, answer(r#"{"result":{"width":2,"height":1,"stride":8}}"#)).is_err()
    );
}

#[test]
fn a_host_error_is_an_engine_error() {
    let json = r#"{"error":{"code":"DocPasswordRequired","message":"document e is locked and requires a password"}}"#;
    let error = decode(&Decode::PageList, answer(json)).err().unwrap();
    assert_eq!(error.code(), Some("DocPasswordRequired"));
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

#[test]
fn the_native_engine_opens_a_pdf_and_lists_its_pages() {
    let (list, text, index, missing) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
                let list = list.as_ref().unwrap().clone();
                let first = list["pages"][0]["ref"]["pageObjectNumber"].as_u64().unwrap() as u32;
                let index = doc.page_index(first);
                let missing =
                    doc.page_index(u32::MAX).err().and_then(|e| e.code().map(str::to_owned));
                doc.call("page.text.read", Some(first), vec![]).on_settle(move |text| {
                    let _ = tx.send((list, text.clone(), index, missing));
                });
            });
        });
    });
    assert_eq!(list["pages"].as_array().map_or(0, Vec::len), 14);
    assert!(text.is_ok(), "a page read reaches the host: {text:?}");
    assert_eq!(index.ok(), Some(0));
    assert_eq!(missing.as_deref(), Some("NotFound"));
}

#[test]
fn a_metadata_update_answers_its_result_after_publishing_its_event() {
    let (result, events) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
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
    });
    let result = result.unwrap();
    assert_eq!(result["metadata"]["title"], "Native");
    assert_eq!(events.len(), 1, "one event, delivered before the call resolved");
    assert_eq!(events[0]["type"], "metadata.updated");
    assert_eq!(events[0]["metadata"]["title"], "Native");
}

#[test]
fn the_kernels_metadata_folds_both_halves_and_its_patch_is_one_change() {
    // The host reads and writes the standard fields and the custom keys
    // apart (upstream's a4b8259f7); `call_host` folds them back into the
    // kernel's one `DocumentMetadata`, with its `created` and `modified`.
    use epdf_viewer_host::translate::call_host;

    let mut host = epdf_engine::host::Host::new();
    let open = json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d1" });
    let opened =
        call_host(&mut host, &open.to_string(), vec![test_support::fixture(test_support::REPORT)]);
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);

    let patch = json!({ "title": "Native", "created": "2021-03-04T05:06:07Z",
        "custom": { "Department": "Legal" } });
    let call = translate("d1", "metadata.update", None, &[patch], 2).unwrap();
    let response = call_host(&mut host, &call.request.to_string(), call.payloads);
    let decoded = decode(&call.decode, response).unwrap();
    assert_eq!(decoded.value["metadata"]["title"], "Native");
    assert_eq!(decoded.value["metadata"]["created"], "2021-03-04T05:06:07Z");
    assert_eq!(decoded.value["metadata"]["custom"]["Department"], "Legal");
    let event = decoded.event.expect("one event");
    assert_eq!(event["type"], "metadata.updated");
    assert_eq!(event["metadata"], decoded.value["metadata"]);

    let call = translate("d1", "metadata.read", None, &[], 3).unwrap();
    let response = call_host(&mut host, &call.request.to_string(), call.payloads);
    let read = decode(&call.decode, response).unwrap().value;
    assert_eq!(read, decoded.value["metadata"]);
    assert!(read.get("createdAt").is_none(), "{read}");

    // A custom key the PDF cannot hold refuses the whole patch.
    let bad = json!({ "title": "Not written", "custom": { "Title": "shadow" } });
    let call = translate("d1", "metadata.update", None, &[bad], 4).unwrap();
    let response = call_host(&mut host, &call.request.to_string(), call.payloads);
    let error = decode(&call.decode, response).err().unwrap();
    assert_eq!(error.code(), Some("InvalidArg"));
    let call = translate("d1", "metadata.read", None, &[], 5).unwrap();
    let response = call_host(&mut host, &call.request.to_string(), call.payloads);
    assert_eq!(decode(&call.decode, response).unwrap().value["title"], "Native");
}

#[test]
fn a_locked_open_resolves_locked_and_keeps_its_session_on_the_host() {
    let worker = EngineWorker::spawn();
    let prompt = on_kernel(worker.clone(), |engine, tx| {
        engine.open(open_input(test_support::ENCRYPTED), None).on_settle(move |doc| {
            let _ = tx.send(doc.as_ref().ok().map(|d| d.security().password_prompt()));
        });
    });
    assert_eq!(prompt, Some(json!({ "state": "required" })));
    // The session stays parked on the host, waiting for its password, so a
    // close finds it.
    let (tx, rx) = oneshot::channel();
    worker.submit(
        json!({ "kind": "close", "jobId": 0, "docId": "doc-1" }).to_string(),
        Vec::new(),
        Box::new(move |response| {
            let _ = tx.send(response.json);
        }),
    );
    let answer: Value = serde_json::from_str(&futures::executor::block_on(rx).unwrap()).unwrap();
    assert_eq!(answer, json!({ "result": { "closedSessions": 1 } }));
}

use epdf_viewer_host::translate::{PATHS, maps, optional_whole, whole};

#[test]
fn every_routed_path_is_listed() {
    // Paths whose dummy args can't be valid (a page list, a PDF's bytes):
    // page edit's six, covered by `a_rotation_travels_as_a_whole_number`,
    // `a_move_carries_its_destination`, `an_absent_destination_is_left_out`
    // and `an_insert_moves_its_pdf_into_the_payload`.
    const NEEDS_REAL_ARGS: &[&str] = &[
        "pages.move",
        "pages.rotate",
        "pages.delete",
        "pages.insert",
        "pages.insertBlank",
        "pages.extract",
        // Form's import (bytes) and widget links (a widget ref), covered by
        // `an_import_sends_its_bytes_as_a_payload` and
        // `a_widget_link_sends_the_widgets_annotation_ref`.
        "forms.importData",
        "forms.attachWidget",
        "forms.detachWidget",
        // A scope, covered by `a_pages_scope_goes_out_as_whole_page_refs`.
        "redaction.apply",
        // A page and a page list, covered by
        // `named_page_paths_go_out_as_the_host_reads_them` and
        // `a_flatten_carries_its_pages_and_usage_and_answers_page_refs`.
        "pages.setName",
        "pages.flatten",
        // An index and two byte payloads, covered by
        // `the_reads_carry_their_arguments`,
        // `complete_moves_the_cms_into_a_payload` and
        // `a_visual_fill_moves_the_pdf_into_a_payload`.
        "signatures.revisionBytes",
        "signatures.complete",
        "forms.setSignatureAppearance",
        // A query, which the flattened request lifts to its top level,
        // covered by `a_search_request_goes_flat_with_its_start_page_as_a_whole_page_ref`
        // and `a_search_start_page_that_is_not_a_page_ref_is_invalid_input`.
        "search.query",
    ];
    for path in PATHS.iter().filter(|p| !NEEDS_REAL_ARGS.contains(p)) {
        assert!(translate("d1", path, Some(1), &[json!({}), json!({})], 0).is_ok(), "{path}");
    }
}

#[test]
fn a_namespace_answers_when_a_listed_path_is_under_it() {
    assert!(maps("metadata"));
    assert!(maps("metadata.read"));
    assert!(maps("page.text"));
    assert!(!maps("meta"));
    assert!(!maps("metadata.write"));
    assert!(!maps("scripts"));
}

#[test]
fn a_whole_javascript_number_goes_out_as_an_integer() {
    let value = whole("pages.rotate", "rotation", &json!(90.0)).unwrap();
    assert_eq!(value, json!(90));
    assert!(value.is_u64());
    for bad in [json!(1.5), json!(-1.0), json!({ "$number": "NaN" }), json!("3")] {
        assert_eq!(whole("p", "f", &bad).unwrap_err().code(), Some("invalid-input"), "{bad}");
    }
}

#[test]
fn an_absent_or_undefined_whole_is_left_out() {
    assert_eq!(optional_whole("p", "f", None).unwrap(), Value::Null);
    assert_eq!(optional_whole("p", "f", Some(&Value::Null)).unwrap(), Value::Null);
    assert_eq!(optional_whole("p", "f", Some(&epdf_kernel::undefined())).unwrap(), Value::Null);
    assert_eq!(optional_whole("p", "f", Some(&json!(2.0))).unwrap(), json!(2));
}

#[test]
fn an_undefined_argument_is_left_out_of_the_request() {
    let call = translate("d1", "metadata.update", None, &[epdf_kernel::undefined()], 9).unwrap();
    assert_eq!(call.request, json!({ "kind": "metadata.update", "jobId": 9, "docId": "d1" }));
}

#[test]
fn the_native_engine_opens_bytes_sent_as_json() {
    let pages = on_kernel(EngineWorker::spawn(), |engine, tx| {
        let bytes =
            epdf_kernel::bytes::Bytes(test_support::fixture(test_support::REPORT)).to_json();
        engine.open(json!({ "kind": "bytes", "bytes": bytes }), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.call("pages.list", None, vec![]).on_settle(move |list| {
                let _ =
                    tx.send(list.as_ref().ok().map(|l| l["pages"].as_array().map_or(0, Vec::len)));
            });
        });
    });
    assert_eq!(pages, Some(14));
}

#[test]
fn an_open_without_bytes_is_invalid_input() {
    let code = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(json!({ "kind": "bytes" }), None).on_settle(move |doc| {
            let _ = tx.send(doc.as_ref().err().and_then(|e| e.code().map(str::to_owned)));
        });
    });
    assert_eq!(code.as_deref(), Some("invalid-input"));
}

// ── page edits ─────────────────────────────────────────────────────────────

/// A page as the kernel holds it.
fn page_ref(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": pon })
}

/// The same page as the host reads it (595bdc6c7).
fn wire_page(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": pon })
}

/// An annotation ref by object number, as the host reads it.
fn wire_annot(pon: u32, number: u32) -> Value {
    json!({ "kind": "objectNumber", "page": wire_page(pon), "objectNumber": number })
}

#[test]
fn a_rotation_travels_as_a_whole_number() {
    let args = [json!([page_ref(3), page_ref(4)]), json!(90.0)];
    let call = translate("d1", "pages.rotate", None, &args, 11).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.rotate", "jobId": 11, "docId": "d1",
            "pages": [wire_page(3), wire_page(4)], "rotation": 90
        })
    );
    assert!(call.request["rotation"].is_u64());
    assert!(matches!(call.decode, Decode::PageMutation));
}

#[test]
fn a_move_carries_its_destination() {
    let args = [json!([page_ref(3)]), json!(0.0)];
    let call = translate("d1", "pages.move", None, &args, 12).unwrap();
    assert_eq!(call.request["pages"], json!([wire_page(3)]));
    assert_eq!(call.request["destIndex"], json!(0));
}

#[test]
fn a_fractional_index_is_invalid_input() {
    for bad in [json!(1.5), json!(-1.0), json!({ "$number": "NaN" })] {
        let args = [json!([page_ref(3)]), bad.clone()];
        let error = translate("d1", "pages.move", None, &args, 13).err().unwrap();
        assert_eq!(error.code(), Some("invalid-input"), "{bad}");
    }
}

#[test]
fn an_absent_destination_is_left_out() {
    let spec = json!({ "size": { "width": 612.0, "height": 792.0 }, "count": 2.0 });
    let call =
        translate("d1", "pages.insertBlank", None, &[spec, epdf_kernel::undefined()], 14).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.insertBlank", "jobId": 14, "docId": "d1",
            "size": { "width": 612.0, "height": 792.0 }, "count": 2
        })
    );
}

#[test]
fn an_insert_moves_its_pdf_into_the_payload() {
    let bytes = epdf_kernel::bytes::Bytes(b"%PDF-1.7".to_vec()).to_json();
    let call = translate("d1", "pages.insert", None, &[bytes, json!(1.0)], 15).unwrap();
    assert_eq!(call.payloads, vec![b"%PDF-1.7".to_vec()]);
    assert_eq!(call.request["destIndex"], json!(1));
    assert!(call.request.get("bytes").is_none());
}

#[test]
fn an_insert_without_bytes_is_invalid_input() {
    let error = translate("d1", "pages.insert", None, &[json!({})], 16).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_extract_decodes_to_host_bytes() {
    let response =
        HostResponse { json: r#"{"result":{"size":3}}"#.into(), payloads: vec![vec![1, 2, 3]] };
    let value = decode(&Decode::HostBytes, response).unwrap().value;
    let id = value["$hostBytes"].as_u64().unwrap();
    assert_eq!(HostTable::take(id), Some(vec![1, 2, 3]));
}

const PAGE: &str = r#"{"boxes":{},"index":0,"label":null,"ref":{"kind":"objectNumber","objectNumber":2},"rotation":90,"size":{"height":792.0,"width":612.0},"userUnit":1.0}"#;

#[test]
fn a_page_mutation_answers_every_layout_page_by_its_ref() {
    let json = format!(
        r#"{{"result":{{"event":{{"layout":{{"pageCount":1,"pages":[{PAGE}]}},"type":"pages.rotated"}},"mutationSequence":2,"result":{{"layout":{{"pageCount":1,"pages":[{PAGE}]}}}}}}}}"#
    );
    let decoded = decode(&Decode::PageMutation, answer(&json)).unwrap();
    assert_eq!(decoded.value["layout"]["pages"][0]["ref"], page_ref(2));
    let event = decoded.event.unwrap();
    assert_eq!(event["type"], "pages.rotated");
    assert_eq!(event["layout"]["pages"][0]["ref"], page_ref(2));
    let flat = r#"{"result":{"event":null,"mutationSequence":2,"result":{"layout":{"pageCount":1,"pages":[{"index":0,"pageObjectNumber":2}]}}}}"#;
    assert!(decode(&Decode::PageMutation, answer(flat)).is_err());
}

#[test]
fn an_insert_result_names_its_pages_as_refs() {
    let json = format!(
        r#"{{"result":{{"event":{{"insertedPages":[{{"kind":"objectNumber","objectNumber":2}}],"layout":{{"pageCount":1,"pages":[{PAGE}]}},"type":"pages.inserted"}},"mutationSequence":3,"result":{{"insertedPages":[{{"kind":"objectNumber","objectNumber":2}}],"layout":{{"pageCount":1,"pages":[{PAGE}]}}}}}}}}"#
    );
    let decoded = decode(&Decode::PageMutation, answer(&json)).unwrap();
    assert_eq!(decoded.value["insertedPages"], json!([page_ref(2)]));
}

#[test]
fn an_inserted_page_is_known_to_the_engine_handle() {
    let (inserted, index, text) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            let spec = json!({ "size": { "width": 612.0, "height": 792.0 } });
            doc.clone()
                .call("pages.insertBlank", None, vec![spec, epdf_kernel::undefined()])
                .on_settle(move |result| {
                    let result = result.as_ref().unwrap().clone();
                    let pon =
                        result["insertedPages"][0]["pageObjectNumber"].as_u64().unwrap() as u32;
                    let index = doc.page_index(pon);
                    doc.call("page.text.read", Some(pon), vec![]).on_settle(move |text| {
                        let _ = tx.send((result, index, text.is_ok()));
                    });
                });
        });
    });
    assert_eq!(inserted["layout"]["pages"].as_array().map_or(0, Vec::len), 15);
    assert_eq!(index.ok(), Some(14), "the new last page");
    assert!(text, "a page call reaches the inserted page");
}

#[test]
fn an_extract_round_trips_through_the_host_table() {
    let (bytes, left) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        HostTable::install_bytes_resolver();
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
                let first = list.as_ref().unwrap()["pages"][0]["ref"].clone();
                doc.call("pages.extract", None, vec![json!([first])]).on_settle(move |value| {
                    let bytes = epdf_kernel::bytes::Bytes::from_json(value.as_ref().unwrap());
                    let _ = tx.send((bytes, HostTable::len()));
                });
            });
        });
    });
    assert!(bytes.unwrap().0.starts_with(b"%PDF"));
    assert_eq!(left, 0, "resolving took the bytes out of the table");
}

#[test]
fn a_raw_engine_opens_bytes_given_as_bytes_json() {
    let count = on_kernel(EngineWorker::spawn(), |engine, tx| {
        let bytes = epdf_kernel::bytes::Bytes(test_support::fixture(test_support::COLORS));
        let input = json!({ "kind": "bytes", "id": "source", "bytes": bytes.to_json() });
        engine.open(input, None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.call("pages.list", None, vec![]).on_settle(move |list| {
                let _ = tx.send(list.as_ref().unwrap()["pages"].as_array().map_or(0, Vec::len));
            });
        });
    });
    assert!(count > 0);
}

#[test]
fn actions_read_becomes_its_wire_kind() {
    let call = translate("d1", "actions.read", None, &[], 11).unwrap();
    assert_eq!(call.request, json!({"kind": "actions.read", "jobId": 11, "docId": "d1"}));
    assert!(matches!(call.decode, Decode::Result));
}

#[test]
fn the_actions_namespace_is_present() {
    assert!(maps("actions"));
    assert!(maps("actions.read"));
}

#[test]
fn forms_submit_stays_unmapped_so_submits_reach_the_handler() {
    assert!(!maps("forms.submit"));
}

#[test]
fn a_namespace_is_not_a_prefix_match() {
    assert!(!maps("act"));
    assert!(!maps("actions.write"));
}

#[test]
fn a_page_annotation_list_carries_its_page_and_answers_the_snapshot() {
    let call = translate("d1", "page.annotations.list", Some(5), &[], 11).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "annotations.listRawPage", "jobId": 11, "docId": "d1", "page": wire_page(5) })
    );
    assert!(matches!(call.decode, Decode::Result));
    let error = translate("d1", "page.annotations.list", None, &[], 12).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

/// Page 1 of `features.pdf` as the native engine lists it, trimmed: every
/// page the snapshot names (the DTO's own, its ref's, a destination's, the
/// page state's and its revision's) is the `page: PageRef` the host spells
/// `objectNumber`, and arrives as the kernel spells it, `pageObjectNumber`,
/// with the annotation ref's number `annotObjectNumber`.
#[test]
fn a_listed_snapshot_arrives_in_the_kernels_spelling() {
    let json = r#"{"result":{"annotations":[{"page":{"kind":"objectNumber","objectNumber":5},"subtype":"link",
        "ref":{"objectNumber":8,"kind":"objectNumber","page":{"kind":"objectNumber","objectNumber":5}},
        "target":{"kind":"goto","destination":{"kind":"xyz","left":0.0,"page":{"kind":"objectNumber","objectNumber":7},"top":400.0,"zoom":null}},
        "actions":{"activate":{"root":{"type":"goto","subtype":"GoTo","next":[],
            "destination":{"kind":"fitR","page":{"kind":"objectNumber","objectNumber":7},"left":1.0,"bottom":2.0,"right":3.0,"top":4.0}}}}}],
        "pageState":{"page":{"kind":"objectNumber","objectNumber":5},"revision":{"docSessionId":"s","generation":0,"page":{"kind":"objectNumber","objectNumber":5}}}}}"#;
    let kernel = r#"{"annotations":[{"page":{"kind":"objectNumber","pageObjectNumber":5},"subtype":"link",
        "ref":{"annotObjectNumber":8,"kind":"objectNumber","page":{"kind":"objectNumber","pageObjectNumber":5}},
        "target":{"kind":"goto","destination":{"kind":"xyz","left":0.0,"page":{"kind":"objectNumber","pageObjectNumber":7},"top":400.0,"zoom":null}},
        "actions":{"activate":{"root":{"type":"goto","subtype":"GoTo","next":[],
            "destination":{"kind":"fitR","page":{"kind":"objectNumber","pageObjectNumber":7},"left":1.0,"bottom":2.0,"right":3.0,"top":4.0}}}}}],
        "pageState":{"page":{"kind":"objectNumber","pageObjectNumber":5},"revision":{"docSessionId":"s","generation":0,"page":{"kind":"objectNumber","pageObjectNumber":5}}}}"#;
    let call = translate("d1", "page.annotations.list", Some(5), &[], 14).unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap().value;
    assert_eq!(decoded, serde_json::from_str::<Value>(kernel).unwrap());
    assert_eq!(decoded["annotations"][0]["ref"]["page"], page_ref(5));
    assert_eq!(decoded["pageState"]["revision"]["page"], page_ref(5));
}

/// A form field ref keeps `fieldObjectNumber` both ways: the host still
/// reads and answers that spelling.
#[test]
fn a_field_ref_keeps_its_spelling_both_ways() {
    let field = json!({ "kind": "objectNumber", "fieldObjectNumber": 4 });
    let call = translate("d1", "forms.reset", None, std::slice::from_ref(&field), 1).unwrap();
    assert_eq!(call.request["ref"], field);
    let json = r#"{"result":{"result":{"field":{"ref":{"kind":"objectNumber","fieldObjectNumber":4}}},"mutationSequence":1}}"#;
    let decoded = decode(&call.decode, answer(json)).unwrap();
    assert_eq!(decoded.value["field"]["ref"], field);
}

#[test]
#[ignore = "prints the host's annotation JSON; run by hand when the wire changes"]
fn dump_annotation_shapes() {
    let listing = on_kernel(EngineWorker::spawn(), |engine, tx| {
        let input = open_input("tooling/engine-conformance/fixtures/annotation_kinds.pdf");
        engine.open(input, None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.call("annotations.listRawAll", None, vec![]).on_settle(move |listing| {
                let _ = tx.send(listing.clone());
            });
        });
    });
    println!("{}", serde_json::to_string_pretty(&listing.unwrap()).unwrap());
}
#[test]
fn a_ref_goes_out_nested_as_the_kernel_holds_it() {
    let nested = json!({ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17 });
    let call =
        translate("d1", "page.annotations.delete", Some(3), std::slice::from_ref(&nested), 12)
            .unwrap();
    assert_eq!(call.request["ref"], wire_annot(3, 17));
    assert!(matches!(call.decode, Decode::Mutation));
}

#[test]
fn a_stamp_drafts_bytes_travel_as_a_resource_payload() {
    let draft = json!({ "subtype": "stamp", "source": { "data": { "$bytes": "iVBORw==" }, "mimeType": "image/png" } });
    let call = translate("d1", "page.annotations.create", Some(3), &[draft], 13).unwrap();
    assert_eq!(call.request["kind"], "annotations.create");
    assert_eq!(call.request["page"], wire_page(3));
    assert_eq!(call.request["draft"]["source"], json!({ "resource": "r0" }));
    assert_eq!(
        call.request["resources"],
        json!({ "r0": { "payload": 0, "mimeType": "image/png" } })
    );
    assert_eq!(call.payloads, vec![vec![0x89, b'P', b'N', b'G']]);
}

#[test]
fn an_attachment_drafts_file_travels_as_a_resource_with_its_name() {
    let draft = json!({ "subtype": "file-attachment",
        "file": { "data": { "$bytes": "aGk=" }, "name": "notes.txt" } });
    let call = translate("d1", "page.annotations.create", Some(3), &[draft], 14).unwrap();
    assert_eq!(call.request["draft"]["file"], json!({ "resource": "r0", "name": "notes.txt" }));
    assert_eq!(call.request["resources"]["r0"]["payload"], 0);
    assert_eq!(call.payloads, vec![b"hi".to_vec()]);
}

#[test]
fn an_update_carries_its_ref_and_patch() {
    let nested = json!({ "kind": "nm",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "nm": "abc" });
    let call = translate(
        "d1",
        "page.annotations.update",
        Some(3),
        &[nested, json!({ "contents": "x" })],
        15,
    )
    .unwrap();
    assert_eq!(call.request["ref"], json!({ "kind": "nm", "page": wire_page(3), "nm": "abc" }));
    assert_eq!(call.request["patch"], json!({ "contents": "x" }));
    assert!(matches!(call.decode, Decode::Mutation));
}

/// The kernel's annotation numbers are JavaScript numbers (`17.0`); the
/// host's `AnnotationRef`, `RevisionToken` and `PageRef` parse whole integers.
#[test]
fn a_refs_javascript_numbers_go_out_whole() {
    let index = json!({ "kind": "index",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3.0 }, "index": 2.0,
        "revision": { "docSessionId": "s",
            "page": { "kind": "objectNumber", "pageObjectNumber": 3.0 }, "generation": 4.0 } });
    let call = translate("d1", "page.annotations.delete", Some(3), &[index], 17).unwrap();
    assert_eq!(
        call.request["ref"],
        json!({ "kind": "index", "page": wire_page(3), "index": 2,
            "revision": { "docSessionId": "s", "page": wire_page(3), "generation": 4 } })
    );
    assert!(call.request["ref"]["page"]["objectNumber"].is_u64());
    let object = json!({ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17.0 });
    let call =
        translate("d1", "page.annotations.update", Some(3), &[object, json!({})], 18).unwrap();
    assert_eq!(call.request["ref"]["objectNumber"], json!(17));
    let fractional = json!({ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 1.5 });
    let error =
        translate("d1", "page.annotations.delete", Some(3), &[fractional], 19).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_attachment_read_carries_its_page_and_ref_and_answers_its_bytes() {
    let nested = json!({ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17.0 });
    let call = translate("d1", "page.annotations.downloadFile", Some(3), &[nested], 20).unwrap();
    assert_eq!(call.request["kind"], "annotations.readFile");
    assert_eq!(call.request["page"], wire_page(3));
    assert_eq!(call.request["ref"], wire_annot(3, 17));
    let response = HostResponse {
        json: r#"{"result":{"name":"notes.txt","mimeType":"text/plain","size":2}}"#.into(),
        payloads: vec![b"hi".to_vec()],
    };
    let decoded = decode(&call.decode, response).unwrap().value;
    assert_eq!(decoded["name"], "notes.txt");
    let id = decoded["data"]["$hostBytes"].as_u64().unwrap();
    assert_eq!(HostTable::take(id), Some(b"hi".to_vec()));
}

/// A reply's `inReplyTo` and a link's `goto` destination sit inside the
/// draft or patch, not in `ref`; the host parses both nested and whole.
#[test]
fn refs_inside_a_draft_or_patch_go_out_whole() {
    let page3 = json!({ "kind": "objectNumber", "pageObjectNumber": 3 });
    let draft = json!({ "subtype": "text", "index": 2.5,
        "inReplyTo": { "kind": "objectNumber", "page": page3.clone(), "annotObjectNumber": 17.0 } });
    let call = translate("d1", "page.annotations.create", Some(3), &[draft], 21).unwrap();
    assert_eq!(call.request["draft"]["inReplyTo"], wire_annot(3, 17));
    // A draft member that only shares a ref member's name is left alone.
    assert_eq!(call.request["draft"]["index"], json!(2.5));
    let target = json!({ "kind": "objectNumber", "page": page3.clone(), "annotObjectNumber": 5.0 });
    let patch = json!({ "target": { "kind": "goto",
        "destination": { "kind": "fit", "page": { "kind": "objectNumber", "pageObjectNumber": 7.0 } } } });
    let call = translate("d1", "page.annotations.update", Some(3), &[target, patch], 22).unwrap();
    assert_eq!(
        call.request["patch"]["target"]["destination"],
        json!({ "kind": "fit", "page": wire_page(7) })
    );
}

// ── forms (plan 06) ───────────────────────────────────────────────────────

#[test]
fn a_form_list_answers_each_widget_with_its_ref_and_page() {
    let json = r#"{"result":{"formKind":"acroform","needsAppearances":false,"calculationOrder":[],"fields":[{"ref":{"kind":"objectNumber","fieldObjectNumber":4},"family":"checkbox","widgets":[{"ref":{"kind":"objectNumber","page":{"kind":"objectNumber","objectNumber":2},"annotObjectNumber":9},"annotObjectNumber":9,"page":{"kind":"objectNumber","objectNumber":2},"onState":"On","exportValue":"Yes","checked":true}]}]}}"#;
    let call = translate("d1", "forms.list", None, &[], 20).unwrap();
    let widget =
        decode(&call.decode, answer(json)).unwrap().value["fields"][0]["widgets"][0].clone();
    assert_eq!(
        widget,
        json!({
            "annotObjectNumber": 9, "onState": "On", "exportValue": "Yes", "checked": true,
            "page": { "kind": "objectNumber", "pageObjectNumber": 2 },
            "ref": { "kind": "objectNumber", "annotObjectNumber": 9,
                "page": { "kind": "objectNumber", "pageObjectNumber": 2 } }
        })
    );
}

#[test]
fn a_field_ref_is_left_as_it_is() {
    let call = translate(
        "d1",
        "forms.setValue",
        None,
        &[
            json!({ "kind": "objectNumber", "fieldObjectNumber": 4 }),
            json!({ "type": "text", "value": "x" }),
        ],
        21,
    )
    .unwrap();
    assert_eq!(call.request["ref"], json!({ "kind": "objectNumber", "fieldObjectNumber": 4 }));
    assert_eq!(call.request["value"], json!({ "type": "text", "value": "x" }));
}

#[test]
fn a_widget_placement_goes_out_with_a_whole_page_ref() {
    let draft = json!({ "family": "text", "name": "f",
        "widget": { "page": { "kind": "objectNumber", "pageObjectNumber": 2.0 },
            "rect": { "left": 0, "bottom": 0, "right": 10, "top": 10 } } });
    let call = translate("d1", "forms.createField", None, &[draft], 22).unwrap();
    assert_eq!(call.request["draft"]["widget"]["page"], wire_page(2));
    assert!(call.request["draft"]["widget"]["page"]["objectNumber"].is_u64());
    let radio = json!({ "family": "radio", "name": "r",
        "widgets": [{ "page": { "kind": "index", "index": 0 }, "onState": "A" }] });
    let error = translate("d1", "forms.createField", None, &[radio], 23).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_export_answers_its_payload_as_bytes() {
    let call = translate("d1", "forms.exportData", None, &[json!("fdf")], 23).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "forms.export", "jobId": 23, "docId": "d1", "format": "fdf" })
    );
    let response = HostResponse {
        json: r#"{"result":{"format":"fdf","size":8}}"#.into(),
        payloads: vec![b"%FDF-1.2".to_vec()],
    };
    let value = decode(&call.decode, response).unwrap().value;
    assert_eq!(value["format"], "fdf");
    assert_eq!(value["bytes"], json!({ "$bytes": "JUZERi0xLjI=" }));
}

#[test]
fn an_import_sends_its_bytes_as_a_payload() {
    let call = translate(
        "d1",
        "forms.importData",
        None,
        &[json!({ "$bytes": "JUZERi0xLjI=" }), epdf_kernel::undefined()],
        24,
    )
    .unwrap();
    assert_eq!(call.request, json!({ "kind": "forms.import", "jobId": 24, "docId": "d1" }));
    assert_eq!(call.payloads, vec![b"%FDF-1.2".to_vec()]);
}

#[test]
fn a_repair_reads_bake_appearances_from_its_options() {
    let call =
        translate("d1", "forms.repair", None, &[json!({ "bakeAppearances": true })], 25).unwrap();
    assert_eq!(call.request["bakeAppearances"], true);
    assert!(matches!(call.decode, Decode::Mutation));
}

#[test]
fn a_field_ref_sent_as_a_javascript_number_goes_out_whole() {
    let call = translate(
        "d1",
        "forms.reset",
        None,
        &[json!({ "kind": "objectNumber", "fieldObjectNumber": 4.0 })],
        26,
    )
    .unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "forms.reset", "jobId": 26, "docId": "d1",
        "ref": { "kind": "objectNumber", "fieldObjectNumber": 4 } })
    );
    assert!(matches!(call.decode, Decode::Mutation));
}

#[test]
fn a_widget_link_sends_the_widgets_annotation_ref() {
    let widget = json!({ "kind": "objectNumber", "annotObjectNumber": 9.0,
        "page": { "kind": "objectNumber", "pageObjectNumber": 2 } });
    let field = json!({ "kind": "objectNumber", "fieldObjectNumber": 4 });
    for (path, kind) in
        [("forms.attachWidget", "forms.attachWidget"), ("forms.detachWidget", "forms.detachWidget")]
    {
        let call = translate("d1", path, None, &[field.clone(), widget.clone()], 27).unwrap();
        assert_eq!(
            call.request,
            json!({ "kind": kind, "jobId": 27, "docId": "d1", "ref": field,
            "widget": wire_annot(2, 9) })
        );
    }
}

#[test]
fn an_effects_batch_that_wrote_nothing_answers_its_result_and_no_event() {
    // The host answers a no-op batch `{result, mutationSequence}` with no
    // event, as it answers a no-op `pages.flatten` or `redaction.apply`.
    let call = translate("d1", "forms.applyEffects", None, &[json!([])], 28).unwrap();
    let json = r#"{"result":{"result":{"results":[],"changedWidgets":[],"meta":null},"mutationSequence":3}}"#;
    let decoded = decode(&call.decode, answer(json)).unwrap();
    assert_eq!(decoded.value, json!({ "results": [], "changedWidgets": [], "meta": null }));
    assert!(decoded.event.is_none());
}

#[test]
fn a_form_mutation_answers_the_widgets_of_its_result_and_event() {
    let call = translate("d1", "forms.setValue", None, &[json!({}), json!({})], 29).unwrap();
    let field = r#"{"ref":{"kind":"objectNumber","fieldObjectNumber":4},"widgets":[{"ref":{"kind":"objectNumber","annotObjectNumber":9,"page":{"kind":"objectNumber","pageObjectNumber":2}},"annotObjectNumber":9,"page":{"kind":"objectNumber","pageObjectNumber":2}}]}"#;
    let json = format!(
        r#"{{"result":{{"result":{{"field":{field}}},"mutationSequence":3,"event":{{"type":"form.valueChanged","field":{field}}}}}}}"#
    );
    let decoded = decode(&call.decode, answer(&json)).unwrap();
    let nested = json!({ "kind": "objectNumber", "annotObjectNumber": 9,
        "page": { "kind": "objectNumber", "pageObjectNumber": 2 } });
    assert_eq!(decoded.value["field"]["widgets"][0]["ref"], nested);
    assert_eq!(decoded.event.unwrap()["field"]["widgets"][0]["ref"], nested);
}

#[test]
fn a_field_edit_answers_its_result_and_event_as_the_host_sent_them() {
    // The host answers createField, updateField and the widget links as
    // the TypeScript engine does, `{field, meta}`, and spreads that into the
    // event beside `type`, so both go out as they came.
    for path in
        ["forms.createField", "forms.updateField", "forms.attachWidget", "forms.detachWidget"]
    {
        let widget = json!({ "kind": "objectNumber", "annotObjectNumber": 9, "page": page_ref(2) });
        let call = translate("d1", path, None, &[json!({}), widget], 30).unwrap();
        let field = r#""field":{"ref":{"kind":"objectNumber","fieldObjectNumber":4},"name":"a","widgets":[{"ref":{"kind":"objectNumber","annotObjectNumber":9,"page":{"kind":"objectNumber","pageObjectNumber":2}},"annotObjectNumber":9,"page":{"kind":"objectNumber","pageObjectNumber":2}}]},"meta":{"affectedPages":[],"cacheDelta":null}"#;
        let json = format!(
            r#"{{"result":{{"result":{{{field}}},"mutationSequence":3,"event":{{"type":"form.fieldCreated","origin":{{"kind":"local"}},{field}}}}}}}"#
        );
        let decoded = decode(&call.decode, answer(&json)).unwrap();
        assert_eq!(decoded.value["field"]["name"], "a", "{path}");
        assert_eq!(decoded.value["field"]["widgets"][0]["ref"]["annotObjectNumber"], 9, "{path}");
        assert_eq!(decoded.value["meta"], json!({ "affectedPages": [], "cacheDelta": null }));
        let event = decoded.event.unwrap();
        assert_eq!(event["type"], "form.fieldCreated");
        assert_eq!(event["origin"], json!({ "kind": "local" }));
        assert_eq!(event["field"]["name"], "a", "{path}");
        assert_eq!(event["meta"], json!({ "affectedPages": [], "cacheDelta": null }));
    }
}

#[test]
fn an_effects_batch_hands_back_the_hosts_effects_applied() {
    // The host publishes `form.effectsApplied` itself, as the TypeScript
    // engine does, so the event goes out as it came.
    let call = translate("d1", "forms.applyEffects", None, &[json!([])], 31).unwrap();
    let json = r#"{"result":{"result":{"results":[],"changedWidgets":[]},"mutationSequence":4,"event":{"type":"form.effectsApplied","origin":{"kind":"local"},"results":[],"changedWidgets":[]}}}"#;
    let decoded = decode(&call.decode, answer(json)).unwrap();
    assert_eq!(decoded.value, json!({ "results": [], "changedWidgets": [] }));
    let event = decoded.event.unwrap();
    assert_eq!(event["type"], "form.effectsApplied");
    assert_eq!(event["changedWidgets"], json!([]));
}

// ── redaction ──────────────────────────────────────────────────────────────

#[test]
fn redaction_is_present_once_its_path_is_listed() {
    assert!(maps("redaction"));
    assert!(maps("redaction.apply"));
    assert!(!maps("redact"));
    assert!(!maps("scripts"));
}

#[test]
fn a_pages_scope_goes_out_as_whole_page_refs() {
    // Page object numbers arrive as JavaScript numbers.
    let scope = json!({ "kind": "pages", "pages": [
        { "kind": "objectNumber", "pageObjectNumber": 3.0 },
        { "kind": "objectNumber", "pageObjectNumber": 7 } ] });
    let call = translate("d1", "redaction.apply", None, &[scope], 30).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "redaction.apply", "jobId": 30, "docId": "d1",
            "scope": { "kind": "pages", "pages": [wire_page(3), wire_page(7)] } })
    );
    assert!(call.request["scope"]["pages"][0]["objectNumber"].is_u64());
}

#[test]
fn an_annotations_scope_goes_out_with_whole_refs() {
    let scope = json!({ "kind": "annotations", "refs": [{ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17.0 }] });
    let call = translate("d1", "redaction.apply", None, &[scope], 31).unwrap();
    assert_eq!(
        call.request["scope"],
        json!({ "kind": "annotations", "refs": [wire_annot(3, 17)] })
    );
    assert!(call.request["scope"]["refs"][0]["objectNumber"].is_u64());
}

#[test]
fn a_redaction_scope_that_is_neither_kind_is_invalid_input() {
    let error = translate("d1", "redaction.apply", None, &[json!({})], 33).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
    let pages = json!({ "kind": "pages", "pages": [{ "pageObjectNumber": "x" }] });
    let error = translate("d1", "redaction.apply", None, &[pages], 34).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_apply_result_answers_pages_and_nested_refs() {
    // The host's event spreads the result's members beside `type` and
    // `origin` (`DocumentEventInit` is an internally tagged enum).
    let json = r#"{"result":{"result":{"scope":{"kind":"pages","pages":[{"kind":"objectNumber","objectNumber":3}]},"results":[{"page":{"kind":"objectNumber","objectNumber":3},"status":"applied","removedAnnotationCount":2}],"removedAnnotationCount":2},"mutationSequence":4,"event":{"type":"redaction.applied","origin":{"kind":"local","serverId":null},"scope":{"kind":"pages","pages":[{"kind":"objectNumber","objectNumber":3}]},"results":[{"page":{"kind":"objectNumber","objectNumber":3},"status":"applied","removedAnnotationCount":2}],"removedAnnotationCount":2}}}"#;
    let call =
        translate("d1", "redaction.apply", None, &[json!({ "kind": "pages", "pages": [] })], 32)
            .unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap();
    let page3 = json!({ "kind": "objectNumber", "pageObjectNumber": 3 });
    assert_eq!(decoded.value["scope"], json!({ "kind": "pages", "pages": [page3.clone()] }));
    assert_eq!(decoded.value["results"][0]["page"], page3);
    assert!(decoded.value["results"][0].get("pageObjectNumber").is_none());
    let event = decoded.event.unwrap();
    assert_eq!(event["scope"], json!({ "kind": "pages", "pages": [page3.clone()] }));
    assert_eq!(event["results"][0]["page"], page3);
}

#[test]
fn an_annotations_apply_result_answers_nested_refs() {
    let json = r#"{"result":{"result":{"scope":{"kind":"annotations","refs":[{"kind":"objectNumber","page":{"kind":"objectNumber","objectNumber":3},"objectNumber":17}]},"results":[{"page":{"kind":"objectNumber","objectNumber":3},"status":"applied","removedAnnotationCount":0}],"removedAnnotationCount":0},"mutationSequence":5,"event":null}}"#;
    let call = translate(
        "d1",
        "redaction.apply",
        None,
        &[json!({ "kind": "annotations", "refs": [] })],
        35,
    )
    .unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap();
    assert_eq!(
        decoded.value["scope"]["refs"][0],
        json!({ "kind": "objectNumber", "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17 })
    );
}

#[test]
fn page_measure_paths_go_to_the_measure_kinds() {
    assert!(epdf_viewer_host::translate::maps("page.measure"));
    let read = translate("doc-1", "page.measure.viewports", Some(7), &[], 3).unwrap();
    assert_eq!(read.request["kind"], json!("measure.viewports"));
    assert_eq!(read.request["page"], wire_page(7));
    assert!(matches!(read.decode, Decode::Field("viewports")));
    let measure = json!({ "subtype": "RL", "ratio": "1 in = 1 m" });
    let write =
        translate("doc-1", "page.measure.setScale", Some(7), std::slice::from_ref(&measure), 4)
            .unwrap();
    assert_eq!(write.request["kind"], json!("measure.setScale"));
    assert_eq!(write.request["page"], wire_page(7));
    assert_eq!(write.request["measure"], measure);
}

#[test]
fn clearing_a_scale_sends_an_explicit_null() {
    let write = translate("doc-1", "page.measure.setScale", Some(7), &[Value::Null], 4).unwrap();
    assert!(write.request.as_object().unwrap().contains_key("measure"));
    assert_eq!(write.request["measure"], Value::Null);
}

#[test]
fn viewports_decode_to_the_list() {
    let call = translate("doc-1", "page.measure.viewports", Some(7), &[], 3).unwrap();
    let decoded = decode(
        &call.decode,
        answer(
            &json!({ "result": { "viewports": [{
                "bbox": { "left": 0, "bottom": 0, "right": 1, "top": 1 }, "owned": true
            }] } })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(decoded.value.as_array().unwrap().len(), 1);
}

#[test]
fn a_version_decodes_to_the_version_it_carries() {
    // `{ tag: 'document.version'; version }`; the kernel takes the version.
    let call = translate("doc-1", "document.version", None, &[], 3).unwrap();
    let version = json!({ "sha256": "ab".repeat(32), "byteLength": 1024 });
    let decoded = decode(
        &call.decode,
        answer(&json!({ "result": { "version": version.clone() } }).to_string()),
    )
    .unwrap();
    assert_eq!(decoded.value, version);
}

#[test]
fn a_scale_write_publishes_its_page_as_a_page_ref() {
    let call = translate("doc-1", "page.measure.setScale", Some(7), &[Value::Null], 4).unwrap();
    let result =
        json!({ "page": page_ref(7), "meta": { "affectedPages": [], "cacheDelta": null } });
    let response = HostResponse {
        json: json!({ "result": {
            "result": result,
            "mutationSequence": 2,
            "event": {
                "type": "page.viewportsChanged",
                "page": page_ref(7),
                "meta": { "affectedPages": [], "cacheDelta": null },
                "origin": { "kind": "local", "sessionId": "s", "ts": 1 }
            }
        } })
        .to_string(),
        payloads: Vec::new(),
    };
    let decoded = decode(&call.decode, response).unwrap();
    let page = json!({ "kind": "objectNumber", "pageObjectNumber": 7 });
    assert_eq!(decoded.value["page"], page);
    let event = decoded.event.expect("the event to publish");
    assert_eq!(event["type"], json!("page.viewportsChanged"));
    assert_eq!(event["page"], page);
}

// ── stamp ──────────────────────────────────────────────────────────────────

#[test]
fn piece_info_paths_carry_their_holder() {
    assert!(maps("pieceInfo") && maps("page.pieceInfo"));
    let doc = translate("doc-1", "pieceInfo.read", None, &[json!("EmbedPDF")], 1).unwrap();
    assert_eq!(doc.request["kind"], json!("pieceInfo.read"));
    assert_eq!(doc.request["application"], json!("EmbedPDF"));
    assert!(doc.request.get("page").is_none());
    let page = translate(
        "doc-1",
        "page.pieceInfo.update",
        Some(4),
        &[json!("EmbedPDF"), json!({ "set": {} })],
        2,
    )
    .unwrap();
    assert_eq!(page.request["kind"], json!("pieceInfo.update"));
    assert_eq!(page.request["page"], wire_page(4));
    assert_eq!(page.request["patch"], json!({ "set": {} }));
    // `void` to the kernel, whatever the host answers (upstream's
    // `PieceInfoUpdateResult` until Task 13 ports the kernel), and no event.
    let decoded = decode(
        &page.decode,
        answer(
            r#"{"result":{"result":{"pieceInfo":null,"meta":{"affectedPages":[],"cacheDelta":null,"opId":"op","undoable":false}},"mutationSequence":1}}"#,
        ),
    )
    .unwrap();
    assert!(epdf_kernel::is_undefined(&decoded.value), "{:?}", decoded.value);
    assert!(decoded.event.is_none());
}

#[test]
fn an_application_without_piece_info_reads_as_null() {
    let call = translate("doc-1", "page.pieceInfo.read", Some(4), &[json!("EmbedPDF")], 1).unwrap();
    let decoded = decode(&call.decode, answer(r#"{"result":null}"#)).unwrap();
    assert_eq!(decoded.value, Value::Null);
    let found = decode(&call.decode, answer(r#"{"result":{"entries":{}}}"#)).unwrap();
    assert_eq!(found.value, json!({ "entries": {} }));
}

#[test]
fn named_page_paths_go_out_as_the_host_reads_them() {
    assert!(maps("pages.setName") && maps("pages.removeName"));
    let set = translate(
        "doc-1",
        "pages.setName",
        None,
        &[json!({ "name": "a=A", "page": page_ref(3), "replace": "a=Z" })],
        1,
    )
    .unwrap();
    assert_eq!(set.request["kind"], json!("pages.setName"));
    assert_eq!(set.request["name"], json!("a=A"));
    assert_eq!(set.request["page"], wire_page(3));
    assert_eq!(set.request["replace"], json!("a=Z"));
    let remove =
        translate("doc-1", "pages.removeName", None, &[json!({ "name": "a=A" })], 2).unwrap();
    assert_eq!(remove.request["name"], json!("a=A"));
    let pageless = translate("doc-1", "pages.setName", None, &[json!({ "name": "a=A" })], 3);
    assert_eq!(pageless.err().unwrap().code(), Some("invalid-input"));
}

#[test]
fn a_named_page_target_arrives_as_a_page_ref() {
    let json = r#"{"result":{"pageCount":1,"pages":[{"index":0,"ref":{"kind":"objectNumber","objectNumber":3}}],"namedPages":[{"name":"a=A","target":{"kind":"page","page":{"kind":"objectNumber","objectNumber":3}}},{"name":"t","target":{"kind":"template","objectNumber":9}}]}}"#;
    let decoded = decode(&Decode::PageList, answer(json)).unwrap();
    assert_eq!(
        decoded.value["namedPages"][0]["target"],
        json!({ "kind": "page", "page": page_ref(3) })
    );
    assert_eq!(
        decoded.value["namedPages"][1]["target"],
        json!({ "kind": "template", "objectNumber": 9 })
    );
    let set = translate(
        "doc-1",
        "pages.setName",
        None,
        &[json!({ "name": "a=A", "page": page_ref(3) })],
        1,
    )
    .unwrap();
    let mutation = r#"{"result":{"result":{"layout":{"pageCount":1,"pages":[{"index":0,"ref":{"kind":"objectNumber","objectNumber":3}}],"namedPages":[{"name":"a=A","target":{"kind":"page","page":{"kind":"objectNumber","objectNumber":3}}}]}},"mutationSequence":1,"event":{"type":"pages.named","name":"a=A","page":{"kind":"objectNumber","objectNumber":3},"layout":{"pageCount":1,"pages":[{"index":0,"ref":{"kind":"objectNumber","objectNumber":3}}],"namedPages":[]}}}}"#;
    let decoded = decode(&set.decode, answer(mutation)).unwrap();
    assert_eq!(decoded.value["layout"]["pages"][0]["ref"], page_ref(3));
    assert_eq!(decoded.value["layout"]["namedPages"][0]["target"]["page"], page_ref(3));
    let event = decoded.event.unwrap();
    assert_eq!(event["type"], json!("pages.named"));
    // `page: PageRef | null`, as `LocalDocumentPagesService.runNameJob` spells it.
    assert_eq!(event["page"], page_ref(3));
    assert!(event.get("pageObjectNumber").is_none());
    let remove =
        translate("doc-1", "pages.removeName", None, &[json!({ "name": "a=A" })], 2).unwrap();
    let removed = r#"{"result":{"result":{"layout":{"pageCount":1,"pages":[{"index":0,"ref":{"kind":"objectNumber","objectNumber":3}}],"namedPages":[]}},"mutationSequence":2,"event":{"type":"pages.named","name":"a=A","page":null,"layout":{"pageCount":1,"pages":[{"index":0,"ref":{"kind":"objectNumber","objectNumber":3}}],"namedPages":[]}}}}"#;
    let event = decode(&remove.decode, answer(removed)).unwrap().event.unwrap();
    assert_eq!(event["page"], Value::Null);
    assert!(event.get("pageObjectNumber").is_none());
}

#[test]
fn a_flatten_carries_its_pages_and_usage_and_answers_page_refs() {
    let call =
        translate("doc-1", "pages.flatten", None, &[json!([page_ref(3)]), json!("display")], 1)
            .unwrap();
    assert_eq!(call.request["kind"], json!("pages.flatten"));
    assert_eq!(call.request["pages"], json!([wire_page(3)]));
    assert_eq!(call.request["usage"], json!("display"));
    let json = r#"{"result":{"result":{"pages":[{"kind":"objectNumber","objectNumber":3}],"usage":"display","results":[{"page":{"kind":"objectNumber","objectNumber":3},"status":"applied"}]},"mutationSequence":1,"event":null}}"#;
    let decoded = decode(&call.decode, answer(json)).unwrap();
    assert_eq!(decoded.value["results"][0]["page"], page_ref(3));
}

#[test]
fn a_download_saves_the_whole_file_into_the_host_table() {
    assert!(maps("download"));
    let call = translate("doc-1", "download", None, &[], 1).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "document.saveBuffer", "jobId": 1, "docId": "doc-1", "mode": "rewrite" })
    );
    let response =
        HostResponse { json: r#"{"result":{"size":3}}"#.into(), payloads: vec![vec![1, 2, 3]] };
    let decoded = decode(&call.decode, response).unwrap();
    let id = decoded.value["$hostBytes"].as_u64().unwrap();
    assert_eq!(HostTable::take(id), Some(vec![1, 2, 3]));
}

#[test]
fn a_download_carries_the_kernels_save_mode() {
    let mode = |options: Value| {
        translate("doc-1", "download", None, &[options], 1).map(|call| call.request["mode"].clone())
    };
    assert_eq!(mode(json!({ "mode": "incremental" })).unwrap(), json!("incremental"));
    assert_eq!(mode(json!({ "mode": "rewrite" })).unwrap(), json!("rewrite"));
    assert_eq!(mode(json!({})).unwrap(), json!("rewrite"));
    assert_eq!(mode(json!({ "mode": { "$undefined": true } })).unwrap(), json!("rewrite"));
    let error = mode(json!({ "mode": "sideways" })).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_encoded_render_asks_for_png_and_answers_bytes() {
    use epdf_viewer_host::translate::translate_encoded_render;
    let options = json!({ "viewport": { "kind": "width", "width": 64 }, "format": "png" });
    let call = translate_encoded_render("doc-1", Some(2), &[options], 1).unwrap();
    assert_eq!(call.request["kind"], json!("pages.renderEncoded"));
    assert_eq!(call.request["page"], wire_page(2));
    assert_eq!(call.request["encode"], json!({ "format": "png" }));
    assert!(call.request["options"].get("format").is_none());
    let response = HostResponse {
        json: r#"{"result":{"mimeType":"image/png","width":64,"height":80,"size":3}}"#.into(),
        payloads: vec![vec![1, 2, 3]],
    };
    let decoded = decode(&Decode::Encoded, response).unwrap();
    assert_eq!(decoded.value["contentType"], json!("image/png"));
    assert_eq!(decoded.value["width"], json!(64));
    assert_eq!(decoded.value["source"]["kind"], json!("bytes"));
    assert_eq!(decoded.value["source"]["bytes"], json!({ "$bytes": "AQID" }));
    let jpeg = json!({ "format": "jpeg" });
    let refused = translate_encoded_render("doc-1", Some(2), &[jpeg], 2);
    assert_eq!(refused.err().unwrap().code(), Some("unsupported"));
}

// 16 signature.

#[test]
fn signature_paths_are_mapped() {
    for path in [
        "signatures",
        "document.versioned",
        "forms.setSignatureAppearance",
        "signatures.list",
        "signatures.contents",
        "signatures.digest",
        "signatures.revisionBytes",
        "signatures.analyze",
        "signatures.prepare",
        "signatures.complete",
        "signatures.abort",
        "document.version",
    ] {
        assert!(epdf_viewer_host::translate::maps(path), "{path}");
    }
}

#[test]
fn complete_moves_the_cms_into_a_payload() {
    let input = json!({ "signingId": "s1", "cms": { "$bytes": "AQID" },
                        "expectedVersion": { "baseSha256": "x", "editsVersion": 0.0 } });
    let call = translate("doc-1", "signatures.complete", None, &[input], 1).unwrap();
    assert_eq!(call.request["kind"], json!("signatures.complete"));
    assert!(call.request["input"].get("cms").is_none());
    assert_eq!(call.request["input"]["expectedVersion"]["editsVersion"], json!(0));
    assert_eq!(call.payloads, vec![vec![1, 2, 3]]);
    assert!(matches!(call.decode, Decode::Mutation));
}

#[test]
fn a_visual_fill_moves_the_pdf_into_a_payload() {
    let field = json!({ "kind": "objectNumber", "fieldObjectNumber": 12.0 });
    let call = translate(
        "doc-1",
        "forms.setSignatureAppearance",
        None,
        &[field, json!({ "pdf": { "$bytes": "JVBERg==" }, "pageIndex": 0.0 })],
        1,
    )
    .unwrap();
    assert_eq!(call.request["ref"], json!({ "kind": "objectNumber", "fieldObjectNumber": 12 }));
    assert_eq!(call.request["pageIndex"], json!(0));
    assert_eq!(call.payloads, vec![b"%PDF".to_vec()]);
}

#[test]
fn prepare_moves_its_appearance_into_a_payload_and_whole_numbers() {
    let request = json!({
        "field": { "kind": "objectNumber", "fieldObjectNumber": 12.0 },
        "certify": { "permission": 2.0 },
        "lock": { "action": "include", "fields": ["a"], "permission": 1.0 },
        "appearance": { "pdf": { "$bytes": "JVBERg==" }, "pageIndex": 0.0 },
    });
    let call = translate("doc-1", "signatures.prepare", None, &[request], 1).unwrap();
    let input = &call.request["input"];
    assert_eq!(input["field"]["fieldObjectNumber"], json!(12));
    assert_eq!(input["certify"]["permission"], json!(2));
    assert_eq!(input["lock"]["permission"], json!(1));
    assert_eq!(input["appearance"], json!({ "pageIndex": 0 }));
    assert_eq!(call.payloads, vec![b"%PDF".to_vec()]);
    // No artwork: the appearance is left out altogether.
    let bare = json!({ "field": { "kind": "objectNumber", "fieldObjectNumber": 12 },
                       "appearance": { "pdf": null, "pageIndex": 0 } });
    let call = translate("doc-1", "signatures.prepare", None, &[bare], 2).unwrap();
    assert!(call.request["input"].get("appearance").is_none(), "{}", call.request);
    assert!(call.payloads.is_empty());
}

#[test]
fn the_reads_carry_their_arguments() {
    let field = json!({ "kind": "objectNumber", "fieldObjectNumber": 5 });
    let call =
        translate("d", "signatures.contents", None, std::slice::from_ref(&field), 1).unwrap();
    assert_eq!(call.request["ref"], field);
    let call =
        translate("d", "signatures.digest", None, &[field.clone(), json!("sha256")], 1).unwrap();
    assert_eq!(call.request["algorithm"], json!("sha256"));
    let call = translate("d", "signatures.revisionBytes", None, &[json!(1.0)], 1).unwrap();
    assert_eq!(call.request["revisionIndex"], json!(1));
    let call = translate("d", "signatures.abort", None, &[json!("s1")], 1).unwrap();
    assert_eq!(call.request["signingId"], json!("s1"));
    let input = json!({ "since": { "signatureIndex": 1.0 }, "until": "working-copy" });
    let call = translate("d", "signatures.analyze", None, &[input], 1).unwrap();
    assert_eq!(call.request["input"]["since"]["signatureIndex"], json!(1));
    let call = translate("d", "signatures.list", None, &[], 1).unwrap();
    assert!(call.request.get("workingCopy").is_none());
    let call = translate("d", "document.version", None, &[], 1).unwrap();
    assert_eq!(call.request["kind"], json!("document.version"));
}

#[test]
fn a_signing_notice_beside_the_result_is_handed_back() {
    // `signatures.prepare` and `signatures.abort` are not mutations: their
    // answer is the bare result, and the notice the host publishes rides in
    // the envelope beside it.
    let call = translate("d", "signatures.prepare", None, &[json!({})], 1).unwrap();
    let response = HostResponse {
        json: r#"{"result":{"signingId":"s1"},"event":{"type":"signature.prepared","signingId":"s1","field":{"kind":"objectNumber","fieldObjectNumber":5},"origin":{"kind":"local"}}}"#.into(),
        payloads: vec![vec![0; 32]],
    };
    let decoded = decode(&call.decode, response).unwrap();
    assert_eq!(decoded.value["signingId"], json!("s1"));
    assert!(decoded.value.get("event").is_none(), "{}", decoded.value);
    let event = decoded.event.unwrap();
    assert_eq!(event["type"], json!("signature.prepared"));
    assert_eq!(event["field"]["fieldObjectNumber"], json!(5));

    let call = translate("d", "signatures.abort", None, &[json!("s1")], 2).unwrap();
    let json = r#"{"result":{"status":"aborted"},"event":{"type":"signature.aborted","signingId":"s1","origin":{"kind":"local"}}}"#;
    let decoded = decode(&call.decode, answer(json)).unwrap();
    assert_eq!(decoded.value, json!({ "status": "aborted" }));
    assert_eq!(decoded.event.unwrap()["type"], json!("signature.aborted"));

    // An abort that found nothing parked has no notice.
    let decoded = decode(&call.decode, answer(r#"{"result":{"status":"unknown"}}"#)).unwrap();
    assert!(decoded.event.is_none());
}

#[test]
fn contents_come_back_as_bytes() {
    let response =
        HostResponse { json: r#"{"result":{"size":3}}"#.into(), payloads: vec![vec![1, 2, 3]] };
    let decoded = decode(&Decode::InlineBytes, response).unwrap();
    assert_eq!(decoded.value, json!({ "$bytes": "AQID" }));
}

#[test]
fn prepare_answers_its_digest_inside_the_result() {
    let response =
        HostResponse { json: r#"{"result":{"signingId":"s1"}}"#.into(), payloads: vec![vec![9]] };
    let decoded = decode(&Decode::ResultWithBytes("digest"), response).unwrap();
    assert_eq!(decoded.value["digest"], json!({ "$bytes": "CQ==" }));
    assert_eq!(decoded.value["signingId"], json!("s1"));
}

// ── the Task 8 transition ────────────────────────────────────────────────

fn call_host_json(host: &mut epdf_engine::host::Host, request: Value) -> Value {
    let response = epdf_viewer_host::translate::call_host(host, &request.to_string(), Vec::new());
    serde_json::from_str(&response.json).unwrap()
}

fn opened_host() -> epdf_engine::host::Host {
    let mut host = epdf_engine::host::Host::new();
    let bytes = test_support::fixture("tooling/engine-conformance/fixtures/annotation_kinds.pdf");
    let opened = host.call(r#"{"kind":"open.fatMem","jobId":1,"docId":"d"}"#, vec![bytes]);
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    host
}

/// A kernel path mapped onto a kind upstream removed still reaches the
/// host, through `call_legacy`, while the wire answers it as unknown.
#[test]
fn a_removed_kind_goes_through_the_legacy_entry() {
    let mut host = opened_host();
    let pages =
        call_host_json(&mut host, json!({ "kind": "pages.list", "jobId": 2, "docId": "d" }));
    let first = pages["result"]["pages"][0]["ref"].clone();
    let rotate = json!({ "kind": "pages.rotate", "jobId": 3, "docId": "d",
        "pages": [first], "rotation": 90 });
    let wire: Value =
        serde_json::from_str(&host.call(&rotate.to_string(), Vec::new()).json).unwrap();
    assert_eq!(wire["error"]["message"], "unknown request kind: pages.rotate", "{wire}");
    let legacy = call_host_json(&mut host, rotate);
    assert!(legacy.get("error").is_none(), "{legacy}");
}

/// The listing still names an inline annotation by `nm`, and the kernel
/// sends that back; the wire takes the `baseIndex` upstream names it by.
#[test]
fn an_nm_ref_reaches_the_host_as_its_base_index() {
    let mut host = opened_host();
    let pages =
        call_host_json(&mut host, json!({ "kind": "pages.list", "jobId": 2, "docId": "d" }));
    let named = pages["result"]["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|page| {
            let listed = call_host_json(
                &mut host,
                json!({ "kind": "annotations.listRawPage", "jobId": 3, "docId": "d",
                    "page": page["ref"] }),
            );
            listed["result"]["annotations"]
                .as_array()?
                .iter()
                .find(|a| a["ref"]["kind"] == "nm")
                .cloned()
        })
        .expect("the fixture has an inline annotation with a /NM");
    let update = json!({ "kind": "annotations.update", "jobId": 4, "docId": "d",
        "ref": named["ref"], "patch": { "subtype": named["subtype"], "contents": "edited" } });
    let wire: Value =
        serde_json::from_str(&host.call(&update.to_string(), Vec::new()).json).unwrap();
    assert_eq!(wire["error"]["code"], "WireFormat", "the wire still took an nm ref: {wire}");
    let answer = call_host_json(&mut host, update);
    assert!(answer.get("error").is_none(), "{answer}");
}

// ── page space (Task 8.4) ───────────────────────────────────────────────────

/// A two-page PDF whose visible boxes do not start at the origin: page 3
/// cropped inside its media box, page 4 centred on the origin. The catalog
/// opens at a destination on page 4 and page 3 goes to page 4 when it
/// closes. Cross-reference offsets are computed.
fn shifted_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /OpenAction [4 0 R /XYZ -250 350 null] >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /CropBox [50 60 562 732] \
         /AA << /C << /S /GoTo /D [4 0 R /FitH 300] >> >> >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [-300 -390 300 390] >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn shifted_host() -> epdf_engine::host::Host {
    let mut host = epdf_engine::host::Host::new();
    let opened = host.call(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![shifted_pdf()],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    host
}

#[test]
fn a_page_list_reaches_the_kernel_in_the_files_coordinates() {
    // The host answers page space; the kernel's page registry and stage read
    // PDF-space boxes and destinations until Task 13.
    use epdf_viewer_host::translate::call_host;
    let mut host = shifted_host();
    let call = translate("d", "pages.list", None, &[], 2).unwrap();
    let response = call_host(&mut host, &call.request.to_string(), call.payloads);
    let list = decode(&call.decode, response).expect("decodes").value;
    let first = &list["pages"][0];
    assert_eq!(
        first["boxes"]["crop"],
        json!({ "left": 50.0, "bottom": 60.0, "right": 562.0, "top": 732.0 })
    );
    assert_eq!(
        first["boxes"]["media"],
        json!({ "left": 0.0, "bottom": 0.0, "right": 612.0, "top": 792.0 })
    );
    // The close action goes to page 4, measured on page 4's box.
    let destination = &first["actions"]["close"]["root"]["destination"];
    assert_eq!(destination["kind"], "fitH");
    assert_eq!(destination["top"], json!(300.0));
    assert_eq!(destination["page"], json!({ "kind": "objectNumber", "pageObjectNumber": 4 }));
    assert!(destination.get("y").is_none(), "{destination}");
}

#[test]
fn the_catalog_actions_reach_the_kernel_in_the_files_coordinates() {
    use epdf_viewer_host::translate::call_host;
    let mut host = shifted_host();
    let call = translate("d", "actions.read", None, &[], 2).unwrap();
    let response = call_host(&mut host, &call.request.to_string(), call.payloads);
    let actions = decode(&call.decode, response).expect("decodes").value;
    assert_eq!(
        actions["openDestination"],
        json!({ "kind": "xyz", "page": { "kind": "objectNumber", "pageObjectNumber": 4 },
            "left": -250.0, "top": 350.0, "zoom": null })
    );
    // And the wire itself answers page space.
    let wire = host
        .call(&json!({ "kind": "actions.read", "jobId": 3, "docId": "d" }).to_string(), Vec::new());
    let wire: Value = serde_json::from_str(&wire.json).unwrap();
    assert_eq!(wire["result"]["openDestination"]["x"], json!(50.0));
    assert_eq!(wire["result"]["openDestination"]["y"], json!(40.0));
}

#[test]
fn a_render_target_from_the_kernel_goes_out_in_page_space() {
    // The render plugin sends its tile in the file's coordinates
    // (`page_rect_to_pdf`); the host measures a target from the visible
    // box's top-left.
    use epdf_viewer_host::translate::call_host;
    let mut host = shifted_host();
    let tile = json!({ "target": { "kind": "rect",
        "rect": { "left": 150.0, "right": 250.0, "top": 632.0, "bottom": 582.0 } } });
    let call = translate("d", "page.render.image", Some(3), &[tile], 2).unwrap();
    let response = call_host(&mut host, &call.request.to_string(), call.payloads);
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(
        (envelope["result"]["width"].as_u64(), envelope["result"]["height"].as_u64()),
        (Some(100), Some(50))
    );
    assert_eq!(
        envelope["result"]["area"],
        json!({ "x": 100.0, "y": 100.0, "width": 100.0, "height": 50.0 }),
        "the host drew the area the kernel asked for"
    );
}

#[test]
fn a_kernel_tile_after_a_page_insert_is_measured_on_its_page_as_it_now_sits() {
    // `call_host` reads a tile's page box through the session's index, one
    // page at a time (it takes `&Host`, so it issues no `pages.list`). An
    // insert in front of the page moves its index; the read follows it, and
    // the second tile lands where the first did.
    use epdf_viewer_host::translate::call_host;
    let mut host = shifted_host();
    let tile = json!({ "target": { "kind": "rect",
        "rect": { "left": 150.0, "right": 250.0, "top": 632.0, "bottom": 582.0 } } });
    let render = |host: &mut epdf_engine::host::Host, job: u64| {
        let call =
            translate("d", "page.render.image", Some(3), std::slice::from_ref(&tile), job).unwrap();
        let response = call_host(host, &call.request.to_string(), call.payloads);
        let envelope: Value = serde_json::from_str(&response.json).unwrap();
        envelope["result"]["area"].clone()
    };
    let before = render(&mut host, 2);
    assert_eq!(before, json!({ "x": 100.0, "y": 100.0, "width": 100.0, "height": 50.0 }));

    let insert = translate(
        "d",
        "pages.insertBlank",
        None,
        &[json!({ "size": { "width": 100.0, "height": 100.0 } }), json!(0)],
        3,
    )
    .unwrap();
    let inserted = call_host(&mut host, &insert.request.to_string(), insert.payloads);
    assert!(!inserted.json.contains("\"error\""), "{}", inserted.json);

    assert_eq!(render(&mut host, 4), before, "page 3 moved to index 1 and kept its box");
}

#[test]
fn a_pages_text_geometry_reaches_the_kernel_in_the_files_coordinates_and_its_spelling() {
    // The host answers page space with upstream's names (Task 8.9);
    // `call_host` turns it back into what the kernel's selection decodes
    // until Task 13 ports it.
    use epdf_plugin_selection::text_layout::{PageGeometryRun, PageGeometrySnapshot};
    use epdf_viewer_host::translate::call_host;

    let mut host = epdf_engine::host::Host::new();
    let open = json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d1" });
    let opened = call_host(
        &mut host,
        &open.to_string(),
        vec![test_support::fixture(test_support::ROTATED_TEXT)],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    let list = json!({ "kind": "pages.list", "jobId": 2, "docId": "d1" });
    let pages: Value =
        serde_json::from_str(&host.call(&list.to_string(), Vec::new()).json).unwrap();
    let first = &pages["result"]["pages"][0];
    let pon = first["ref"]["objectNumber"].as_u64().expect("a first page") as u32;
    let visible_top = first["pdfCropBox"]["top"].as_f64().expect("a crop box");

    let wire = json!({ "kind": "pages.geometry", "jobId": 3, "docId": "d1",
        "page": { "kind": "objectNumber", "objectNumber": pon } });
    let in_page: Value =
        serde_json::from_str(&host.call(&wire.to_string(), Vec::new()).json).unwrap();
    let in_page = &in_page["result"];

    let call = translate("d1", "page.geometry.read", Some(pon), &[], 4).unwrap();
    let response = call_host(&mut host, &call.request.to_string(), call.payloads);
    let kernel = decode(&call.decode, response).unwrap().value;
    let snapshot: PageGeometrySnapshot =
        serde_json::from_value(kernel.clone()).expect("the selection plugin decodes it");

    assert_eq!(snapshot.runs.len(), in_page["runs"].as_array().unwrap().len());
    let mut rotated = 0;
    for (run, page_run) in snapshot.runs.iter().zip(in_page["runs"].as_array().unwrap()) {
        match run {
            PageGeometryRun::Upright(run) => {
                assert_eq!(run.char_start as u64, page_run["start"].as_u64().unwrap());
                if page_run["rect"] == json!({ "x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0 }) {
                    // A run with no box keeps the zeroed box, not the page's
                    // corner.
                    assert_eq!(run.rect.top, 0.0);
                    assert_eq!(run.rect.left, 0.0);
                } else {
                    // y-up again: the top edge is the visible top less the
                    // page-space y.
                    let y = page_run["rect"]["y"].as_f64().unwrap();
                    assert!((run.rect.top - (visible_top - y)).abs() < 1e-9);
                }
            }
            PageGeometryRun::Rotated(run) => {
                rotated += 1;
                assert_eq!(run.char_start as u64, page_run["start"].as_u64().unwrap());
                // Radians counter-clockwise, (-π, π], from degrees clockwise.
                let degrees = page_run["rotation"].as_f64().unwrap();
                assert!(
                    run.rotation > -std::f64::consts::PI && run.rotation <= std::f64::consts::PI
                );
                let back = (-run.rotation.to_degrees()).rem_euclid(360.0);
                assert!((back - degrees).abs() < 1e-9 || (back - degrees).abs() > 360.0 - 1e-9);
            }
        }
    }
    assert!(rotated > 0, "rotated_text.pdf has turned runs");
    assert!(kernel["runs"][0].get("charStart").is_some(), "{}", kernel["runs"][0]);
}
