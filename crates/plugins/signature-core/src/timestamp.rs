//! `cms/timestamp.ts`: an RFC 3161 timestamp token's TSTInfo, read and
//! built.

use epdf_kernel::Error;

use crate::asn1js;
use crate::cert::{AlgId, Certificate};
use crate::cms::{RawSigner, attribute, cms_signature_value, signature_algorithm_for};
use crate::der::{self, Node};
use crate::oids::{self, Digest};
use crate::parse::ParseError;
use crate::signers::WebCrypto;

/// `TimestampInfo`: which digest a token's TSTInfo stamped, and when.
#[derive(Clone, Debug, PartialEq)]
pub struct TimestampInfo {
    pub imprint_algorithm: Digest,
    /// The stamped digest: for a document timestamp, the `/ByteRange` digest.
    pub imprint: Vec<u8>,
    /// `genTime`, in epoch milliseconds.
    pub gen_time: f64,
}

// ── reading: asn1js's parse and pkijs's `TSTInfo` schema ─────────────────────

/// A schema element as asn1js's `compareSchema` matches it. Classes are
/// `der`'s (0 universal, 1 application, 2 context-specific).
enum Slot {
    /// A primitive element of this class and tag.
    Prim(u8, u32),
    /// A constructed element whose members match in order.
    Cons(u8, u32, Vec<(Slot, bool)>),
    /// A constructed element whose every member matches the one schema
    /// (`new Constructed({ value: [new Repeated({ value })] })`).
    Repeat(u8, u32, Box<Slot>),
    Choice(Vec<Slot>),
    Any,
}

const UNIVERSAL: u8 = 0;
const APPLICATION: u8 = 1;
const CONTEXT: u8 = 2;

fn required(slot: Slot) -> (Slot, bool) {
    (slot, false)
}

fn optional(slot: Slot) -> (Slot, bool) {
    (slot, true)
}

fn sequence(members: Vec<(Slot, bool)>) -> Slot {
    Slot::Cons(UNIVERSAL, der::SEQUENCE, members)
}

fn universal(tag: u32) -> Slot {
    Slot::Prim(UNIVERSAL, tag)
}

fn strings(tags: &[u32]) -> Slot {
    Slot::Choice(tags.iter().map(|&t| universal(t)).collect())
}

/// pkijs's `GeneralName.schema()`.
fn general_name() -> Slot {
    const NUMERIC: u32 = 18;
    const PRINTABLE: u32 = 19;
    let numeric_or_printable = || strings(&[NUMERIC, PRINTABLE]);
    let directory_string = || strings(&[20, PRINTABLE, 28, der::UTF8_STRING, 30]);
    let ctx_prim = |n| Slot::Prim(CONTEXT, n);
    let ctx = |n, members| Slot::Cons(CONTEXT, n, members);
    let built_in_standard_attributes = sequence(vec![
        optional(Slot::Cons(APPLICATION, 1, vec![required(numeric_or_printable())])),
        optional(Slot::Cons(APPLICATION, 2, vec![required(numeric_or_printable())])),
        optional(ctx_prim(0)),
        optional(ctx_prim(1)),
        optional(ctx(2, vec![required(numeric_or_printable())])),
        optional(ctx_prim(3)),
        optional(ctx_prim(4)),
        optional(ctx(
            5,
            vec![
                required(ctx_prim(0)),
                optional(ctx_prim(1)),
                optional(ctx_prim(2)),
                optional(ctx_prim(3)),
            ],
        )),
        optional(Slot::Repeat(CONTEXT, 6, Box::new(universal(PRINTABLE)))),
    ]);
    let built_in_domain_defined_attributes =
        sequence(vec![required(universal(PRINTABLE)), required(universal(PRINTABLE))]);
    let extension_attributes = Slot::Cons(
        UNIVERSAL,
        der::SET,
        vec![optional(ctx_prim(0)), optional(ctx(1, vec![required(Slot::Any)]))],
    );
    let attribute_type_and_value =
        sequence(vec![required(universal(der::OID)), required(Slot::Any)]);
    let relative_distinguished_names = Slot::Repeat(
        UNIVERSAL,
        der::SEQUENCE,
        Box::new(Slot::Repeat(UNIVERSAL, der::SET, Box::new(attribute_type_and_value))),
    );
    Slot::Choice(vec![
        ctx(0, vec![required(universal(der::OID)), required(ctx(0, vec![required(Slot::Any)]))]),
        ctx_prim(1),
        ctx_prim(2),
        ctx(
            3,
            vec![
                required(built_in_standard_attributes),
                optional(built_in_domain_defined_attributes),
                optional(extension_attributes),
            ],
        ),
        ctx(4, vec![required(relative_distinguished_names)]),
        ctx(
            5,
            vec![
                optional(ctx(0, vec![required(directory_string())])),
                required(ctx(1, vec![required(directory_string())])),
            ],
        ),
        ctx_prim(6),
        ctx_prim(7),
        ctx_prim(8),
    ])
}

