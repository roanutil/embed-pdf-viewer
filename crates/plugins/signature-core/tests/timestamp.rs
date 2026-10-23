//! Document timestamps (`cms/timestamp.ts`, the `timestamp` completion
//! gate, `sign(doc, { kind: 'timestamp', key })`). No conformance vector
//! signs a timestamp, so these pin the ported behavior against the
//! TypeScript source directly.

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::{DocumentHandle, DocumentSecurity, EngineCall};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::store::Unsubscribe;
use epdf_signature_core::cms::{RawAlgorithm, RawSigner};
use epdf_signature_core::gate::{CompletionProfile, Gate, Prepared, verify_for_completion};
use epdf_signature_core::parse::{CmsContent, parse_cms_content};
use epdf_signature_core::signers::{
    CryptoKey, CryptoKeyPair, WebCrypto, certificate_common_name, create_test_timestamp_authority,
};
use epdf_signature_core::timestamp::{
    BuildTimestampTokenInput, build_timestamp_token, read_timestamp_info,
};
use epdf_signature_core::{CmsSigner, Digest, Profile, SignerPort, der, profile_for, sign};
use rsa::pkcs8::DecodePrivateKey as _;
use rsa::traits::PublicKeyParts as _;
use rsa::{Pkcs1v15Sign, RsaPrivateKey};
use serde_json::{Map, Value, json};

fn fixture(section: &str, name: &str, part: Option<&str>) -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tooling/plugin-conformance/fixtures/signature/fixtures.json"
    );
    let json: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let value = match part {
        Some(part) => &json[section][name][part],
        None => &json[section][name],
    };
    STANDARD.decode(value.as_str().unwrap()).unwrap()
}

fn leaf_key() -> RsaPrivateKey {
    RsaPrivateKey::from_pkcs8_der(&fixture("keys", "leaf", Some("pkcs8"))).unwrap()
}

fn pkcs1_sha256(data: &[u8]) -> Vec<u8> {
    leaf_key().sign(Pkcs1v15Sign::new::<sha2::Sha256>(), &Digest::Sha256.hash(data)).unwrap()
}

/// The fixture leaf certificate and key as a raw signer.
fn leaf_signer() -> RawSigner {
    RawSigner {
        certificate_chain: vec![fixture("certs", "leaf", None)],
        algorithm: RawAlgorithm::RsaPkcs1,
        hash: Digest::Sha256,
        sign: Rc::new(|data: Vec<u8>| Promise::resolved(pkcs1_sha256(&data))),
    }
}

/// A WebCrypto whose every key is the fixture leaf key.
struct LeafCrypto;

impl WebCrypto for LeafCrypto {
    fn generate_key(
        &self,
        params: Value,
        extractable: bool,
        _: Vec<String>,
    ) -> Promise<CryptoKeyPair> {
        let key = |key_type: &str, usage: &str| CryptoKey {
            key_type: key_type.into(),
            extractable,
            algorithm: json!({ "name": params["name"], "modulusLength": 2048, "hash": { "name": "SHA-256" } }),
            usages: vec![usage.into()],
            handle: Rc::new(()),
        };
        Promise::resolved(CryptoKeyPair {
            private_key: key("private", "sign"),
            public_key: key("public", "verify"),
        })
    }

    fn sign(&self, _: Value, _: &CryptoKey, data: Vec<u8>) -> Promise<Vec<u8>> {
        Promise::resolved(pkcs1_sha256(&data))
    }

    fn export_jwk(&self, _: &CryptoKey) -> Promise<Value> {
        let key = leaf_key();
        let b64 = |b: Vec<u8>| URL_SAFE_NO_PAD.encode(b);
        Promise::resolved(
            json!({ "kty": "RSA", "n": b64(key.n().to_bytes_be()), "e": b64(key.e().to_bytes_be()) }),
        )
    }

    fn get_random_values(&self, buffer: &mut [u8]) {
        buffer.fill(0xa5);
    }
}

