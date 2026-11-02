mod common;
use common::*;
use epdf_capi::*;
use serde_json::json;

#[test]
fn bytes_open_transfers_a_document_and_reads_page_metadata_text_and_geometry() {
    let engine = Engine::new(json!({}));
    let document = engine.open(PLAIN);
    let (reply, pages, _) = engine.call(document, 3, json!({}), None);
    assert_eq!(reply.document, 0);
    let pages = pages["result"]["pages"].as_array().expect("page array");
    assert!(!pages.is_empty());
    assert_eq!(pages[0]["index"], 0);
    assert!(pages[0]["size"]["width"].as_f64().unwrap() > 0.0);
    let page = pages[0]["ref"].clone();
    for operation in [4, 5] {
        let (reply, response, payloads) =
            engine.call(document, operation, json!({"page":page}), None);
        assert_eq!(reply.document, 0);
        assert!(response.get("error").is_none(), "{response}");
        assert!(response.get("result").is_some());
        assert!(payloads.is_empty());
    }
    let (status, job) = engine.submit(document, 2, json!({}), None);
    assert_eq!(status, 0);
    assert_eq!(engine.submit(document, 3, json!({}), None).0, 3);
    let mut reply = Reply::default();
    assert_eq!(unsafe { epdf_job_wait(job, u32::MAX, &mut reply) }, 0);
    result(reply.result);
    epdf_job_release(job);
    assert_eq!(engine.submit(document, 3, json!({}), None).0, 3);
}

#[test]
fn password_failures_release_parked_bytes_and_return_typed_errors() {
    let engine = Engine::new(json!({"limits":{"maxRetainedInputBytes":LOCKED.len()+16}}));
    for _ in 0..3 {
        for (password, code) in
            [(None, "DocPasswordRequired"), (Some("wrong"), "DocPasswordIncorrect")]
        {
            let (reply, response, _) =
                engine.call(0, 1, json!({"password":password}), Some(LOCKED));
            assert_eq!(reply.document, 0);
            assert_eq!(response["error"]["code"], code, "{response}");
        }
    }
    let (reply, response, _) = engine.call(0, 1, json!({"password":"user"}), Some(LOCKED));
    assert_ne!(reply.document, 0, "{response}");
}

#[test]
fn malformed_pdf_is_a_completed_error_and_document_handles_are_engine_scoped() {
    let first = Engine::new(json!({}));
    let second = Engine::new(json!({}));
    let (_, response, _) = first.call(0, 1, json!({}), Some(b"not a PDF"));
    assert!(response.get("error").is_some());
    let document = first.open(PLAIN);
    assert_eq!(second.submit(document, 3, json!({}), None).0, 3);
    assert_eq!(first.submit(0, 3, json!({}), None).0, 3);
}

#[test]
fn unsupported_operations_and_caller_routing_fields_are_refused() {
    let engine = Engine::new(json!({}));
    for operation in [0, 8, u32::MAX] {
        assert_eq!(engine.submit(0, operation, json!({}), None).0, 2);
    }
    for key in [
        "kind",
        "docId",
        "layerName",
        "authority",
        "effect",
        "scope",
        "identity",
        "jobId",
        "opId",
        "objectNumberFloor",
        "grants",
        "artifactPath",
        "reserveObjectNumbers",
        "objectNumberAuthority",
        "signedDocumentPolicy",
    ] {
        let args = json!({key:"untrusted"});
        assert_eq!(engine.submit(0, 1, args, Some(PLAIN)).0, 2, "{key}");
    }
}

#[test]
fn full_and_region_rgba_renders_preserve_shifted_crop_space_at_every_rotation() {
    let engine = Engine::new(json!({}));
    let document = engine.open(&rotated_crop_pdf());
    let (_, listed, _) = engine.call(document, 3, json!({}), None);
    let pages = listed["result"]["pages"].as_array().unwrap();
    for (index, page) in pages.iter().enumerate() {
        assert_eq!(page["rotation"], index * 90);
        assert_eq!(page["size"]["width"], 160.0);
        assert_eq!(page["size"]["height"], 80.0);
        assert_eq!(page["pdfCropBox"]["left"], -10.0);
        for rotation in [0, 90, 180, 270] {
            for region in
                [None, Some(json!({"kind":"rect","rect":{"x":5,"y":5,"width":40,"height":20}}))]
            {
                let (width, height, area) = if region.is_some() {
                    (80, 40, json!({"x":5.0,"y":5.0,"width":40.0,"height":20.0}))
                } else {
                    (320, 160, json!({"x":0.0,"y":0.0,"width":160.0,"height":80.0}))
                };
                let options = json!({"target":region.unwrap_or(json!({"kind":"page"})),"rotation":rotation,"viewport":{"kind":"scale","scale":2},"background":"transparent","includeAnnotations":false});
                let (_, response, payloads) =
                    engine.call(document, 6, json!({"page":page["ref"],"options":options}), None);
                assert!(response.get("error").is_none(), "{response}");
                let raster = &response["result"]["raster"];
                let (width, height) = if rotation == 90 || rotation == 270 {
                    (height, width)
                } else {
                    (width, height)
                };
                assert_eq!(raster["width"], width);
                assert_eq!(raster["height"], height);
                let stride = raster["stride"].as_u64().unwrap() as usize;
                assert!(stride >= width * 4);
                assert_eq!(payloads.len(), 1);
                assert_eq!(payloads[0].len(), stride * height);
                assert_eq!(response["result"]["area"], area);
                assert!(payloads[0].as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 0));
            }
        }
    }
}

