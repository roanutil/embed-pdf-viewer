//! The CMS reader accepts and refuses what asn1js 3.0.10 and pkijs 3.4.0
//! do, over the whole CMS: re-encodings of the `rsaCades` fixture, asn1js's
//! node and depth limits (its speculative OCTET STRING decode included), and
//! the readers that throw. Every expected line is what `parseCmsInternal`
//! and `verifyCmsSignature` gave under tsx (Node 24.20.0, TZ=UTC) for the
//! same bytes, built the same way.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use epdf_signature_core::der::{self, Node};
use epdf_signature_core::{Cryptography, asn1js, parse, read_timestamp_info, verify_cms_signature};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tooling/plugin-conformance/fixtures/signature/fixtures.json"
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    STANDARD.decode(json["cms"][name].as_str().unwrap()).unwrap()
}

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
}

/// A tree element, re-encoded with DER lengths (tags below 31).
fn encode(node: &Node) -> Vec<u8> {
    let id = (node.class << 6) | if node.constructed { 0x20 } else { 0 } | node.tag as u8;
    if node.constructed {
        der::tlv(id, &node.children.iter().flat_map(encode).collect::<Vec<_>>())
    } else {
        der::tlv(id, &node.content)
    }
}

fn node(bytes: &[u8]) -> Node {
    der::from_ber(bytes).unwrap()
}

fn base() -> Node {
    let bytes = fixture("rsaCades");
    let root = node(&bytes);
    assert_eq!(encode(&root), bytes, "the fixture is DER and re-encodes to itself");
    root
}

fn signed_data(root: &mut Node) -> &mut Node {
    &mut root.children[1].children[0]
}

fn signer_info(root: &mut Node) -> &mut Node {
    signed_data(root).children.last_mut().unwrap().children.first_mut().unwrap()
}

fn signed_attribute<'a>(root: &'a mut Node, oid: &str) -> &'a mut Node {
    let attrs = signer_info(root).children.iter_mut().find(|c| c.is_context(0)).unwrap();
    attrs.children.iter_mut().find(|a| a.children[0].oid().as_deref() == Some(oid)).unwrap()
}

/// An OCTET STRING's content as a constructed OCTET STRING of two pieces,
/// the first of up to 5 bytes.
fn split(content: &[u8]) -> Node {
    let (head, tail) = content.split_at(content.len().min(5));
    let mut pieces = der::octet_string(head);
    pieces.extend(der::octet_string(tail));
    node(&der::tlv(0x24, &pieces))
}

fn index_of_octets(parent: &Node) -> usize {
    parent.children.iter().position(|c| c.is_octet_string()).unwrap()
}

/// `rsaCades` with one unsigned attribute (OID 1.2.3.4) whose value is the
/// given bytes.
fn with_unsigned(value: &[u8]) -> Vec<u8> {
    let mut root = base();
    let attribute = der::sequence(&[der::oid("1.2.3.4"), der::set(&[value.to_vec()])]);
    signer_info(&mut root).children.push(node(&der::context(1, &[attribute])));
    encode(&root)
}

fn nulls(n: usize) -> Vec<u8> {
    der::sequence(&vec![der::null(); n])
}

fn nested(depth: usize) -> Vec<u8> {
    let mut out = der::null();
    for _ in 0..depth {
        out = der::sequence(&[out]);
    }
    out
}

/// The probe's line for one input: what `parseCmsInternal` gave, then
/// `verifyCmsSignature`.
fn outcome(bytes: &[u8]) -> String {
    let parse = match parse::parse_cms_internal(bytes) {
        Ok(i) => format!(
            "ok md={} sig={} ess={}",
            i.parsed.message_digest.len(),
            i.signer_info.signature.len(),
            i.ess_cert_hash.map_or("null".to_string(), |h| h.len().to_string())
        ),
        Err(e) => {
            let error = e.to_error();
            match e.reason() {
                Some(reason) => format!("{}({reason}): {}", error.name, error.message),
                None => format!("{}: {}", error.name, error.message),
            }
        }
    };
    let verify = match verify_cms_signature(bytes) {
        Cryptography::Valid => "valid",
        Cryptography::Invalid => "invalid",
        Cryptography::Unsupported => "unsupported",
    };
    format!("parse: {parse} | verify: {verify}")
}

const NOT_BER: &str = "parse: CmsError(malformed): CMS is not valid BER | verify: invalid";
const SIGNED_DATA_SCHEMA: &str = "parse: CmsError(malformed): CMS does not parse as SignedData: \
     Object's schema was not verified against input data for SignedData | verify: invalid";
const VALID: &str = "parse: ok md=32 sig=256 ess=32 | verify: valid";

fn check(rows: Vec<(&str, Vec<u8>, &str)>) {
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|(name, bytes, expected)| {
            let got = outcome(bytes);
            (got != *expected).then(|| {
                format!(
                    "{name}: expected {expected}\n{:>width$}got {got}",
                    "",
                    width = name.len() + 2
                )
            })
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} rows differ:\n{}",
        wrong.len(),
        rows.len(),
        wrong.join("\n")
    );
}

const MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
const SIGNING_CERTIFICATE_V2: &str = "1.2.840.113549.1.9.16.2.47";

#[test]
fn constructed_strings_read_as_pkijs_reads_them() {
    let signature = {
        let mut root = base();
        let si = signer_info(&mut root);
        let k = index_of_octets(si);
        si.children[k] = split(&si.children[k].content.clone());
        encode(&root)
    };
    let signature_nested = {
        let mut root = base();
        let si = signer_info(&mut root);
        let k = index_of_octets(si);
        let content = si.children[k].content.clone();
        let mut pieces = der::octet_string(&content[..5]);
        pieces.extend(der::tlv(0x24, &der::octet_string(&content[5..])));
        si.children[k] = node(&der::tlv(0x24, &pieces));
        encode(&root)
    };
    let signature_foreign = {
        let mut root = base();
        let si = signer_info(&mut root);
        let k = index_of_octets(si);
        let mut pieces = der::octet_string(&si.children[k].content[..5]);
        pieces.extend(der::integer(1));
        si.children[k] = node(&der::tlv(0x24, &pieces));
        encode(&root)
    };
    let message_digest = {
        let mut root = base();
        let set = &mut signed_attribute(&mut root, MESSAGE_DIGEST).children[1];
        set.children[0] = split(&set.children[0].content.clone());
        encode(&root)
    };
    let cert_hash = {
        let mut root = base();
        let ess = signed_attribute(&mut root, SIGNING_CERTIFICATE_V2);
        let id = &mut ess.children[1].children[0].children[0].children[0];
        let k = index_of_octets(id);
        id.children[k] = split(&id.children[k].content.clone());
        encode(&root)
    };
    let version = {
        let mut root = base();
        let sd = signed_data(&mut root);
        let version = encode(&sd.children[0]);
        sd.children[0] = node(&der::tlv(0x22, &version));
        encode(&root)
    };
    let extn_value = {
        let mut root = base();
        let sd = signed_data(&mut root);
        let certs = sd.children.iter_mut().find(|c| c.is_context(0)).unwrap();
        let tbs = &mut certs.children[0].children[0];
        let ext =
            &mut tbs.children.iter_mut().find(|c| c.is_context(3)).unwrap().children[0].children[0];
        let k = index_of_octets(ext);
        ext.children[k] = split(&ext.children[k].content.clone());
        encode(&root)
    };
    check(vec![
        ("rsaCades", fixture("rsaCades"), VALID),
        ("sigConstructed", signature, SIGNED_DATA_SCHEMA),
        ("sigConstructedNested", signature_nested, SIGNED_DATA_SCHEMA),
        ("sigConstructedForeign", signature_foreign, NOT_BER),
        (
            "messageDigestConstructed",
            message_digest,
            "parse: ok md=0 sig=256 ess=32 | verify: invalid",
        ),
        ("certHashConstructed", cert_hash, "parse: ok md=32 sig=256 ess=0 | verify: invalid"),
        ("versionConstructed", version, NOT_BER),
        ("extnValueConstructed", extn_value, SIGNED_DATA_SCHEMA),
    ]);
}

#[test]
fn asn1js_rules_apply_anywhere_in_the_cms() {
    check(vec![
        (
            "unsignedGenTimeBad",
            with_unsigned(&unhex("180161")),
            "parse: Error: Wrong input string for conversion | verify: invalid",
        ),
        (
            "unsignedBmpOdd",
            with_unsigned(&unhex("1e03004100")),
            "parse: RangeError: Offset is outside the bounds of the DataView | verify: invalid",
        ),
        (
            "unsignedUniversal6",
            with_unsigned(&unhex("1c06000000410000")),
            "parse: RangeError: byte length of Uint32Array should be a multiple of 4 | verify: invalid",
        ),
        ("unsignedPrimitiveSequenceNotBer", with_unsigned(&unhex("1001ff")), NOT_BER),
        ("unsignedPrimitiveSequenceBer", with_unsigned(&unhex("10020500")), VALID),
        ("unsignedConstructedUtf8Ber", with_unsigned(&unhex("2c040c024142")), VALID),
        ("unsignedEocWithContent", with_unsigned(&unhex("30040002ffff")), NOT_BER),
        ("unsignedBitStringUnused8", with_unsigned(&unhex("03020800")), NOT_BER),
        // A throw inside a speculative OCTET STRING decode is caught there.
        ("unsignedOctetsWithGenTimeBad", with_unsigned(&unhex("0403180161")), VALID),
    ]);
}

#[test]
fn the_cms_node_limit_counts_what_asn1js_counts() {
    // tsx: 9,817 NULLs read, 9,818 do not. The CMS's own elements, plus what
    // asn1js finds speculatively decoding its primitive OCTET STRINGs (the
    // signature's random bytes among them), make up the rest of 10,000.
    check(vec![
        ("unsignedNulls9816", with_unsigned(&nulls(9_816)), VALID),
        ("unsignedNulls9817", with_unsigned(&nulls(9_817)), VALID),
        ("unsignedNulls9818", with_unsigned(&nulls(9_818)), NOT_BER),
        ("unsignedNulls9819", with_unsigned(&nulls(9_819)), NOT_BER),
    ]);
}

