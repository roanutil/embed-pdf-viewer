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
            "pageObjectNumber": 12,
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
    let error = translate("d1", "signatures.prepare", None, &[], 10).err().unwrap();
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
fn a_page_list_gives_each_page_its_ref_and_keeps_the_host_fields() {
    let json = r#"{"result":{"pageCount":1,"pages":[{"boxes":{"crop":{"bottom":0.0,"left":0.0,"right":612.0,"top":792.0},"media":{"bottom":0.0,"left":0.0,"right":612.0,"top":792.0}},"index":0,"label":null,"pageObjectNumber":2,"rotation":0,"size":{"height":792.0,"width":612.0},"userUnit":1.0}]}}"#;
    let page = &decode(&Decode::PageList, answer(json)).unwrap().value["pages"][0];
    assert_eq!(page["ref"], json!({"kind": "objectNumber", "pageObjectNumber": 2}));
    assert_eq!(page["pageObjectNumber"], 2);
    assert_eq!(page["size"], json!({"height": 792.0, "width": 612.0}));
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
fn a_search_start_page_travels_as_its_object_number() {
    let request = json!({
        "query": {"text": "the"},
        "mode": "full",
        "startPage": {"kind": "objectNumber", "pageObjectNumber": 43}
    });
    let call = translate("d1", "search.query", None, &[request], 11).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "search.query", "jobId": 11, "docId": "d1",
            "request": {"query": {"text": "the"}, "mode": "full", "startPage": 43}
        })
    );
    assert!(matches!(call.decode, Decode::SearchSlice));
    let plain = json!({"query": {"text": "the"}, "mode": "full"});
    let call = translate("d1", "search.query", None, std::slice::from_ref(&plain), 12).unwrap();
    assert_eq!(call.request["request"], plain);
}

#[test]
fn a_search_start_page_that_is_not_a_page_ref_is_invalid_input() {
    let request = json!({"query": {"text": "the"}, "startPage": {"index": 1}});
    let error = translate("d1", "search.query", None, &[request], 13).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

/// The host's `search.query` answer for "Report" over the report fixture.
const SEARCH_SLICE: &str = r#"{"result":{"matches":[{"charCount":6,"charStart":6156,"pageObjectNumber":202,"segments":[{"advance":1,"quad":{"p1":{"x":354.4305114746094,"y":196.280029296875},"p2":{"x":375.85125732421875,"y":196.280029296875},"p3":{"x":354.4305114746094,"y":185.4755096435547},"p4":{"x":375.85125732421875,"y":185.4755096435547}},"rect":{"bottom":185.4755096435547,"left":354.4305114746094,"right":375.85125732421875,"top":196.280029296875}}],"snippet":{"matchLength":6,"matchStart":47,"text":"then loads and runs each program 10  times and reports the average time taken by each. We ran 4"}}],"nextCursor":null,"scannedPages":14,"totalPages":14}}"#;

#[test]
fn a_search_slice_gives_each_hit_its_page_and_keeps_the_host_fields() {
    let slice = decode(&Decode::SearchSlice, answer(SEARCH_SLICE)).unwrap().value;
    let hit = &slice["matches"][0];
    assert_eq!(hit["page"], json!({"kind": "objectNumber", "pageObjectNumber": 202}));
    assert_eq!(hit["pageObjectNumber"], 202);
    assert_eq!(hit["charStart"], 6156);
    assert_eq!(hit["snippet"]["matchStart"], 47);
    assert_eq!(slice["nextCursor"], Value::Null);
    assert_eq!(slice["totalPages"], 14);
}

#[test]
fn a_search_hit_without_its_page_is_an_error() {
    let json = r#"{"result":{"matches":[{"charCount":1,"charStart":0,"segments":[]}],"nextCursor":null,"scannedPages":1,"totalPages":1}}"#;
    assert!(decode(&Decode::SearchSlice, answer(json)).is_err());
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
    let thread = KernelThread::spawn(|post| Ok(NativeEngine::new(worker, post))).unwrap();
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
fn a_locked_open_rejects_and_leaves_no_session_on_the_host() {
    let worker = EngineWorker::spawn();
    let code = on_kernel(worker.clone(), |engine, tx| {
        engine.open(open_input(test_support::ENCRYPTED), None).on_settle(move |doc| {
            let _ = tx.send(doc.as_ref().err().and_then(|e| e.code().map(str::to_owned)));
        });
    });
    assert_eq!(code.as_deref(), Some("DocPasswordRequired"));
    // The engine's own close went out before the open rejected, and the
    // worker is first in, first out, so a second close finds nothing.
    let (tx, rx) = oneshot::channel();
    worker.submit(
        json!({ "kind": "close", "jobId": 0, "docId": "doc-1" }).to_string(),
        Vec::new(),
        Box::new(move |response| {
            let _ = tx.send(response.json);
        }),
    );
    let answer: Value = serde_json::from_str(&futures::executor::block_on(rx).unwrap()).unwrap();
    assert_eq!(answer, json!({ "result": { "closedSessions": 0 } }));
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
    assert!(!maps("signatures"));
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

fn page_ref(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": pon })
}

#[test]
fn a_rotation_travels_as_a_whole_number() {
    let args = [json!([page_ref(3), page_ref(4)]), json!(90.0)];
    let call = translate("d1", "pages.rotate", None, &args, 11).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.rotate", "jobId": 11, "docId": "d1",
            "pageObjectNumbers": [3, 4], "rotation": 90
        })
    );
    assert!(call.request["rotation"].is_u64());
    assert!(matches!(call.decode, Decode::PageMutation));
}

#[test]
fn a_move_carries_its_destination() {
    let args = [json!([page_ref(3)]), json!(0.0)];
    let call = translate("d1", "pages.move", None, &args, 12).unwrap();
    assert_eq!(call.request["pageObjectNumbers"], json!([3]));
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

const PAGE: &str = r#"{"boxes":{},"index":0,"label":null,"pageObjectNumber":2,"rotation":90,"size":{"height":792.0,"width":612.0},"userUnit":1.0}"#;

#[test]
fn a_page_mutation_gives_every_layout_page_its_ref() {
    let json = format!(
        r#"{{"result":{{"event":{{"layout":{{"pageCount":1,"pages":[{PAGE}]}},"type":"pages.rotated"}},"mutationSequence":2,"result":{{"layout":{{"pageCount":1,"pages":[{PAGE}]}}}}}}}}"#
    );
    let decoded = decode(&Decode::PageMutation, answer(&json)).unwrap();
    assert_eq!(decoded.value["layout"]["pages"][0]["ref"], page_ref(2));
    let event = decoded.event.unwrap();
    assert_eq!(event["type"], "pages.rotated");
    assert_eq!(event["layout"]["pages"][0]["ref"], page_ref(2));
}

#[test]
fn an_insert_result_names_its_pages_as_refs() {
    let json = format!(
        r#"{{"result":{{"event":{{"insertedPageObjectNumbers":[2],"layout":{{"pageCount":1,"pages":[{PAGE}]}},"type":"pages.inserted"}},"mutationSequence":3,"result":{{"insertedPageObjectNumbers":[2],"layout":{{"pageCount":1,"pages":[{PAGE}]}}}}}}}}"#
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