fn block_on<T: 'static>(future: impl std::future::Future<Output = T> + 'static) -> T {
    let out: Rc<RefCell<Option<T>>> = Rc::default();
    let slot = out.clone();
    rt::spawn(async move {
        *slot.borrow_mut() = Some(future.await);
    });
    rt::run_until_stalled();
    out.borrow_mut().take().expect("the future settled")
}

const DIGEST: [u8; 32] = [7; 32];
/// 2026-01-15T00:00:00.250Z.
const GEN_TIME: f64 = 1_768_435_200_250.0;

fn token() -> Vec<u8> {
    token_at(GEN_TIME)
}

fn token_at(gen_time: f64) -> Vec<u8> {
    block_on(build_timestamp_token(
        &LeafCrypto,
        BuildTimestampTokenInput {
            digest: DIGEST.to_vec(),
            hash: Digest::Sha256,
            authority: leaf_signer(),
            gen_time: Some(gen_time),
        },
    ))
    .unwrap()
}

fn prepared(digest: &[u8], algorithm: &str) -> Prepared {
    Prepared { digest: digest.to_vec(), algorithm: algorithm.into(), contents_size: 8192.0 }
}

fn refusal(gate: Gate) -> (&'static str, String) {
    match gate {
        Gate::Refused { reason, detail } => (reason, detail),
        Gate::Ok => panic!("the gate let the token through"),
    }
}

/// `bytes` with the first occurrence of `find` replaced by `with`.
fn patched(bytes: &[u8], find: &[u8], with: &[u8]) -> Vec<u8> {
    let at = bytes.windows(find.len()).position(|w| w == find).expect("the bytes to patch");
    let mut out = bytes.to_vec();
    out[at..at + find.len()].copy_from_slice(with);
    out
}

#[test]
fn a_built_token_carries_its_tst_info_and_passes_the_timestamp_gate() {
    let token = token();
    let internal = parse_cms_content(&token, CmsContent::TstInfo).unwrap();
    let info = read_timestamp_info(internal.e_content.as_deref().unwrap()).unwrap();
    assert_eq!(info.imprint_algorithm, Digest::Sha256);
    assert_eq!(info.imprint, DIGEST);
    assert_eq!(info.gen_time, GEN_TIME);
    assert!(internal.parsed.signing_certificate_v2);
    assert_eq!(internal.parsed.signer_certificate, fixture("certs", "leaf", None));
    // asn1js writes the milliseconds only when they are not zero.
    assert!(token.windows(19).any(|w| w == b"20260115000000.250Z"));
    // The serial is the drawn bytes with the sign bit cleared.
    assert!(
        token
            .windows(10)
            .any(|w| w == [0x02, 0x08, 0x25, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5])
    );
    let gate =
        verify_for_completion(&token, &prepared(&DIGEST, "sha256"), CompletionProfile::Timestamp);
    assert_eq!(gate, Gate::Ok);
}

#[test]
fn the_timestamp_gate_refuses_what_the_typescript_refuses() {
    let token = token();
    let timestamp = CompletionProfile::Timestamp;
    assert_eq!(
        refusal(verify_for_completion(&token, &prepared(&DIGEST, "sha384"), timestamp)),
        (
            "algorithm-mismatch",
            "the token stamps a sha256 digest, the document was prepared with sha384".into()
        )
    );
    assert_eq!(
        refusal(verify_for_completion(&token, &prepared(&[8; 32], "sha256"), timestamp)),
        ("digest-mismatch", "the token's message imprint is not the prepared digest".into())
    );
    let mut small = prepared(&DIGEST, "sha256");
    small.contents_size = 16.0;
    assert_eq!(refusal(verify_for_completion(&token, &small, timestamp)).0, "too-large");

    // A changed genTime no longer matches the signed message digest.
    let retimed = patched(&token, b"20260115000000.250Z", b"20270115000000.250Z");
    assert_eq!(
        refusal(verify_for_completion(&retimed, &prepared(&DIGEST, "sha256"), timestamp)),
        ("signature-invalid", "the token's message digest does not cover its TSTInfo".into())
    );
    // The signature is the token's last element.
    let mut forged = token.clone();
    *forged.last_mut().unwrap() ^= 1;
    assert_eq!(
        refusal(verify_for_completion(&forged, &prepared(&DIGEST, "sha256"), timestamp)),
        ("signature-invalid", "the token signature does not verify".into())
    );

    // A detached CMS is no token, and a token is no detached signature.
    let detached = fixture("cms", "rsaCades", None);
    assert_eq!(
        refusal(verify_for_completion(&detached, &prepared(&DIGEST, "sha256"), timestamp)),
        ("malformed", "a timestamp token must carry a TSTInfo".into())
    );
    assert_eq!(
        refusal(verify_for_completion(&token, &prepared(&DIGEST, "sha256"), Profile::CadesB)),
        ("malformed", "CMS carries encapsulated content; a PDF signature is detached".into())
    );
}