/// pkijs's `TSTInfo.schema()`, members in order.
fn tst_info_members() -> Vec<(Slot, bool)> {
    let algorithm_identifier = sequence(vec![required(universal(der::OID)), optional(Slot::Any)]);
    let extension = sequence(vec![
        required(universal(der::OID)),
        optional(universal(1)),
        required(universal(der::OCTET_STRING)),
    ]);
    vec![
        required(universal(der::INTEGER)),
        required(universal(der::OID)),
        required(sequence(vec![
            required(algorithm_identifier),
            required(universal(der::OCTET_STRING)),
        ])),
        required(universal(der::INTEGER)),
        required(universal(der::GENERALIZED_TIME)),
        // Accuracy.
        optional(sequence(vec![
            optional(universal(der::INTEGER)),
            optional(Slot::Prim(CONTEXT, 0)),
            optional(Slot::Prim(CONTEXT, 1)),
        ])),
        // ordering, nonce.
        optional(universal(1)),
        optional(universal(der::INTEGER)),
        // tsa: `[0]` around a GeneralName.
        optional(Slot::Cons(CONTEXT, 0, vec![required(general_name())])),
        // extensions.
        optional(Slot::Repeat(CONTEXT, 1, Box::new(extension))),
    ]
}

/// The identifier checks of `compareSchema`. asn1js reads a tag number of
/// 31 or more as hex-only, which no schema here names.
fn same_id(node: &Node, class: u8, tag: u32, constructed: bool) -> bool {
    node.class == class && node.tag == tag && node.constructed == constructed && tag < 31
}

fn matches(slot: &Slot, node: &Node) -> bool {
    match slot {
        Slot::Any => true,
        Slot::Choice(alternatives) => alternatives.iter().any(|a| matches(a, node)),
        Slot::Prim(class, tag) => same_id(node, *class, *tag, false),
        Slot::Cons(class, tag, members) => {
            same_id(node, *class, *tag, true) && match_members(members, &node.children).is_some()
        }
        Slot::Repeat(class, tag, member) => {
            same_id(node, *class, *tag, true) && node.children.iter().all(|c| matches(member, c))
        }
    }
}

/// `compareSchema` over a constructed element's members: which input
/// member each slot took, or `None` where asn1js refuses. A slot that fails
/// on an optional member moves on with the same input member; the LAST
/// comparison made must have matched, so a trailing member no optional slot
/// can place refuses the whole element. Input members past the last slot
/// are never looked at.
fn match_members(slots: &[(Slot, bool)], input: &[Node]) -> Option<Vec<Option<usize>>> {
    if slots.is_empty() {
        return Some(Vec::new());
    }
    if input.is_empty() {
        return slots.iter().all(|(_, optional)| *optional).then(|| vec![None; slots.len()]);
    }
    let mut taken = vec![None; slots.len()];
    let mut skipped = 0;
    let mut last = false;
    for (i, (slot, optional)) in slots.iter().enumerate() {
        let at = i - skipped;
        let Some(node) = input.get(at) else {
            if !optional {
                return None;
            }
            continue;
        };
        last = matches(slot, node);
        if last {
            taken[i] = Some(at);
        } else if *optional {
            skipped += 1;
        } else {
            return None;
        }
    }
    last.then_some(taken)
}

