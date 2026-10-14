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
        // `a_widget_link_sends_the_widgets_object_number`.
        "forms.importData",
        "forms.attachWidget",
        "forms.detachWidget",
        // A scope, covered by `a_pages_scope_goes_out_as_object_numbers`.
        "redaction.apply",
        // An index and two byte payloads, covered by
        // `the_reads_carry_their_arguments`,
        // `complete_moves_the_cms_into_a_payload` and
        // `a_visual_fill_moves_the_pdf_into_a_payload`.
        "signatures.revisionBytes",
        "signatures.complete",
        "forms.setSignatureAppearance",
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

#[test]
fn a_page_annotation_list_carries_its_page_and_answers_the_snapshot() {
    let call = translate("d1", "page.annotations.list", Some(5), &[], 11).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "annotations.listRawPage", "jobId": 11, "docId": "d1", "pageObjectNumber": 5 })
    );
    assert!(matches!(call.decode, Decode::Annotations));
    let error = translate("d1", "page.annotations.list", None, &[], 12).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn a_listed_annotations_ref_and_page_arrive_nested() {
    let json = r#"{"result":{"annotations":[{"ref":{"kind":"objectNumber","pageObjectNumber":3,"annotObjectNumber":17},"page":3}]}}"#;
    let call = translate("d1", "page.annotations.list", Some(3), &[], 13).unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap().value;
    let page3 = json!({ "kind": "objectNumber", "pageObjectNumber": 3 });
    assert_eq!(
        decoded["annotations"][0]["ref"],
        json!({ "kind": "objectNumber", "page": page3.clone(), "annotObjectNumber": 17 })
    );
    assert_eq!(decoded["annotations"][0]["page"], page3);
}

#[test]
fn a_form_field_ref_is_not_mistaken_for_an_annotation_ref() {
    let mut value = json!({ "ref": { "kind": "objectNumber", "fieldObjectNumber": 9 } });
    let before = value.clone();
    epdf_viewer_host::translate::nest_refs(&mut value);
    epdf_viewer_host::translate::flatten_refs(&mut value);
    assert_eq!(value, before);
}

/// Page 1 of `features.pdf` as the native engine lists it, trimmed: every
/// page the host spells as `pageObjectNumber` (the DTO's own, its ref's, a
/// destination's, the page state's and its revision's) arrives as the
/// `page: PageRef` the kernel's vectors spell.
#[test]
fn every_page_a_listed_snapshot_names_arrives_as_a_page_ref() {
    let json = r#"{"result":{"annotations":[{"pageObjectNumber":5,"subtype":"link",
        "ref":{"annotObjectNumber":8,"kind":"objectNumber","pageObjectNumber":5},
        "target":{"kind":"goto","destination":{"kind":"xyz","left":0.0,"pageObjectNumber":7,"top":400.0,"zoom":null}},
        "actions":{"activate":{"root":{"type":"goto","subtype":"GoTo","next":[],
            "destination":{"kind":"fitR","pageObjectNumber":7,"left":1.0,"bottom":2.0,"right":3.0,"top":4.0}}}}}],
        "pageState":{"pageObjectNumber":5,"revision":{"docSessionId":"s","generation":0,"pageObjectNumber":5}}}}"#;
    let call = translate("d1", "page.annotations.list", Some(5), &[], 14).unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap().value;
    let page = |n: u32| json!({ "kind": "objectNumber", "pageObjectNumber": n });
    let dto = &decoded["annotations"][0];
    assert_eq!(dto["page"], page(5));
    assert!(dto.get("pageObjectNumber").is_none(), "{dto}");
    assert_eq!(
        dto["ref"],
        json!({ "kind": "objectNumber", "page": page(5), "annotObjectNumber": 8 })
    );
    assert_eq!(
        dto["target"]["destination"],
        json!({ "kind": "xyz", "page": page(7), "left": 0.0, "top": 400.0, "zoom": null })
    );
    assert_eq!(dto["actions"]["activate"]["root"]["destination"]["page"], page(7));
    assert_eq!(decoded["pageState"]["page"], page(5));
    assert_eq!(decoded["pageState"]["revision"]["page"], page(5));
    // A page ref itself keeps its spelling, and flattening undoes nesting.
    let mut back = decoded.clone();
    epdf_viewer_host::translate::flatten_refs(&mut back);
    assert_eq!(back["annotations"][0]["target"]["destination"]["pageObjectNumber"], 7);
    assert_eq!(back["annotations"][0]["ref"]["pageObjectNumber"], 5);
    assert_eq!(back["pageState"]["revision"]["pageObjectNumber"], 5);
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
fn refs_are_nested_on_the_way_in_and_flattened_on_the_way_out() {
    let nested = json!({ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17 });
    let call =
        translate("d1", "page.annotations.delete", Some(3), std::slice::from_ref(&nested), 12)
            .unwrap();
    assert_eq!(
        call.request["ref"],
        json!({ "kind": "objectNumber", "pageObjectNumber": 3, "annotObjectNumber": 17 })
    );
    let mut back = json!({ "created": {
        "ref": { "kind": "objectNumber", "pageObjectNumber": 3, "annotObjectNumber": 17 },
        "page": 3 } });
    epdf_viewer_host::translate::nest_refs(&mut back);
    assert_eq!(back["created"]["ref"], nested);
    assert_eq!(back["created"]["page"], json!({ "kind": "objectNumber", "pageObjectNumber": 3 }));
}

