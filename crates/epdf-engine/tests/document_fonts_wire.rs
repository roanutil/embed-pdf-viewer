//! The five font kinds upstream added, on the wire: `fonts.list`,
//! `fonts.program`, `document.fonts.list`, `document.fonts.resolve` and
//! `document.fonts.program`, answered as `WorkerHost`'s handlers answer them
//! (`handleFontsProgram`, `handleDocumentFontsList`,
//! `handleDocumentFontsResolve`, `handleDocumentFontsProgram`), from
//! `FontRegistrar` and `DocumentFontReader`.
//!
//! `fonts_wire_corpus.rs` pins the same kinds against the TypeScript oracle
//! (`vectors/fonts/wire.json`). The facts asserted here are the fork's own,
//! read from Roboto and `embedded_orientations.pdf`, as `crates/pdfium/tests/
//! document_fonts.rs` reads them below this layer.
//!
//! PDFium's font registry is per thread, so every test runs on a thread of
//! its own.

use epdf_engine::host::Host;

const EMBEDDED: &str = "tooling/engine-conformance/fixtures/embedded_orientations.pdf";
const NO_FONTS: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";
const DOC_ID: &str = "doc-1";

fn on_a_fresh_thread<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(body).join().expect("the test thread panicked")
}

fn roboto() -> Vec<u8> {
    std::fs::read(
        test_support::workspace_root()
            .join("packages/engine/main/test/fixtures/Roboto-Regular.ttf"),
    )
    .expect("read Roboto-Regular.ttf")
}

/// One request, its envelope filled in; the answer's JSON and payloads.
fn call(
    host: &mut Host,
    mut request: serde_json::Value,
    payloads: Vec<Vec<u8>>,
) -> (serde_json::Value, Vec<Vec<u8>>) {
    // The document kinds name the open document unless they say otherwise.
    let kind = request["kind"].as_str().unwrap_or_default();
    if (kind.starts_with("document.") || kind.starts_with("pages.")) && request["docId"].is_null() {
        request["docId"] = DOC_ID.into();
    }
    let response = host.call_filled(&request.to_string(), payloads);
    (serde_json::from_str(&response.json).expect("every response is JSON"), response.payloads)
}

fn result(host: &mut Host, request: serde_json::Value) -> serde_json::Value {
    let (json, _) = call(host, request.clone(), Vec::new());
    assert!(json.get("error").is_none(), "{request}: {json}");
    json["result"].clone()
}

fn open(host: &mut Host, fixture: &str) {
    let (json, _) = call(
        host,
        serde_json::json!({ "kind": "open.fatMem", "docId": DOC_ID }),
        vec![test_support::fixture(fixture)],
    );
    assert!(json.get("error").is_none(), "open {fixture}: {json}");
}

fn register(host: &mut Host, key: &str) {
    let (json, _) = call(
        host,
        serde_json::json!({ "kind": "fonts.register", "fontKey": key, "familyName": "",
            "weight": 0, "italic": -1 }),
        vec![roboto()],
    );
    assert!(json.get("error").is_none(), "register {key}: {json}");
}

fn is_sha256(value: &serde_json::Value) -> bool {
    value.as_str().is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

#[test]
fn an_engine_with_no_fonts_lists_none_and_not_the_last_resort() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        let listed = result(&mut host, serde_json::json!({ "kind": "fonts.list" }));
        assert_eq!(listed, serde_json::json!({ "engineFonts": [], "fallbacks": [] }));
    });
}

#[test]
fn the_engine_lists_its_fonts_in_registration_order_with_their_programs() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        register(&mut host, "zeta");
        register(&mut host, "alpha");
        result(
            &mut host,
            serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "alpha", "tier": 1 }),
        );
        result(&mut host, serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "zeta" }));

        let listed = result(&mut host, serde_json::json!({ "kind": "fonts.list" }));
        let fonts = listed["engineFonts"].as_array().expect("engineFonts");
        let keys: Vec<_> = fonts.iter().map(|font| font["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["zeta", "alpha"], "registration order, not key order");
        let font = &fonts[0];
        assert_eq!(
            font["identity"],
            serde_json::json!({ "familyName": "Roboto", "weight": 400, "italic": false,
                "embeddingPermission": "installable", "editingAuthorized": true,
                "instanced": false })
        );
        assert_eq!(font["postScriptName"], "Roboto-Regular");
        assert_eq!(font["format"], "truetype");
        assert!(is_sha256(&font["sha256"]), "{font}");
        assert_eq!(font["served"], true, "an application's own font goes to browsers");
        assert_eq!(fonts[1]["sha256"], font["sha256"], "one program, registered twice");
        // By tier, then by the order added.
        assert_eq!(listed["fallbacks"], serde_json::json!(["zeta", "alpha"]));
    });
}