fn tst_info_error(detail: &str) -> ParseError {
    ParseError::Cms { reason: "malformed", message: format!("TSTInfo does not parse: {detail}") }
}

/// `readTimestampInfo`: a token's TSTInfo (the encapsulated content of an
/// RFC 3161 token), read as `new pkijs.TSTInfo({ schema })` reads it. Fails
/// with `CmsError` as the TypeScript throws it, or with what asn1js throws
/// reading the bytes (a GeneralizedTime its `fromString` cannot read, a
/// BMPString of odd length, ...: [`asn1js::from_ber`]).
///
/// A GeneralizedTime without `Z` is local time to asn1js; this reads it as
/// asn1js does on a host whose time zone is UTC.
pub fn read_timestamp_info(e_content: &[u8]) -> Result<TimestampInfo, ParseError> {
    let not_ber =
        || ParseError::Cms { reason: "malformed", message: "TSTInfo is not valid BER".into() };
    match asn1js::from_ber(e_content) {
        asn1js::Verdict::Accepted => {}
        asn1js::Verdict::Refused => return Err(not_ber()),
        asn1js::Verdict::Throws(error) => return Err(ParseError::Other(error)),
    }
    let node = der::from_ber(e_content).ok_or_else(not_ber)?;
    let schema_error = || tst_info_error(&der::schema_error("TSTInfo").message);
    if !same_id(&node, UNIVERSAL, der::SEQUENCE, true) {
        return Err(schema_error());
    }
    let taken = match_members(&tst_info_members(), &node.children).ok_or_else(schema_error)?;
    let member = |slot: usize| taken[slot].map(|at| &node.children[at]);
    // `new GeneralName({ schema })` re-reads a registeredID as an OBJECT
    // IDENTIFIER, and asn1js refuses one whose last byte continues.
    if let Some(tsa) = member(8).and_then(|tsa| tsa.children.first())
        && tsa.is_context(8)
        && tsa.content.last().is_some_and(|b| b & 0x80 != 0)
    {
        return Err(tst_info_error(
            "Error during parsing of ASN.1 data. Data is not correct for 'GeneralName registeredID'.",
        ));
    }
    let (Some(imprint), Some(gen_time)) = (member(2), member(4)) else {
        return Err(schema_error());
    };
    let algorithm = AlgId::read(&imprint.children[0]).ok_or_else(schema_error)?;
    let gen_time = der::asn1js_generalized_time(&gen_time.content).ok_or_else(schema_error)?;
    let imprint_algorithm = Digest::from_oid(&algorithm.oid).ok_or_else(|| ParseError::Cms {
        reason: "unsupported",
        message: format!("message imprint algorithm {}", algorithm.oid),
    })?;
    Ok(TimestampInfo { imprint_algorithm, imprint: imprint.children[1].content.clone(), gen_time })
}

/// `BuildTimestampTokenInput`.
#[derive(Clone)]
pub struct BuildTimestampTokenInput {
    /// The digest to stamp (for a document timestamp, the prepared
    /// `/ByteRange` digest).
    pub digest: Vec<u8>,
    pub hash: Digest,
    /// The authority's key and certificate.
    pub authority: RawSigner,
    /// Epoch milliseconds; now when absent.
    pub gen_time: Option<f64>,
}

/// `new asn1js.GeneralizedTime({ valueDate })`: the milliseconds are written
/// (three digits) only when they are not zero.
fn generalized_time_with_ms(ms: f64) -> Vec<u8> {
    let (y, mo, d, h, mi, s) = der::civil(ms);
    let millis = ms.rem_euclid(1000.0).floor() as i64;
    let fraction = if millis == 0 { String::new() } else { format!(".{millis:03}") };
    der::tlv(0x18, format!("{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}{fraction}Z").as_bytes())
}

