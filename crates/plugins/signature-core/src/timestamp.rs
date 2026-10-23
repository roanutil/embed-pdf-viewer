//! `cms/timestamp.ts`: an RFC 3161 timestamp token's TSTInfo, read and
//! built.

use epdf_kernel::Error;

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

/// What pkijs's `TSTInfo` schema requires, in order: `version`, `policy`,
/// `messageImprint`, `serialNumber` and `genTime`. asn1js ignores members
/// past the schema's, and the optional ones (accuracy, ordering, nonce,
/// tsa, extensions) are never read here.
fn tst_info_fields(node: &Node) -> Option<(AlgId, Vec<u8>, f64)> {
    if !node.is_sequence() {
        return None;
    }
    let mut members = node.children.iter();
    members.next().filter(|n| n.is_universal(der::INTEGER))?;
    members.next()?.oid()?;
    let imprint = members.next().filter(|n| n.is_sequence())?;
    let algorithm = AlgId::read(imprint.children.first()?)?;
    let hashed = imprint.children.get(1).filter(|n| n.is_octet_string())?.octets();
    members.next().filter(|n| n.is_universal(der::INTEGER))?;
    let gen_time = members.next().filter(|n| n.is_universal(der::GENERALIZED_TIME))?.time()?;
    Some((algorithm, hashed, gen_time))
}

/// `readTimestampInfo`: a token's TSTInfo (the encapsulated content of an
/// RFC 3161 token). Fails with `CmsError` as the TypeScript throws it.
pub fn read_timestamp_info(e_content: &[u8]) -> Result<TimestampInfo, ParseError> {
    let node = der::from_ber(e_content).ok_or_else(|| ParseError::Cms {
        reason: "malformed",
        message: "TSTInfo is not valid BER".into(),
    })?;
    let (algorithm, imprint, gen_time) = tst_info_fields(&node).ok_or_else(|| ParseError::Cms {
        reason: "malformed",
        message: format!("TSTInfo does not parse: {}", der::schema_error("TSTInfo").message),
    })?;
    let imprint_algorithm = Digest::from_oid(&algorithm.oid).ok_or_else(|| ParseError::Cms {
        reason: "unsupported",
        message: format!("message imprint algorithm {}", algorithm.oid),
    })?;
    Ok(TimestampInfo { imprint_algorithm, imprint, gen_time })
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