#[test]
fn a_served_fonts_program_is_its_bytes_by_hash() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        register(&mut host, "roboto");
        let listed = result(&mut host, serde_json::json!({ "kind": "fonts.list" }));
        let sha256 = listed["engineFonts"][0]["sha256"].as_str().expect("a hash").to_owned();

        let (json, payloads) = call(
            &mut host,
            serde_json::json!({ "kind": "fonts.program", "sha256": sha256 }),
            vec![],
        );
        assert_eq!(json["result"], serde_json::json!({ "bytes": { "payload": 0 } }), "{json}");
        assert_eq!(payloads, [roboto()]);

        let (json, _) =
            call(&mut host, serde_json::json!({ "kind": "fonts.program", "sha256": "ab" }), vec![]);
        assert_eq!(json["error"]["code"], "NotFound", "{json}");
        assert_eq!(json["error"]["message"], "no served font has the program ab");
    });
}

#[test]
fn clearing_forgets_the_listed_fonts() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        register(&mut host, "roboto");
        result(&mut host, serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "roboto" }));
        result(&mut host, serde_json::json!({ "kind": "fonts.clear" }));
        let listed = result(&mut host, serde_json::json!({ "kind": "fonts.list" }));
        assert_eq!(listed, serde_json::json!({ "engineFonts": [], "fallbacks": [] }));
        // The registry still works: the last resort registered again.
        register(&mut host, "roboto");
    });
}

#[test]
fn a_documents_font_list_facts_name_its_embedded_program() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, EMBEDDED);
        register(&mut host, "roboto");
        let listed = result(&mut host, serde_json::json!({ "kind": "document.fonts.list" }));
        let facts = &listed["facts"];
        assert_eq!(facts["fallbacks"], serde_json::json!([]));
        assert_eq!(facts["engineFonts"][0]["key"], "roboto");

        let faces = facts["faces"].as_array().expect("faces");
        assert_eq!(faces.len(), 1, "{faces:?}");
        let face = &faces[0];
        assert_eq!(
            (&face["familyName"], &face["weight"], &face["italic"]),
            (&serde_json::json!("Roboto"), &serde_json::json!(400), &serde_json::json!(false))
        );
        let program = &face["programs"][0];
        assert!(program["objectNumber"].as_u64().is_some_and(|n| n > 0), "{program}");
        assert_eq!(program["format"], "truetype");
        assert_eq!(program["embedded"], "whole");
        assert_eq!(program["postScriptName"], "Roboto-Regular");
        assert_eq!(program["permission"], "installable");
        assert_eq!(program["mapsUnicode"], true);
        assert!(is_sha256(&program["sha256"]));
        assert_eq!(program["displaySha256"], program["sha256"]);
        // Thousandths of the size, rounded; the descent negative.
        assert!(program["ascent"].as_f64().is_some_and(|a| a > 0.0 && a.fract() == 0.0));
        assert!(program["descent"].as_f64().is_some_and(|d| d < 0.0 && d.fract() == 0.0));
        let keys: Vec<&String> = program.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 10, "{keys:?}");
    });
}

