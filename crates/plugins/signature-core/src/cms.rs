//! `cms/verify.ts` and `cms/build.ts`.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;

use crate::cert::Certificate;
use crate::crypto;
use crate::der;
use crate::oids::{self, Digest};
use crate::parse::{Internal, parse_cms_internal};

/// `CryptographyVerdict`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cryptography {
    Valid,
    Invalid,
    Unsupported,
}

impl Cryptography {
    pub fn name(self) -> &'static str {
        match self {
            Cryptography::Valid => "valid",
            Cryptography::Invalid => "invalid",
            Cryptography::Unsupported => "unsupported",
        }
    }
}

/// `verifyCmsSignature(bytes)`.
pub fn verify_cms_signature(cms: &[u8]) -> Cryptography {
    match parse_cms_internal(cms) {
        Ok(internal) => verify_parsed(&internal),
        Err(e) if e.reason() == Some("unsupported") => Cryptography::Unsupported,
        Err(_) => Cryptography::Invalid,
    }
}

fn mentions_unsupported(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["unsupported", "unknown", "not supported", "not implemented"].iter().any(|w| lower.contains(w))
}

/// `verifyCmsSignature(parsed)`: the ESS certificate hash, then the
/// signature over the signed attributes, hashed with the DIGEST algorithm
/// (pkijs is handed that hash, so an RSA or ECDSA signature algorithm's own
/// hash is never read).
pub fn verify_parsed(internal: &Internal) -> Cryptography {
    if let Some(ess) = &internal.ess_cert_hash
        && &Digest::Sha256.hash(&internal.parsed.signer_certificate) != ess
    {
        return Cryptography::Invalid;
    }
    let info = &internal.signer_info;
    match crypto::verify_with_public_key(
        &info.signed_attributes_der(),
        &info.signature,
        &internal.signer_certificate.spki,
        &info.signature_algorithm,
        Some(internal.parsed.digest_algorithm),
    ) {
        Ok(true) => Cryptography::Valid,
        Ok(false) => Cryptography::Invalid,
        Err(message) if mentions_unsupported(&message) => Cryptography::Unsupported,
        Err(_) => Cryptography::Invalid,
    }
}

// ── build ────────────────────────────────────────────────────────────────────

/// `SignatureProfile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Pkcs7,
    CadesB,
}

impl Profile {
    pub fn from_name(name: &str) -> Option<Profile> {
        match name {
            "pkcs7" => Some(Profile::Pkcs7),
            "cades-b" => Some(Profile::CadesB),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Profile::Pkcs7 => "pkcs7",
            Profile::CadesB => "cades-b",
        }
    }
}

/// `RawSignatureAlgorithm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawAlgorithm {
    RsaPkcs1,
    RsaPss,
    Ecdsa,
}

impl RawAlgorithm {
    pub fn from_name(name: &str) -> Option<RawAlgorithm> {
        match name {
            "RSA-PKCS1-v1_5" => Some(RawAlgorithm::RsaPkcs1),
            "RSA-PSS" => Some(RawAlgorithm::RsaPss),
            "ECDSA" => Some(RawAlgorithm::Ecdsa),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RawAlgorithm::RsaPkcs1 => "RSA-PKCS1-v1_5",
            RawAlgorithm::RsaPss => "RSA-PSS",
            RawAlgorithm::Ecdsa => "ECDSA",
        }
    }
}

/// `RawSigner`: signs the DER of the signed attributes with a key it never
/// hands out. For ECDSA the answer is `r || s`, as WebCrypto produces it.
#[derive(Clone)]
pub struct RawSigner {
    pub certificate_chain: Vec<Vec<u8>>,
    pub algorithm: RawAlgorithm,
    pub hash: Digest,
    pub sign: Rc<dyn Fn(Vec<u8>) -> Promise<Vec<u8>>>,
}

/// `BuildDetachedCmsInput`.
#[derive(Clone)]
pub struct BuildInput {
    pub digest: Vec<u8>,
    pub hash: Digest,
    pub profile: Profile,
    pub signer: RawSigner,
    /// Epoch milliseconds; `pkcs7` only.
    pub signing_time: Option<f64>,
}

pub(crate) fn attribute(oid: &str, value: Vec<u8>) -> Vec<u8> {
    der::sequence(&[der::oid(oid), der::set(&[value])])
}

/// `signatureAlgorithmFor`: the SignerInfo signature algorithm of a raw key
/// and hash.
pub fn signature_algorithm_for(algorithm: RawAlgorithm, hash: Digest) -> Vec<u8> {
    match algorithm {
        RawAlgorithm::RsaPkcs1 => {
            let oid = match hash {
                Digest::Sha384 => oids::SHA384_WITH_RSA,
                Digest::Sha512 => oids::SHA512_WITH_RSA,
                _ => oids::SHA256_WITH_RSA,
            };
            der::sequence(&[der::oid(oid), der::null()])
        }
        RawAlgorithm::Ecdsa => {
            let oid = match hash {
                Digest::Sha384 => oids::ECDSA_SHA384,
                Digest::Sha512 => oids::ECDSA_SHA512,
                _ => oids::ECDSA_SHA256,
            };
            der::sequence(&[der::oid(oid)])
        }
        RawAlgorithm::RsaPss => {
            der::sequence(&[der::oid(oids::RSA_PSS), pss_params(hash, hash.size() as u64)])
        }
    }
}