#[test]
fn render_limits_and_invalid_scales_refuse_before_raster_output() {
    let engine = Engine::new(json!({"limits":{"maxOutputPixels":10}}));
    let document = engine.open(PLAIN);
    let (_, listed, _) = engine.call(document, 3, json!({}), None);
    let page = listed["result"]["pages"][0]["ref"].clone();
    for options in [
        json!({"viewport":{"kind":"scale","scale":1}}),
        json!({"viewport":{"kind":"scale","scale":1e200}}),
        json!({"viewport":{"kind":"scale","scale":0}}),
        json!({"viewport":{"kind":"scale","scale":null}}),
        json!({"rotation":45}),
        json!({"usage":"print"}),
        json!({"viewport":{"kind":"width","width":5}}),
    ] {
        let (_, response, payloads) =
            engine.call(document, 6, json!({"page":page,"options":options}), None);
        assert!(response.get("error").is_some(), "{response}");
        assert!(payloads.is_empty());
    }
}

#[test]
fn literal_search_returns_bounded_continuations_and_refuses_other_query_or_document() {
    let engine = Engine::new(json!({}));
    let bytes = rotated_crop_pdf();
    let first = engine.open(&bytes);
    let second = engine.open(&bytes);
    let (_, response, _) = engine.call(
        first,
        7,
        json!({"request":{"text":"needle","snippets":true,"limit":{"pages":1,"matches":1}}}),
        None,
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["pageCount"], 4);
    assert_eq!(response["result"]["pagesSearched"], 1);
    let matches = response["result"]["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0]["count"].as_u64().unwrap() > 0);
    assert!(!matches[0]["segments"].as_array().unwrap().is_empty());
    let cursor = response["result"]["nextCursor"].as_str().unwrap();
    for (document, text) in [(first, "other"), (second, "needle")] {
        let (_, response, _) = engine.call(
            document,
            7,
            json!({"request":{"text":text,"snippets":true,"cursor":cursor,"limit":{"pages":1}}}),
            None,
        );
        assert_eq!(response["error"]["code"], "InvalidArg", "{response}");
    }
    let mut cursor = Some(cursor.to_owned());
    let mut scanned = 1;
    while let Some(current) = cursor {
        let (_,response,_)=engine.call(first,7,json!({"request":{"text":"needle","snippets":true,"cursor":current,"limit":{"pages":1}}}),None);
        assert!(response.get("error").is_none(), "{response}");
        scanned += 1;
        assert_eq!(response["result"]["pagesSearched"], scanned);
        cursor = response["result"]["nextCursor"].as_str().map(str::to_owned);
    }
    assert_eq!(scanned, 4);
    let (_, response, _) =
        engine.call(first, 7, json!({"request":{"text":"needle","regex":true}}), None);
    assert_eq!(response["error"]["code"], "InvalidArg");
}

#[test]
fn transparent_raster_is_straight_rgba_without_a_second_alpha_conversion() {
    let engine = Engine::new(json!({}));
    let bytes = translucent_pdf();
    let document = engine.open(&bytes);
    let (_, listed, _) = engine.call(document, 3, json!({}), None);
    let page = &listed["result"]["pages"][0]["ref"];
    let (_, response, payloads) = engine.call(
        document,
        6,
        json!({"page":page,"options":{"background":"transparent","includeAnnotations":false}}),
        None,
    );
    assert!(response.get("error").is_none(), "{response}");
    let pixels = payloads[0].as_chunks::<4>().0;
    assert!(
        pixels.iter().any(|pixel| pixel[0] > 240
            && pixel[1] < 10
            && pixel[2] < 10
            && (120..=136).contains(&pixel[3])),
        "missing straight-alpha translucent red pixel; first nonzero {:?}",
        pixels.iter().find(|pixel| pixel[3] > 0)
    );
}