#[test]
fn a_tst_info_that_does_not_read_is_a_cms_error() {
    let error = |bytes: &[u8]| {
        let e = read_timestamp_info(bytes).unwrap_err();
        (e.reason().unwrap(), e.message().to_string())
    };
    assert_eq!(error(&[0x30]), ("malformed", "TSTInfo is not valid BER".into()));
    assert_eq!(
        error(&der::sequence(&[der::null()])),
        (
            "malformed",
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo"
                .into()
        )
    );
    let unknown = der::sequence(&[
        der::integer(1),
        der::oid("1.2.3"),
        der::sequence(&[der::sequence(&[der::oid("1.2.3.4")]), der::octet_string(&DIGEST)]),
        der::integer(5),
        der::tlv(0x18, b"20260115000000Z"),
    ]);
    assert_eq!(error(&unknown), ("unsupported", "message imprint algorithm 1.2.3.4".into()));
}

#[test]
fn profile_for_names_the_timestamp_profile_and_refuses_unknown_sub_filters() {
    assert_eq!(profile_for("ETSI.RFC3161").unwrap(), CompletionProfile::Timestamp);
    assert_eq!(profile_for("adbe.pkcs7.detached").unwrap().name(), "pkcs7");
    let error = profile_for("adbe.x509.rsa_sha1").unwrap_err();
    assert_eq!(error.name, "EngineError");
    assert_eq!(error.code(), Some("InvalidArg"));
    assert_eq!(error.message, "unknown subFilter 'adbe.x509.rsa_sha1'");
    assert_eq!(error.member("details"), Some(&json!({ "field": "subFilter" })));
}

/// A document whose `signatures` service answers from a script and
/// records each call.
struct ScriptedDoc {
    calls: RefCell<Vec<(String, Vec<Value>)>>,
}

impl DocumentHandle for ScriptedDoc {
    fn id(&self) -> String {
        "d".into()
    }
    fn capabilities(&self) -> Value {
        json!({})
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        unimplemented!("sign reads no security")
    }
    fn has(&self, _: &str) -> bool {
        true
    }
    fn call(&self, path: &str, _: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        self.calls.borrow_mut().push((path.into(), args));
        match path {
            "signatures.prepare" => Promise::resolved(json!({
                "signingId": "signing-1",
                "digest": Bytes(DIGEST.to_vec()).to_json(),
                "algorithm": "sha256",
                "contentsSize": 8192,
                "subFilter": "ETSI.RFC3161",
                "expectedVersion": { "baseSha256": "base", "editsVersion": 0 },
            })),
            "signatures.complete" => Promise::resolved(json!({ "status": "completed" })),
            "signatures.cancel" => Promise::resolved(json!({ "status": "aborted" })),
            other => Promise::rejected(Error::new(format!("unexpected {other}"))),
        }
    }
    fn page_index(&self, _: u32) -> Result<u32, Error> {
        Ok(0)
    }
    fn subscribe(&self, _: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        Unsubscribe::new(|| {})
    }
}