/// `new RSASSAPSSParams({ hashAlgorithm, maskGenAlgorithm, saltLength }).toSchema()`:
/// members at their default are left out.
pub fn pss_params(hash: Digest, salt: u64) -> Vec<u8> {
    let hash_algorithm = der::sequence(&[der::oid(hash.oid()), der::null()]);
    let mut members = Vec::new();
    if hash != Digest::Sha1 {
        members.push(der::context(0, std::slice::from_ref(&hash_algorithm)));
        members.push(der::context(1, &[der::sequence(&[der::oid(oids::MGF1), hash_algorithm])]));
    }
    if salt != 20 {
        members.push(der::context(2, &[der::integer(salt)]));
    }
    der::sequence(&members)
}

/// `createCMSECDSASignature`: `r || s` into a SEQUENCE of two INTEGERs; an
/// odd length is an empty signature.
pub fn cms_ecdsa_signature(raw: &[u8]) -> Vec<u8> {
    if !raw.len().is_multiple_of(2) {
        return Vec::new();
    }
    let (r, s) = raw.split_at(raw.len() / 2);
    der::sequence(&[der::integer_der(r), der::integer_der(s)])
}

/// `cmsSignatureValue`: a raw signature as a SignerInfo carries it (ECDSA
/// `r || s` becomes DER).
pub fn cms_signature_value(algorithm: RawAlgorithm, raw: Vec<u8>) -> Vec<u8> {
    if algorithm == RawAlgorithm::Ecdsa { cms_ecdsa_signature(&raw) } else { raw }
}

/// `buildDetachedCms`.
pub async fn build_detached_cms(input: BuildInput) -> Result<Vec<u8>, Error> {
    if input.digest.len() != input.hash.size() {
        return Err(Error::new(format!(
            "digest is {} bytes; {} needs {}",
            input.digest.len(),
            input.hash.name(),
            input.hash.size()
        )));
    }
    if input.signer.hash != input.hash {
        return Err(Error::new(format!(
            "the signer hashes with {}, the document was prepared with {}",
            input.signer.hash.name(),
            input.hash.name()
        )));
    }
    if input.signer.certificate_chain.is_empty() {
        return Err(Error::new("the signer carries no certificate"));
    }
    let chain = input
        .signer
        .certificate_chain
        .iter()
        .map(|der| Certificate::from_ber(der))
        .collect::<Result<Vec<_>, _>>()?;
    let signer_cert = &chain[0];

    let mut attributes = vec![
        attribute(oids::CONTENT_TYPE, der::oid(oids::DATA)),
        attribute(oids::MESSAGE_DIGEST, der::octet_string(&input.digest)),
    ];
    if input.profile == Profile::Pkcs7
        && let Some(time) = input.signing_time
    {
        attributes.push(attribute(oids::SIGNING_TIME, der::utc_time(time)));
    }
    if input.profile == Profile::CadesB {
        let cert_hash = Digest::Sha256.hash(&input.signer.certificate_chain[0]);
        let issuer_serial = der::sequence(&[
            der::sequence(&[der::context(4, std::slice::from_ref(&signer_cert.issuer.raw))]),
            der::integer_bytes(&signer_cert.serial),
        ]);
        let ess_cert_id = der::sequence(&[der::octet_string(&cert_hash), issuer_serial]);
        attributes.push(attribute(
            oids::SIGNING_CERTIFICATE_V2,
            der::sequence(&[der::sequence(&[ess_cert_id])]),
        ));
    }

    let digest_algorithm = der::sequence(&[der::oid(input.hash.oid())]);
    let to_sign = der::set(&attributes);
    let raw = (input.signer.sign)(to_sign).await?;
    let signature = cms_signature_value(input.signer.algorithm, raw);

    let signer_info = der::sequence(&[
        der::integer(1),
        der::sequence(&[signer_cert.issuer.raw.clone(), der::integer_bytes(&signer_cert.serial)]),
        digest_algorithm.clone(),
        der::context(0, &attributes),
        signature_algorithm_for(input.signer.algorithm, input.hash),
        der::octet_string(&signature),
    ]);
    let signed_data = der::sequence(&[
        der::integer(1),
        der::set(&[digest_algorithm]),
        der::sequence(&[der::oid(oids::DATA)]),
        der::context(0, &chain.iter().map(|c| c.raw.clone()).collect::<Vec<_>>()),
        der::set(&[signer_info]),
    ]);
    Ok(der::sequence(&[der::oid(oids::SIGNED_DATA), der::context(0, &[signed_data])]))
}