#[test]
fn the_cms_depth_limit_is_asn1js_s() {
    // tsx: 92 SEQUENCEs around a NULL read, 93 do not (the NULL then sits
    // 101 levels below the ContentInfo).
    check(vec![
        ("unsignedNested92", with_unsigned(&nested(92)), VALID),
        ("unsignedNested93", with_unsigned(&nested(93)), NOT_BER),
    ]);
}

/// The 13.9 probe's TSTInfo: its imprint AlgorithmIdentifier carries
/// `nulls` NULLs as parameters, `10 + nulls` elements.
fn tst_info_with_null_params(nulls: usize) -> Vec<u8> {
    der::sequence(&[
        der::integer(1),
        der::oid("1.2.3"),
        der::sequence(&[
            der::sequence(&[
                der::oid(epdf_signature_core::oids::SHA256),
                der::sequence(&vec![der::null(); nulls]),
            ]),
            der::octet_string(&[7; 32]),
        ]),
        der::integer(5),
        der::tlv(0x18, b"20260115000000Z"),
    ])
}

#[test]
fn the_tst_info_node_limit_counts_the_speculative_imprint_element() {
    // tsx: 9,989 NULLs read (9,999 elements); from 9,990 (10,000 elements)
    // on, "TSTInfo is not valid BER": the imprint's `07 07 ...` decodes
    // speculatively as one more element.
    let rows = [
        (9_988, true),
        (9_989, true),
        (9_990, false),
        (9_991, false),
        (10_000, false),
        (10_001, false),
    ];
    for (n, reads) in rows {
        match read_timestamp_info(&tst_info_with_null_params(n)) {
            Ok(info) => {
                assert!(reads, "{n} NULLs: read here, refused by asn1js");
                assert_eq!(info.gen_time, 1_768_435_200_000.0);
            }
            Err(e) => {
                assert!(!reads, "{n} NULLs: refused here ({}), read by asn1js", e.message());
                assert_eq!(
                    (e.reason(), e.message()),
                    (Some("malformed"), "TSTInfo is not valid BER")
                );
            }
        }
    }
}

#[test]
fn the_walk_answers_as_asn1js_from_ber_does() {
    // `asn1js.fromBER` under tsx for each input.
    let throws = |name: &str, message: &str| {
        asn1js::Verdict::Throws(epdf_kernel::Error::named(name, message))
    };
    let rows = [
        ("1e03004100", throws("RangeError", "Offset is outside the bounds of the DataView")),
        (
            "1c06000000410000",
            throws("RangeError", "byte length of Uint32Array should be a multiple of 4"),
        ),
        ("1800", throws("Error", "Wrong input string for conversion")),
        ("180161", throws("Error", "Wrong input string for conversion")),
        ("170161", asn1js::Verdict::Accepted),
        ("04051e03004100", asn1js::Verdict::Accepted),
    ];
    for (hex, expected) in rows {
        assert_eq!(asn1js::from_ber(&unhex(hex)), expected, "{hex}");
    }
}

/// `skiSid` with each carried certificate's subject key identifier replaced:
/// `make` gets the identifier's bytes and returns the extension's new
/// `extnValue` content.
fn with_ski(make: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let mut root = node(&fixture("skiSid"));
    let sd = signed_data(&mut root);
    let certs = sd.children.iter_mut().find(|c| c.is_context(0)).unwrap();
    for cert in &mut certs.children {
        let Some(exts) = cert.children[0].children.iter_mut().find(|c| c.is_context(3)) else {
            continue;
        };
        for ext in &mut exts.children[0].children {
            if ext.children[0].oid().as_deref() != Some("2.5.29.14") {
                continue;
            }
            let value = ext.children.last_mut().unwrap();
            value.content = make(&node(&value.content).content);
        }
    }
    encode(&root)
}

#[test]
fn a_subject_key_identifier_is_read_as_value_hex_view() {
    // tsx `parseCmsInternal`: findSignerCertificate compares
    // `fromBER(extnValue).result.valueBlock.valueHexView`, whatever the type.
    let parse_only = |bytes: &[u8]| outcome(bytes).split(" | ").next().unwrap().to_string();
    let not_carried =
        "parse: CmsError(malformed): the signer certificate is not carried by the CMS";
    let rows = [
        ("skiSid", fixture("skiSid"), "parse: ok md=32 sig=256 ess=32"),
        ("skiInnerInteger", with_ski(|s| der::tlv(0x02, s)), "parse: ok md=32 sig=256 ess=32"),
        ("skiInnerConstructed", with_ski(|s| der::tlv(0x24, &der::octet_string(s))), not_carried),
        ("skiInnerSequence", with_ski(|s| der::sequence(&[der::octet_string(s)])), not_carried),
        (
            "skiInnerGenTimeBad",
            with_ski(|_| unhex("180161")),
            "parse: Error: Wrong input string for conversion",
        ),
        ("skiInnerNotBer", with_ski(|_| unhex("040501")), not_carried),
    ];
    for (name, bytes, expected) in rows {
        assert_eq!(parse_only(&bytes), expected, "{name}");
    }
}