fn timestamp_input() -> Map<String, Value> {
    json!({ "field": { "kind": "fqn", "name": "Sig1" }, "kind": "timestamp" })
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn a_document_timestamp_prepares_without_a_sub_filter_and_installs_the_token() {
    let doc = Rc::new(ScriptedDoc { calls: RefCell::default() });
    let requests: Rc<RefCell<Vec<Value>>> = Rc::default();
    let seen = requests.clone();
    let authority = CmsSigner {
        sign: Rc::new(move |request| {
            seen.borrow_mut().push(request.to_json());
            Promise::spawn(build_timestamp_token(
                &LeafCrypto,
                BuildTimestampTokenInput {
                    digest: request.digest,
                    hash: Digest::Sha256,
                    authority: leaf_signer(),
                    gen_time: Some(GEN_TIME),
                },
            ))
        }),
    };
    let result =
        block_on(sign(doc.clone(), timestamp_input(), SignerPort::Cms(authority))).unwrap();
    assert_eq!(result, json!({ "status": "completed" }));
    assert_eq!(requests.borrow()[0]["subFilter"], "ETSI.RFC3161");
    let calls = doc.calls.borrow();
    let paths: Vec<&str> = calls.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["signatures.prepare", "signatures.complete"]);
    assert_eq!(
        calls[0].1[0],
        json!({ "field": { "kind": "fqn", "name": "Sig1" }, "kind": "timestamp", "digest": "sha256" })
    );
    assert_eq!(calls[1].1[0]["signingId"], "signing-1");
    assert_eq!(Bytes::from_json(&calls[1].1[0]["cms"]).unwrap().0, token());
}

#[test]
fn a_document_timestamp_with_a_raw_key_is_refused_before_prepare() {
    let doc = Rc::new(ScriptedDoc { calls: RefCell::default() });
    let error =
        block_on(sign(doc.clone(), timestamp_input(), SignerPort::Raw(leaf_signer()))).unwrap_err();
    assert_eq!(error.code(), Some("InvalidArg"));
    assert_eq!(
        error.message,
        "a document timestamp needs a timestamp authority: a CMS key that returns an RFC 3161 token"
    );
    assert_eq!(error.member("details"), Some(&json!({ "field": "key" })));
    assert!(doc.calls.borrow().is_empty());
}

#[test]
fn an_unknown_sub_filter_is_refused_before_prepare() {
    let doc = Rc::new(ScriptedDoc { calls: RefCell::default() });
    let mut input = timestamp_input();
    input.remove("kind");
    input.insert("subFilter".into(), json!("adbe.x509.rsa_sha1"));
    let error = block_on(sign(doc.clone(), input, SignerPort::Raw(leaf_signer()))).unwrap_err();
    assert_eq!(error.code(), Some("InvalidArg"));
    assert_eq!(error.message, "unknown subFilter 'adbe.x509.rsa_sha1'");
    assert!(doc.calls.borrow().is_empty());
}

#[test]
fn a_gate_refusal_is_a_signature_refused_engine_error_and_cancels() {
    let doc = Rc::new(ScriptedDoc { calls: RefCell::default() });
    // A detached CMS answered for a document timestamp.
    let wrong =
        CmsSigner { sign: Rc::new(|_| Promise::resolved(fixture("cms", "rsaCades", None))) };
    let error = block_on(sign(doc.clone(), timestamp_input(), SignerPort::Cms(wrong))).unwrap_err();
    assert_eq!(error.name, "EngineError");
    assert_eq!(error.code(), Some("SignatureRefused"));
    assert_eq!(
        error.message,
        "signing refused (malformed): a timestamp token must carry a TSTInfo"
    );
    assert_eq!(error.member("details"), Some(&json!({ "reason": "malformed" })));
    let calls = doc.calls.borrow();
    assert_eq!(calls.last().unwrap(), &("signatures.cancel".to_string(), vec![json!("signing-1")]));
}

