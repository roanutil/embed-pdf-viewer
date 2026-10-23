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
    block_on(build_timestamp_token(
        &LeafCrypto,
        BuildTimestampTokenInput {
            digest: DIGEST.to_vec(),
            hash: Digest::Sha256,
            authority: leaf_signer(),
            gen_time: Some(GEN_TIME),
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