#[test]
fn a_document_program_is_read_by_its_display_hash() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, EMBEDDED);
        let listed = result(&mut host, serde_json::json!({ "kind": "document.fonts.list" }));
        let program = listed["facts"]["faces"][0]["programs"][0].clone();
        let hash = program["displaySha256"].as_str().expect("a display hash").to_owned();

        let info = result(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.program", "displayHash": hash,
                "part": "info" }),
        );
        assert_eq!(info["bytes"], serde_json::Value::Null);
        assert_eq!(info["ascent"], program["ascent"]);
        assert_eq!(info["descent"], program["descent"]);
        let covers = info["covers"].as_array().expect("covers");
        let covered = |c: char| {
            covers.iter().any(|range| {
                let start = range["start"].as_u64().unwrap();
                (start..start + range["count"].as_u64().unwrap()).contains(&u64::from(c))
            })
        };
        assert!(covered('A'), "{covers:?}");

        let (json, payloads) = call(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.program", "displayHash": hash,
                "part": "data" }),
            vec![],
        );
        assert_eq!(json["result"]["bytes"], serde_json::json!({ "payload": 0 }), "{json}");
        assert_eq!(&payloads[0][..4], &[0, 1, 0, 0], "a TrueType sfnt");

        let unknown = "0".repeat(64);
        let (json, _) = call(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.program", "displayHash": unknown,
                "part": "info" }),
            vec![],
        );
        assert_eq!(json["error"]["code"], "NotFound", "{json}");
        assert_eq!(json["error"]["message"], format!("no font program {unknown}"));
    });
}

#[test]
fn an_engine_fonts_program_is_read_by_its_hash_served_or_not() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, NO_FONTS);
        register(&mut host, "roboto");
        let listed = result(&mut host, serde_json::json!({ "kind": "fonts.list" }));
        let hash = listed["engineFonts"][0]["sha256"].as_str().unwrap().to_owned();

        let info = result(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.program", "displayHash": hash,
                "part": "info" }),
        );
        // hhea ascender 1900 and descender -500 over 2048 units per em.
        assert_eq!(info["ascent"], 928.0);
        assert_eq!(info["descent"], -244.0);
        assert!(!info["covers"].as_array().unwrap().is_empty());

        let (json, payloads) = call(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.program", "displayHash": hash,
                "part": "data" }),
            vec![],
        );
        assert_eq!(json["result"]["bytes"], serde_json::json!({ "payload": 0 }), "{json}");
        assert_eq!(payloads, [roboto()]);
    });
}

#[test]
fn resolving_names_the_font_each_run_would_be_drawn_in() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, NO_FONTS);
        register(&mut host, "roboto");
        result(&mut host, serde_json::json!({ "kind": "fonts.addFallback", "fontKey": "roboto" }));

        let resolved = result(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.resolve", "runs": [
                { "text": "Hi", "fontFamily": "helvetica-bold" },
                { "text": "Hi \u{10FFFD}", "fontFamily": "roboto" },
                { "text": "Hi", "family": "Roboto", "weight": 400, "italic": false },
            ] }),
        );
        let runs = resolved["runs"].as_array().expect("runs");
        assert_eq!(runs.len(), 3);

        let standard = &runs[0];
        assert_eq!(standard["source"], "standard");
        assert_eq!(standard["engineFont"], serde_json::Value::Null);
        assert_eq!(standard["baseFontName"], "Helvetica-Bold");
        assert_eq!(standard["objectNumber"], 0);
        assert_eq!(standard["fallbacks"], serde_json::json!(["roboto"]));

        for engine in &runs[1..] {
            assert_eq!(engine["source"], "engine", "{engine}");
            assert_eq!(engine["engineFont"], "roboto");
            assert_eq!(engine["familyName"], "Roboto");
            assert_eq!(engine["degraded"], false);
        }
        let keys: Vec<&String> = runs[0].as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 10, "{keys:?}");

        assert_eq!(resolved["missingCharacters"], serde_json::json!(["\u{10FFFD}"]));
        // The facts the runs were read against.
        assert_eq!(resolved["facts"]["engineFonts"][0]["key"], "roboto");
        assert_eq!(resolved["facts"]["fallbacks"], serde_json::json!(["roboto"]));
    });
}

#[test]
fn a_run_naming_a_document_font_asks_for_its_program() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, EMBEDDED);
        let listed = result(&mut host, serde_json::json!({ "kind": "document.fonts.list" }));
        let object_number = listed["facts"]["faces"][0]["programs"][0]["objectNumber"].clone();

        let resolved = result(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.resolve", "runs": [
                { "text": "Hi", "fontFamily": "Roboto-Regular" },
            ] }),
        );
        let run = &resolved["runs"][0];
        assert_eq!(run["source"], "document-program", "{run}");
        assert_eq!(run["objectNumber"], object_number);
        assert_eq!(run["familyName"], "Roboto");
    });
}