#[test]
fn the_test_timestamp_authority_stamps_with_its_own_certificate() {
    let authority = block_on(create_test_timestamp_authority(Rc::new(LeafCrypto), None)).unwrap();
    assert_eq!(
        certificate_common_name(&authority.certificate).unwrap().as_deref(),
        Some("EmbedPDF test timestamp authority")
    );
    let request = epdf_signature_core::CmsSignRequest {
        digest: DIGEST.to_vec(),
        algorithm: "sha256".into(),
        sub_filter: "ETSI.RFC3161".into(),
    };
    let pending = (authority.signer.sign)(request);
    let token = block_on(async move { pending.await }).unwrap();
    let internal = parse_cms_content(&token, CmsContent::TstInfo).unwrap();
    assert_eq!(internal.parsed.signer_certificate, authority.certificate);
    assert_eq!(
        verify_for_completion(&token, &prepared(&DIGEST, "sha256"), CompletionProfile::Timestamp),
        Gate::Ok
    );
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn gen_time_writes_milliseconds_only_when_present_and_padded_to_three() {
    assert!(contains(&token_at(1_768_435_200_000.0), b"\x18\x0f20260115000000Z"));
    assert!(contains(&token_at(1_768_435_200_005.0), b"\x18\x1320260115000000.005Z"));
}

#[test]
fn the_tst_info_is_carried_as_a_constructed_octet_string() {
    // id-ct-TSTInfo, then `[0]` around a constructed OCTET STRING (0x24).
    let mut header = der::oid(epdf_signature_core::oids::TST_INFO);
    header.push(0xa0);
    let token = token();
    let at =
        token.windows(header.len()).position(|w| w == header.as_slice()).unwrap() + header.len();
    let length_octets = if token[at] & 0x80 == 0 { 1 } else { 1 + usize::from(token[at] & 0x7f) };
    assert_eq!(token[at + length_octets], 0x24);
}

/// The built token with its encapsulated content replaced by `e_content`.
fn token_with_e_content(e_content: Vec<u8>) -> Vec<u8> {
    let root = der::from_ber(&token()).unwrap();
    let signed = &root.children[1].children[0];
    let mut members: Vec<Vec<u8>> = signed.children.iter().map(|n| n.raw.clone()).collect();
    members[2] = der::sequence(&[
        der::oid(epdf_signature_core::oids::TST_INFO),
        der::context(0, &[e_content]),
    ]);
    der::sequence(&[root.children[0].raw.clone(), der::context(0, &[der::sequence(&members)])])
}

#[test]
fn a_constructed_e_content_joins_its_own_pieces_only() {
    // asn1js `OctetString.getValue()`: a nested constructed piece adds nothing.
    let nested = der::tlv(
        0x24,
        &[der::tlv(0x24, &der::octet_string(&[1, 2, 3])), der::octet_string(&[4, 5])].concat(),
    );
    let internal = parse_cms_content(&token_with_e_content(nested), CmsContent::TstInfo).unwrap();
    assert_eq!(internal.e_content, Some(vec![4, 5]));
}

#[test]
fn a_sub_filter_that_is_not_a_string_is_unknown() {
    let doc = Rc::new(ScriptedDoc { calls: RefCell::default() });
    let mut input = timestamp_input();
    input.remove("kind");
    input.insert("subFilter".into(), json!(["adbe.pkcs7.detached"]));
    let error = block_on(sign(doc.clone(), input, SignerPort::Raw(leaf_signer()))).unwrap_err();
    assert_eq!(error.code(), Some("InvalidArg"));
    assert_eq!(error.message, "unknown subFilter 'adbe.pkcs7.detached'");
    assert!(doc.calls.borrow().is_empty());
}

/// `Ok(genTime)`, or `Err(name, CmsError reason, message)`.
#[derive(Debug, PartialEq)]
enum Outcome {
    Ok(f64),
    Err(&'static str, Option<&'static str>, &'static str),
}

/// Every row: the TSTInfo bytes, and what `readTimestampInfo` answers for them
/// (pkijs 3.4.0 / asn1js 3.0.10 under tsx, `TZ=UTC`).
const READER_CASES: &[(&str, &str, Outcome)] = &[
    (
        "base",
        "304c02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "imprintConstructed",
        "305002010106022a033033300b06096086480165030402012424041007070707070707070707070707070707041007070707070707070707070707070707020105180f32303236303131353030303030305a",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "imprintNested",
        "305002010106022a033033300b06096086480165030402012424242204200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "timeComma",
        "305002010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105181332303236303131353030303030302c3235305a",
        Outcome::Err("Error", None, "Wrong input string for conversion"),
    ),
    (
        "timeHours",
        "304802010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180b323032363031313530305a",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "timeHoursFraction",
        "304a02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180d323032363031313530302e355a",
        Outcome::Ok(1768437000000.0),
    ),
    (
        "timeMinutes",
        "304a02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180d3230323630313135303030305a",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "timeMinutesFraction",
        "304d02010106022a03302f300b06096086480165030402010420070707070707070707070707070707070707070707070707070707070707070702010518103230323630313135303030302e32355a",
        Outcome::Ok(1768435215000.0),
    ),
    (
        "timeDate",
        "304602010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180932303236303131355a",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "timeDateFraction",
        "304802010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180b32303236303131352e355a",
        Outcome::Err("Error", None, "Wrong input string for conversion"),
    ),
    (
        "timeMillis",
        "305002010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105181332303236303131353030303030302e3235305a",
        Outcome::Ok(1768435200250.0),
    ),
    (
        "timeEmptyFraction",
        "304d02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105181032303236303131353030303030302e5a",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "timeExponent",
        "305002010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105181332303236303131353030303030302e3565325a",
        Outcome::Ok(1768435250000.0),
    ),
    (
        "timeTrailingSpace",
        "304f02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105181232303236303131353030303030302e31205a",
        Outcome::Ok(1768435200100.0),
    ),
    (
        "timeMinusInUtc",
        "305102010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105181432303236303131353030303030302e31652d325a",
        Outcome::Err("Error", None, "Wrong input string for conversion"),
    ),
    (
        "timeMonth13",
        "304c02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236313331353030303030305a",
        Outcome::Ok(1799971200000.0),
    ),
    (
        "timeLetter",
        "304c02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f323032364f3131353030303030305a",
        Outcome::Err("Error", None, "Wrong input string for conversion"),
    ),
    (
        "timeShort",
        "304d02010106022a03302f300b06096086480165030402010420070707070707070707070707070707070707070707070707070707070707070702010518103230323630313135303030303030305a",
        Outcome::Err("Error", None, "Wrong input string for conversion"),
    ),
    (
        "timeEmpty",
        "303d02010106022a03302f300b0609608648016503040201042007070707070707070707070707070707070707070707070707070707070707070201051800",
        Outcome::Err("Error", None, "Wrong input string for conversion"),
    ),
    (
        "timeYear0050",
        "304c02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f30303530303131353030303030305a",
        Outcome::Ok(-629942400000.0),
    ),
    (
        "timeConstructed",
        "304e02010106022a03302f300b0609608648016503040201042007070707070707070707070707070707070707070707070707070707070707070201053811180f32303236303131353030303030305a",
        Outcome::Err("CmsError", Some("malformed"), "TSTInfo is not valid BER"),
    ),
    (
        "timeLocal",
        "304b02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180e3230323630313135303030303030",
        Outcome::Ok(1767139200000.0),
    ),
    (
        "timeOffset",
        "305002010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105181332303236303131353030303030302b30313030",
        Outcome::Ok(1767142800000.0),
    ),
    (
        "trailingOctets",
        "304f02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a040101",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "accuracy",
        "305102010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a3003020101",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "accuracyEmpty",
        "304e02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a3000",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "accuracyNotAccuracy",
        "305202010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a300406022a03",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "orderingBeforeAccuracy",
        "305402010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a0101ff3003020101",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "twoNonces",
        "305202010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a02010902010a",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "nonceThenOrdering",
        "305202010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a0201090101ff",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "allOptional",
        "307c02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a3009020101800101810102010100020109a012a410300e310c300a06035504030c03545341a109300706022a03040105",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "tsaDirectory",
        "306002010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa012a410300e310c300a06035504030c03545341",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "tsaDns",
        "305b02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa00d820b7473612e6578616d706c65",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "tsaNotGeneralName",
        "305102010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa003020103",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "tsaEmpty",
        "304e02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa000",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "tsaRegisteredIdBad",
        "305102010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa003880181",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Error during parsing of ASN.1 data. Data is not correct for 'GeneralName registeredID'.",
        ),
    ),
    (
        "tsaRegisteredId",
        "305202010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa00488022a03",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "extensions",
        "305702010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa109300706022a03040105",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "extensionsEmpty",
        "304e02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa100",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "extensionsNotExtension",
        "305102010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa103020103",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "extensionsThenTrailing",
        "305a02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa109300706022a03040105040101",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "badTimeInsideExtensions",
        "305102010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa103180178",
        Outcome::Err("Error", None, "Wrong input string for conversion"),
    ),
    (
        "extraAfterAllSlots",
        "307902010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a3003020101010100020109a012a410300e310c300a06035504030c03545341a109300706022a03040105040101",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "rid0",
        "305002010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa0028800",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "ridPadded",
        "305202010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa00488028001",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "ridBig",
        "305b02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa00d880b2affffffffffffffffff7f",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "rfc822Binary",
        "305202010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa0048102ff00",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "uriEmpty",
        "305002010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa0028600",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "ipBinary",
        "305402010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa006870401020304",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "otherNameBadTime",
        "305902010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa00ba00906022a03a003180178",
        Outcome::Err("Error", None, "Wrong input string for conversion"),
    ),
    (
        "otherName",
        "305902010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa00ba00906022a03a003020101",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "utcConstructedTrailing",
        "304e02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a3700",
        Outcome::Err("CmsError", Some("malformed"), "TSTInfo is not valid BER"),
    ),
    (
        "utcBadTrailing",
        "304f02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a170178",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "gtConstructedInExt",
        "305002010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa1023800",
        Outcome::Err("CmsError", Some("malformed"), "TSTInfo is not valid BER"),
    ),
    (
        "x400",
        "305202010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa004a3023000",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "ediParty",
        "305502010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa007a505a1030c0170",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "ediPartyBad",
        "305502010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305aa007a505a103020101",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "versionConstructed",
        "304e220302010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a",
        Outcome::Err("CmsError", Some("malformed"), "TSTInfo is not valid BER"),
    ),
    (
        "octetConstructedTrailing",
        "304e02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a2400",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "imprintAlgExtra",
        "305102010106022a03303430100609608648016503040201050002010104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "imprintAlgNoOid",
        "304402010106022a033027300302010104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "imprintExtra",
        "304f02010106022a033032300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020101020105180f32303236303131353030303030305a",
        Outcome::Ok(1768435200000.0),
    ),
    (
        "imprintShort",
        "302a02010106022a03300d300b0609608648016503040201020105180f32303236303131353030303030305a",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "fourMembers",
        "303b02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "notSequence",
        "314c02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
    (
        "longTag",
        "304f02010106022a03302f300b060960864801650304020104200707070707070707070707070707070707070707070707070707070707070707020105180f32303236303131353030303030305a9f1f00",
        Outcome::Err(
            "CmsError",
            Some("malformed"),
            "TSTInfo does not parse: Object's schema was not verified against input data for TSTInfo",
        ),
    ),
];

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn the_tst_info_reader_accepts_and_refuses_what_pkijs_does() {
    let mut wrong = Vec::new();
    for (name, hex, expected) in READER_CASES {
        let got = match read_timestamp_info(&unhex(hex)) {
            Ok(info) => {
                assert_eq!(info.imprint_algorithm, Digest::Sha256, "{name}");
                assert_eq!(info.imprint.len(), 32, "{name}");
                format!("{:?}", Outcome::Ok(info.gen_time))
            }
            Err(e) => {
                let error = e.to_error();
                format!("Err({:?}, {:?}, {:?})", error.name, e.reason(), error.message)
            }
        };
        if got != format!("{expected:?}") {
            wrong.push(format!("{name}: expected {expected:?}, got {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} rows differ:\n{}",
        wrong.len(),
        READER_CASES.len(),
        wrong.join("\n")
    );
}