/// `new EncapsulatedContentInfo({ eContent })` re-wraps a primitive OCTET
/// STRING as a constructed one of 65,536-byte pieces.
fn constructed_octets(bytes: &[u8]) -> Vec<u8> {
    let pieces: Vec<Vec<u8>> = bytes.chunks(65_536).map(der::octet_string).collect();
    der::tlv(0x24, &pieces.concat())
}

/// `buildTimestampToken`: an RFC 3161 timestamp token, a SignedData whose
/// content is a TSTInfo stamping `digest`, signed by the authority over
/// content-type (id-ct-TSTInfo), message-digest (of the TSTInfo) and ESS
/// signing-certificate-v2. What a timestamp authority returns; used by the
/// test authority, never to stamp a real document. `crypto` draws the
/// serial number, as `globalThis.crypto.getRandomValues` does.
pub async fn build_timestamp_token(
    crypto: &dyn WebCrypto,
    input: BuildTimestampTokenInput,
) -> Result<Vec<u8>, Error> {
    if input.digest.len() != input.hash.size() {
        return Err(Error::new(format!(
            "digest is {} bytes; {} needs {}",
            input.digest.len(),
            input.hash.name(),
            input.hash.size()
        )));
    }
    let Some(authority_der) = input.authority.certificate_chain.first() else {
        // `certificateChain[0]!` is undefined; pkijs throws reading it.
        return Err(Error::named(
            "TypeError",
            "Cannot read properties of undefined (reading 'byteLength')",
        ));
    };
    let authority_cert = Certificate::from_ber(authority_der)?;
    let mut serial = [0u8; 8];
    crypto.get_random_values(&mut serial);
    serial[0] &= 0x7f;
    let gen_time = input.gen_time.unwrap_or_else(epdf_kernel::rt::now);
    let e_content = der::sequence(&[
        der::integer(1),
        der::oid("1.3.6.1.4.1.57264.1.1"),
        der::sequence(&[
            der::sequence(&[der::oid(input.hash.oid())]),
            der::octet_string(&input.digest),
        ]),
        der::integer_bytes(&serial),
        generalized_time_with_ms(gen_time),
    ]);
    let content_digest = input.authority.hash.hash(&e_content);
    let cert_hash = Digest::Sha256.hash(authority_der);
    let issuer_serial = der::sequence(&[
        der::sequence(&[der::context(4, std::slice::from_ref(&authority_cert.issuer.raw))]),
        der::integer_bytes(&authority_cert.serial),
    ]);
    let attributes = vec![
        attribute(oids::CONTENT_TYPE, der::oid(oids::TST_INFO)),
        attribute(oids::MESSAGE_DIGEST, der::octet_string(&content_digest)),
        attribute(
            oids::SIGNING_CERTIFICATE_V2,
            der::sequence(&[der::sequence(&[der::sequence(&[
                der::octet_string(&cert_hash),
                issuer_serial,
            ])])]),
        ),
    ];
    let digest_algorithm = der::sequence(&[der::oid(input.authority.hash.oid())]);
    let raw = (input.authority.sign)(der::set(&attributes)).await?;
    let signature = cms_signature_value(input.authority.algorithm, raw);
    let signer_info = der::sequence(&[
        der::integer(1),
        der::sequence(&[
            authority_cert.issuer.raw.clone(),
            der::integer_bytes(&authority_cert.serial),
        ]),
        digest_algorithm.clone(),
        der::context(0, &attributes),
        signature_algorithm_for(input.authority.algorithm, input.authority.hash),
        der::octet_string(&signature),
    ]);
    let signed_data = der::sequence(&[
        der::integer(3),
        der::set(&[digest_algorithm]),
        der::sequence(&[
            der::oid(oids::TST_INFO),
            der::context(0, &[constructed_octets(&e_content)]),
        ]),
        der::context(0, std::slice::from_ref(&authority_cert.raw)),
        der::set(&[signer_info]),
    ]);
    Ok(der::sequence(&[der::oid(oids::SIGNED_DATA), der::context(0, &[signed_data])]))
}