#[test]
fn malformed_runs_and_missing_annotations_are_refused() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, NO_FONTS);
        let refused = |host: &mut Host, request: serde_json::Value| {
            let (json, _) = call(host, request, vec![]);
            json["error"].clone()
        };
        let error = refused(&mut host, serde_json::json!({ "kind": "document.fonts.resolve" }));
        assert_eq!(error["code"], "WireFormat", "{error}");
        let error = refused(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.resolve", "runs": [{ "text": 1,
                "fontFamily": "helvetica" }] }),
        );
        assert_eq!(error["code"], "WireFormat", "{error}");

        let pages = result(&mut host, serde_json::json!({ "kind": "pages.list" }));
        let page = pages["pages"][0]["ref"]["objectNumber"]
            .as_u64()
            .or_else(|| pages["pages"][0]["objectNumber"].as_u64())
            .expect("a page object number");
        let error = refused(
            &mut host,
            serde_json::json!({ "kind": "document.fonts.resolve",
                "annotation": { "kind": "objectNumber",
                    "page": { "kind": "objectNumber", "objectNumber": page },
                    "objectNumber": 999_999 },
                "runs": [{ "text": "Hi", "fontFamily": "helvetica" }] }),
        );
        assert_eq!(error["code"], "NotFound", "{error}");
    });
}

#[test]
fn every_document_font_kind_wants_an_open_document() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        for request in [
            serde_json::json!({ "kind": "document.fonts.list", "docId": "nothing" }),
            serde_json::json!({ "kind": "document.fonts.resolve", "docId": "nothing",
                "runs": [] }),
            serde_json::json!({ "kind": "document.fonts.program", "docId": "nothing",
                "displayHash": "00", "part": "info" }),
        ] {
            let (json, _) = call(&mut host, request.clone(), vec![]);
            assert!(json.get("error").is_some(), "{request}: {json}");
        }
    });
}

// ── Writes: `missingCharacters` and `fontsChanged` (`finishMutation`) ────

/// The first page's object number.
fn first_page(host: &mut Host) -> u64 {
    let pages = result(host, serde_json::json!({ "kind": "pages.list" }));
    pages["pages"][0]["ref"]["objectNumber"]
        .as_u64()
        .or_else(|| pages["pages"][0]["objectNumber"].as_u64())
        .expect("a page object number")
}

fn free_text(text: &str, font: &str) -> serde_json::Value {
    serde_json::json!({ "subtype": "free-text", "intent": "free-text", "contents": text,
        "fontFamily": font, "fontSize": 12,
        "box": { "x": 20, "y": 20, "width": 200, "height": 60 } })
}

fn square() -> serde_json::Value {
    serde_json::json!({ "subtype": "square",
        "box": { "x": 10, "y": 10, "width": 90, "height": 90 } })
}

/// `annotations.create` of `draft` on the first page: the whole answer.
fn create(host: &mut Host, draft: serde_json::Value) -> serde_json::Value {
    let page = first_page(host);
    result(
        host,
        serde_json::json!({ "kind": "annotations.create", "docId": DOC_ID,
            "page": { "kind": "objectNumber", "objectNumber": page }, "draft": draft }),
    )
}

fn draws_the_last_resort(host: &mut Host) -> bool {
    let listed = result(host, serde_json::json!({ "kind": "document.fonts.list" }));
    listed["facts"]["faces"].as_array().expect("faces").iter().any(|face| {
        face["programs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|program| program["sha256"] == epdf_engine::features::fonts::LAST_RESORT_HASH)
    })
}

#[test]
fn a_write_answers_the_characters_it_drew_with_no_font() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, NO_FONTS);
        let answer = create(&mut host, free_text("Hi \u{10FFFD}", "helvetica"));
        assert_eq!(
            answer["result"]["meta"]["missingCharacters"],
            serde_json::json!(["\u{10FFFD}"]),
            "{answer}"
        );
        // The next write starts from none.
        let answer = create(&mut host, free_text("Hi", "helvetica"));
        assert!(answer["result"]["meta"].get("missingCharacters").is_none(), "{answer}");
        assert!(answer.get("fontsChanged").is_none(), "not tracked: {answer}");
    });
}