#[test]
fn a_stamp_drafts_bytes_travel_as_a_resource_payload() {
    let draft = json!({ "subtype": "stamp", "source": { "data": { "$bytes": "iVBORw==" }, "mimeType": "image/png" } });
    let call = translate("d1", "page.annotations.create", Some(3), &[draft], 13).unwrap();
    assert_eq!(call.request["kind"], "annotations.create");
    assert_eq!(call.request["pageObjectNumber"], 3);
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
    assert_eq!(call.request["ref"], json!({ "kind": "nm", "pageObjectNumber": 3, "nm": "abc" }));
    assert_eq!(call.request["patch"], json!({ "contents": "x" }));
    assert!(matches!(call.decode, Decode::AnnotationMutation));
}

#[test]
fn list_raw_all_answers_nested_refs() {
    let json = r#"{"result":{"pages":[{"pageState":{"page":3},"annotations":[{"ref":{"kind":"objectNumber","pageObjectNumber":3,"annotObjectNumber":17},"page":3}]}]}}"#;
    let call = translate("d1", "annotations.listRawAll", None, &[], 16).unwrap();
    let decoded = decode(&call.decode, answer(json)).unwrap().value;
    assert_eq!(
        decoded["pages"][0]["pageState"]["page"],
        json!({ "kind": "objectNumber", "pageObjectNumber": 3 })
    );
    assert_eq!(decoded["pages"][0]["annotations"][0]["ref"]["page"]["pageObjectNumber"], 3);
}

