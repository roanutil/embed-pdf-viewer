//! `signatures.finalizeCandidate`, the session-less half of a durable
//! signing. Mirrors `packages/engine/main/test/signature-finalize-candidate.test.ts`
//! over `CandidateFinalizer.ts`.
//!
//! The Rust prepare parks its candidate in memory rather than on disk, so a
//! candidate FILE is rebuilt here from a completed signing: its sealed
//! bytes with the `/Contents` hole emptied back to zeros. Finalizing that
//! file with the same CMS has to reproduce the sealed bytes exactly, which
//! is a stronger check than the TypeScript's own (it compares hashes).

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{sig_call_bytes, sig_call_with_payload, sig_list, sig_open_fixture, sig_prepare};

const FIX: &str = "packages/engine/main/test/fixtures";
const FAKE_CMS: [u8; 5] = [0x30, 3, 2, 1, 1];

struct Sealed {
    bytes: Vec<u8>,
    byte_range: [u64; 4],
    contents_size: u64,
    field_object_number: u64,
    version: Value,
}

/// Prepare, complete and save a signing over the unsigned field.
fn seal() -> Sealed {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let listed = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    let field = listed["signatures"][0]["field"].clone();
    let field_object_number = field["objectNumber"].as_u64().expect("an object-number field");
    let (prepared, _) = sig_prepare(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc,
                "input": { "field": field, "certify": { "permission": 2 }, "contentsSize": 512 } }),
    );
    let input = json!({ "signingId": prepared["signingId"], "expectedVersion": prepared["expectedVersion"] });
    let completed = sig_call_with_payload(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc, "input": input }),
        FAKE_CMS.to_vec(),
    );
    assert_eq!(completed["result"]["status"], json!("completed"), "{completed}");
    let bytes = sig_call_bytes(
        &mut host,
        json!({ "kind": "document.saveBuffer", "docId": doc, "mode": "incremental" }),
    );
    let range: Vec<u64> = serde_json::from_value(prepared["byteRange"].clone()).unwrap();
    Sealed {
        bytes,
        byte_range: range.try_into().unwrap(),
        contents_size: prepared["contentsSize"].as_u64().unwrap(),
        field_object_number,
        version: completed["result"]["version"].clone(),
    }
}

/// The sealed bytes with the hole's hex digits zeroed: what a prepare wrote.
fn candidate(sealed: &Sealed) -> Vec<u8> {
    let mut bytes = sealed.bytes.clone();
    let [_, r1, r2, _] = sealed.byte_range;
    for b in &mut bytes[(r1 + 1) as usize..(r2 - 1) as usize] {
        *b = b'0';
    }
    bytes
}

fn write(name: &str, bytes: &[u8]) -> String {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("finalize-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

fn request(sealed: &Sealed, path: &str) -> Value {
    json!({ "kind": "signatures.finalizeCandidate", "jobId": 1, "path": path,
            "byteRange": sealed.byte_range, "contentsSize": sealed.contents_size,
            "fieldObjectNumber": sealed.field_object_number, "password": null })
}

fn finalize(request: &Value, cms: &[u8]) -> Value {
    let mut host = Host::new();
    let response = host.call_filled(&request.to_string(), vec![cms.to_vec()]);
    serde_json::from_str(&response.json).unwrap()
}

#[test]
fn a_rebuilt_candidate_finalizes_to_the_sealed_file() {
    let sealed = seal();
    let path = write("ok.pdf", &candidate(&sealed));
    let answer = finalize(&request(&sealed, &path), &FAKE_CMS);
    assert!(answer.get("error").is_none(), "{answer}");
    let finalized = &answer["result"];
    assert_eq!(finalized["signature"]["signed"], json!(true));
    assert_eq!(finalized["signature"]["coverage"], json!("whole-revision"));
    assert_eq!(finalized["signature"]["byteRange"], json!(sealed.byte_range));
    assert_eq!(finalized["signature"]["docMdp"], json!(2));
    assert_eq!(finalized["protection"]["certification"]["permission"], json!(2), "{finalized}");
    // The version IS the file, and the file is the bytes the in-session
    // completion sealed.
    assert_eq!(std::fs::read(&path).unwrap(), sealed.bytes);
    assert_eq!(finalized["version"], sealed.version);

    // Every object the sealed file has is at or below its last object
    // number: the one a layer over the file opens with.
    let mut reader = Host::new();
    let opened: Value = serde_json::from_str(
        &reader
            .call_filled(
                &json!({ "kind": "open.layerMemBase", "jobId": 2, "docId": "sealed",
                    "baseKey": "sealed", "layer": { "kind": "fresh" }, "password": null })
                .to_string(),
                vec![sealed.bytes.clone()],
            )
            .json,
    )
    .unwrap();
    let last = opened["result"]["lastObjectNumber"].as_u64().expect("a layer's last number");
    assert!(last > 0, "{opened}");
    assert_eq!(finalized["lastObjectNumber"], json!(last), "{finalized}");

    // Finalizing the same file with the same CMS again writes the same bytes.
    let again = finalize(&request(&sealed, &path), &FAKE_CMS);
    assert_eq!(again["result"]["version"], sealed.version, "{again}");
}

#[test]
fn a_finalize_refuses_what_the_typescript_refuses() {
    let sealed = seal();
    let rebuilt = candidate(&sealed);
    let [r0, r1, r2, r3] = sealed.byte_range;
    let path = write("refusals.pdf", &rebuilt);
    let refuse = |mut request: Value, cms: &[u8], code: &str, needle: &str| {
        request["path"] = json!(path);
        let answer = finalize(&request, cms);
        assert_eq!(answer["error"]["code"], json!(code), "{answer}");
        let message = answer["error"]["message"].as_str().unwrap_or("");
        assert!(message.contains(needle), "{needle:?} not in {message:?}");
    };
    let base = request(&sealed, &path);
    let with = |key: &str, v: Value| {
        let mut r = base.clone();
        r[key] = v;
        r
    };
    refuse(
        with("byteRange", json!([r0, r1, r2, r3 + 1])),
        &FAKE_CMS,
        "InvalidArg",
        "does not span",
    );
    refuse(
        with("contentsSize", json!(sealed.contents_size - 1)),
        &FAKE_CMS,
        "InvalidArg",
        "hex digits",
    );
    refuse(
        with("byteRange", json!([r0, r1 + 1, r2 + 1, r3 - 1])),
        &FAKE_CMS,
        "InvalidArg",
        "Contents hole",
    );
    let too_big = vec![0x30u8; sealed.contents_size as usize + 1];
    refuse(base.clone(), &too_big, "SignatureRefused", "does not fit");
    refuse(base.clone(), &[0x04, 1, 1], "SignatureRefused", "DER SEQUENCE");
    // Nothing above touched the file.
    assert_eq!(std::fs::read(&path).unwrap(), rebuilt);
    // A wrong field: the bytes carry the signature on another object.
    refuse(
        with("fieldObjectNumber", json!(sealed.field_object_number + 1)),
        &FAKE_CMS,
        "SignatureRefused",
        "lost the signature field",
    );

    // A tampered rebuild: a byte appended past what the prepare sealed.
    let mut tampered = rebuilt.clone();
    tampered.push(b'\n');
    let path = write("tampered.pdf", &tampered);
    let mut request = request(&sealed, &path);
    request["path"] = json!(path);
    let answer = finalize(&request, &FAKE_CMS);
    assert_eq!(answer["error"]["code"], json!("InvalidArg"), "{answer}");
}