#[test]
fn a_write_before_any_font_kind_draws_the_last_resort_box() {
    on_a_fresh_thread(|| {
        // No font kind before the write: the host registered the last resort
        // when it was built, as `WorkerHost` builds its `FontRegistrar`.
        let mut host = Host::new();
        open(&mut host, NO_FONTS);
        create(&mut host, free_text("Hi \u{10FFFD}", "helvetica"));
        assert!(draws_the_last_resort(&mut host));
    });
}

#[test]
fn two_hosts_on_one_thread_share_the_last_resort() {
    on_a_fresh_thread(|| {
        let mut first = Host::new();
        register(&mut first, "roboto");
        // A second host's construction neither clobbers the first's fonts
        // nor its last resort.
        let second = Host::new();
        let listed = result(&mut first, serde_json::json!({ "kind": "fonts.list" }));
        assert_eq!(listed["engineFonts"][0]["key"], "roboto");
        drop(second);
        open(&mut first, NO_FONTS);
        create(&mut first, free_text("Hi \u{10FFFD}", "helvetica"));
        assert!(draws_the_last_resort(&mut first), "the second host took it with it");

        // And the other way about: the host with the registry goes, and the
        // one left still draws the box.
        let mut left = Host::new();
        drop(first);
        open(&mut left, NO_FONTS);
        create(&mut left, free_text("Hi \u{10FFFD}", "helvetica"));
        assert!(draws_the_last_resort(&mut left), "the first host's registry took it");
    });
}

#[test]
fn listing_the_fonts_starts_telling_of_every_change() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, NO_FONTS);
        register(&mut host, "roboto");
        // Not tracked yet: no write says.
        let answer = create(&mut host, free_text("Hi", "helvetica"));
        assert!(answer.get("fontsChanged").is_none(), "{answer}");

        result(&mut host, serde_json::json!({ "kind": "document.fonts.list" }));
        let answer = create(&mut host, square());
        assert!(answer.get("fontsChanged").is_none(), "a square writes no font: {answer}");
        let answer = create(&mut host, free_text("Hi", "roboto"));
        assert_eq!(answer["fontsChanged"], true, "Roboto joined the list: {answer}");
        let answer = create(&mut host, square());
        assert!(answer.get("fontsChanged").is_none(), "asked again, nothing moved: {answer}");
    });
}

#[test]
fn reporting_every_font_change_tracks_from_the_first_write() {
    for report in [true, false] {
        let answer = on_a_fresh_thread(move || {
            let mut host = Host::new().with_report_every_font_change(report);
            open(&mut host, NO_FONTS);
            register(&mut host, "roboto");
            create(&mut host, free_text("Hi", "roboto"))
        });
        assert_eq!(answer.get("fontsChanged").is_some(), report, "{report}: {answer}");
    }
}

#[test]
fn a_changes_items_each_say_what_they_drew_with_no_font() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        open(&mut host, NO_FONTS);
        let page =
            serde_json::json!({ "kind": "objectNumber", "objectNumber": first_page(&mut host) });
        let op = |data: serde_json::Value| serde_json::json!({ "type": "annotations.create", "page": page, "data": data });
        let answer = result(
            &mut host,
            serde_json::json!({ "kind": "document.apply", "docId": DOC_ID, "change": { "ops": [
                op(free_text("A \u{10FFFD}", "helvetica")),
                op(square()),
                op(free_text("B \u{10FFFC} \u{10FFFD}", "helvetica")),
            ] } }),
        );
        let items = answer["result"]["items"].as_array().expect("items");
        assert_eq!(items[0]["meta"]["missingCharacters"], serde_json::json!(["\u{10FFFD}"]));
        assert!(items[1]["meta"].get("missingCharacters").is_none(), "{}", items[1]);
        assert_eq!(
            items[2]["meta"]["missingCharacters"],
            serde_json::json!(["\u{10FFFC}", "\u{10FFFD}"])
        );
        // The change's meta: every op's, first seen first.
        assert_eq!(
            answer["result"]["meta"]["missingCharacters"],
            serde_json::json!(["\u{10FFFD}", "\u{10FFFC}"]),
            "{answer}"
        );
    });
}