/// The kernel's annotation numbers are JavaScript numbers (`17.0`); the
/// host's `AnnotationRef` and `RevisionToken` parse whole integers.
#[test]
fn a_refs_javascript_numbers_go_out_whole() {
    let index = json!({ "kind": "index",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "index": 2.0,
        "revision": { "docSessionId": "s",
            "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "generation": 4.0 } });
    let call = translate("d1", "page.annotations.delete", Some(3), &[index], 17).unwrap();
    assert_eq!(
        call.request["ref"],
        json!({ "kind": "index", "pageObjectNumber": 3, "index": 2,
            "revision": { "docSessionId": "s", "pageObjectNumber": 3, "generation": 4 } })
    );
    let object = json!({ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17.0 });
    let call =
        translate("d1", "page.annotations.update", Some(3), &[object, json!({})], 18).unwrap();
    assert_eq!(call.request["ref"]["annotObjectNumber"], json!(17));
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
    assert_eq!(call.request["pageObjectNumber"], 3);
    assert_eq!(
        call.request["ref"],
        json!({ "kind": "objectNumber", "pageObjectNumber": 3, "annotObjectNumber": 17 })
    );
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
/// draft or patch, not in `ref`; the host parses both flat and whole.
#[test]
fn refs_inside_a_draft_or_patch_go_out_flat_and_whole() {
    let page3 = json!({ "kind": "objectNumber", "pageObjectNumber": 3 });
    let draft = json!({ "subtype": "text", "index": 2.5,
        "inReplyTo": { "kind": "objectNumber", "page": page3.clone(), "annotObjectNumber": 17.0 } });
    let call = translate("d1", "page.annotations.create", Some(3), &[draft], 21).unwrap();
    assert_eq!(
        call.request["draft"]["inReplyTo"],
        json!({ "kind": "objectNumber", "pageObjectNumber": 3, "annotObjectNumber": 17 })
    );
    // A draft member that only shares a ref member's name is left alone.
    assert_eq!(call.request["draft"]["index"], json!(2.5));
    let target = json!({ "kind": "objectNumber", "page": page3.clone(), "annotObjectNumber": 5.0 });
    let patch = json!({ "target": { "kind": "goto",
        "destination": { "kind": "fit", "page": { "kind": "objectNumber", "pageObjectNumber": 7 } } } });
    let call = translate("d1", "page.annotations.update", Some(3), &[target, patch], 22).unwrap();
    assert_eq!(
        call.request["patch"]["target"]["destination"],
        json!({ "kind": "fit", "pageObjectNumber": 7 })
    );
}

// ── forms (plan 06) ───────────────────────────────────────────────────────

#[test]
fn a_form_list_gives_each_widget_its_ref_and_page() {
    let json = r#"{"result":{"formKind":"acroform","needsAppearances":false,"calculationOrder":[],"fields":[{"ref":{"kind":"objectNumber","fieldObjectNumber":4},"family":"checkbox","widgets":[{"annotObjectNumber":9,"pageObjectNumber":2,"onState":"On","exportValue":"Yes","checked":true}]}]}}"#;
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
fn a_widget_placement_goes_out_with_its_object_number() {
    let draft = json!({ "family": "text", "name": "f",
        "widget": { "page": { "kind": "objectNumber", "pageObjectNumber": 2 },
            "rect": { "left": 0, "bottom": 0, "right": 10, "top": 10 } } });
    let call = translate("d1", "forms.createField", None, &[draft], 22).unwrap();
    assert_eq!(call.request["draft"]["widget"]["pageObjectNumber"], 2);
    assert!(call.request["draft"]["widget"].get("page").is_none());
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
    assert!(matches!(call.decode, Decode::FormMutation));
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
    assert!(matches!(call.decode, Decode::FormMutation));
}

#[test]
fn a_widget_link_sends_the_widgets_object_number() {
    let widget = json!({ "kind": "objectNumber", "annotObjectNumber": 9.0,
        "page": { "kind": "objectNumber", "pageObjectNumber": 2 } });
    let field = json!({ "kind": "objectNumber", "fieldObjectNumber": 4 });
    for (path, kind) in
        [("forms.attachWidget", "forms.attachWidget"), ("forms.detachWidget", "forms.detachWidget")]
    {
        let call = translate("d1", path, None, &[field.clone(), widget.clone()], 27).unwrap();
        assert_eq!(
            call.request,
            json!({ "kind": kind, "jobId": 27, "docId": "d1",
            "ref": field, "widget": 9 })
        );
    }
}

#[test]
fn a_form_mutation_without_a_sequence_answers_the_bare_result() {
    let call = translate("d1", "forms.applyEffects", None, &[json!([])], 28).unwrap();
    let decoded = decode(&call.decode, answer(r#"{"result":{"results":[]}}"#)).unwrap();
    assert_eq!(decoded.value, json!({ "results": [] }));
    assert!(decoded.event.is_none());
}

#[test]
fn a_form_mutation_nests_the_widgets_of_its_result_and_event() {
    let call = translate("d1", "forms.setValue", None, &[json!({}), json!({})], 29).unwrap();
    let field = r#"{"ref":{"kind":"objectNumber","fieldObjectNumber":4},"widgets":[{"annotObjectNumber":9,"pageObjectNumber":2}]}"#;
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
fn a_field_edit_answers_its_field_under_field_as_the_kernel_reads_it() {
    // The host answers createField, updateField and the widget links with
    // the bare field, and spreads it into the event beside `type`; the
    // plugin reads `field` from both, as the TypeScript engine sends them.
    for path in
        ["forms.createField", "forms.updateField", "forms.attachWidget", "forms.detachWidget"]
    {
        let call = translate("d1", path, None, &[json!({}), json!({ "annotObjectNumber": 9 })], 30)
            .unwrap();
        let field = r#""ref":{"kind":"objectNumber","fieldObjectNumber":4},"name":"a","widgets":[{"annotObjectNumber":9,"pageObjectNumber":2}]"#;
        let json = format!(
            r#"{{"result":{{"result":{{{field}}},"mutationSequence":3,"event":{{"type":"form.fieldCreated","origin":{{"kind":"local"}},{field}}}}}}}"#
        );
        let decoded = decode(&call.decode, answer(&json)).unwrap();
        assert_eq!(decoded.value["field"]["name"], "a", "{path}");
        assert_eq!(decoded.value["field"]["widgets"][0]["ref"]["annotObjectNumber"], 9, "{path}");
        let event = decoded.event.unwrap();
        assert_eq!(event["type"], "form.fieldCreated");
        assert_eq!(event["origin"], json!({ "kind": "local" }));
        assert_eq!(event["field"]["name"], "a", "{path}");
        assert!(event.get("name").is_none(), "{path}");
    }
}

#[test]
fn an_effects_batch_publishes_effects_applied() {
    // The host finishes applyEffects as `form.valueChanged`; the TypeScript
    // engine publishes `form.effectsApplied`, which carries no `field`.
    let call = translate("d1", "forms.applyEffects", None, &[json!([])], 31).unwrap();
    let json = r#"{"result":{"result":{"results":[],"changedWidgets":[]},"mutationSequence":4,"event":{"type":"form.valueChanged","origin":{"kind":"local"},"results":[],"changedWidgets":[]}}}"#;
    let decoded = decode(&call.decode, answer(json)).unwrap();
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
fn a_pages_scope_goes_out_as_object_numbers() {
    // Page object numbers arrive as JavaScript numbers.
    let scope = json!({ "kind": "pages", "pages": [
        { "kind": "objectNumber", "pageObjectNumber": 3.0 },
        { "kind": "objectNumber", "pageObjectNumber": 7 } ] });
    let call = translate("d1", "redaction.apply", None, &[scope], 30).unwrap();
    assert_eq!(
        call.request,
        json!({ "kind": "redaction.apply", "jobId": 30, "docId": "d1",
            "scope": { "kind": "pages", "pageObjectNumbers": [3, 7] } })
    );
    assert!(call.request["scope"]["pageObjectNumbers"][0].is_u64());
}

#[test]
fn an_annotations_scope_goes_out_with_flat_whole_refs() {
    let scope = json!({ "kind": "annotations", "refs": [{ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "pageObjectNumber": 3 }, "annotObjectNumber": 17.0 }] });
    let call = translate("d1", "redaction.apply", None, &[scope], 31).unwrap();
    assert_eq!(
        call.request["scope"],
        json!({ "kind": "annotations", "refs": [{ "kind": "objectNumber", "pageObjectNumber": 3, "annotObjectNumber": 17 }] })
    );
    assert!(call.request["scope"]["refs"][0]["annotObjectNumber"].is_u64());
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
    let json = r#"{"result":{"result":{"scope":{"kind":"pages","pageObjectNumbers":[3]},"results":[{"pageObjectNumber":3,"status":"applied","removedAnnotationCount":2}],"removedAnnotationCount":2},"mutationSequence":4,"event":{"type":"redaction.applied","origin":{"kind":"local","serverId":null},"scope":{"kind":"pages","pageObjectNumbers":[3]},"results":[{"pageObjectNumber":3,"status":"applied","removedAnnotationCount":2}],"removedAnnotationCount":2}}}"#;
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
    let json = r#"{"result":{"result":{"scope":{"kind":"annotations","refs":[{"kind":"objectNumber","pageObjectNumber":3,"annotObjectNumber":17}]},"results":[{"pageObjectNumber":3,"status":"applied","removedAnnotationCount":0}],"removedAnnotationCount":0},"mutationSequence":5,"event":null}}"#;
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
