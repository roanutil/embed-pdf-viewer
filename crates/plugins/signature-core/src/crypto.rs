//! Signature verification as pkijs's `CryptoEngine.verifyWithPublicKey`
//! does it over WebCrypto: which algorithm the key is imported as, which
//! hash, which salt, how an ECDSA signature is unwrapped, and the text of
//! each refusal. `Err` is a thrown error; `Ok(false)` is WebCrypto's
//! `verify` answering false.

use ecdsa::signature::hazmat::PrehashVerifier as _;
use rsa::{BigUint, Pkcs1v15Sign, Pss, RsaPublicKey};

use crate::cert::{AlgId, Spki};
use crate::der;
use crate::oids::{self, Digest};

/// `getHashAlgorithm(signatureAlgorithm)`: `None` is pkijs's empty string.
pub fn hash_of(algorithm: &AlgId) -> Option<Digest> {
    match algorithm.oid.as_str() {
        "1.2.840.10045.4.1" | "1.2.840.113549.1.1.5" => Some(Digest::Sha1),
        oids::ECDSA_SHA256 | oids::SHA256_WITH_RSA => Some(Digest::Sha256),
        oids::ECDSA_SHA384 | oids::SHA384_WITH_RSA => Some(Digest::Sha384),
        oids::ECDSA_SHA512 | oids::SHA512_WITH_RSA => Some(Digest::Sha512),
        oids::RSA_PSS => match pss_params(algorithm) {
            Ok(params) => match params.hash {
                Some(oid) => Digest::from_oid(&oid),
                None => Some(Digest::Sha1),
            },
            Err(_) => None,
        },
        _ => None,
    }
}

/// The names `getAlgorithmByOID` answers for the identifiers a key or a
/// signature can carry.
fn algorithm_name(oid: &str) -> Option<&'static str> {
    Some(match oid {
        oids::RSA_ENCRYPTION => "RSAES-PKCS1-v1_5",
        "1.2.840.113549.1.1.5"
        | oids::SHA256_WITH_RSA
        | oids::SHA384_WITH_RSA
        | oids::SHA512_WITH_RSA => "RSASSA-PKCS1-v1_5",
        oids::RSA_PSS => "RSA-PSS",
        "1.2.840.113549.1.1.7" => "RSA-OAEP",
        oids::EC_PUBLIC_KEY
        | "1.2.840.10045.4.1"
        | oids::ECDSA_SHA256
        | oids::ECDSA_SHA384
        | oids::ECDSA_SHA512 => "ECDSA",
        "1.3.133.16.840.63.0.2" | "1.3.132.1.11.1" | "1.3.132.1.11.2" | "1.3.132.1.11.3" => "ECDH",
        "2.16.840.1.101.3.4.1.2" | "2.16.840.1.101.3.4.1.22" | "2.16.840.1.101.3.4.1.42" => {
            "AES-CBC"
        }
        "2.16.840.1.101.3.4.1.6" | "2.16.840.1.101.3.4.1.26" | "2.16.840.1.101.3.4.1.46" => {
            "AES-GCM"
        }
        "2.16.840.1.101.3.4.1.4" | "2.16.840.1.101.3.4.1.24" | "2.16.840.1.101.3.4.1.44" => {
            "AES-CFB"
        }
        "2.16.840.1.101.3.4.1.5" | "2.16.840.1.101.3.4.1.25" | "2.16.840.1.101.3.4.1.45" => {
            "AES-KW"
        }
        "1.2.840.113549.2.7"
        | "1.2.840.113549.2.9"
        | "1.2.840.113549.2.10"
        | "1.2.840.113549.2.11" => "HMAC",
        "1.2.840.113549.1.9.16.3.5" => "DH",
        oids::SHA1 => "SHA-1",
        oids::SHA256 => "SHA-256",
        oids::SHA384 => "SHA-384",
        oids::SHA512 => "SHA-512",
        "1.2.840.113549.1.5.12" => "PBKDF2",
        "1.2.840.10045.3.1.7" => "P-256",
        "1.3.132.0.34" => "P-384",
        "1.3.132.0.35" => "P-521",
        _ => return None,
    })
}

fn unsupported_identifier(oid: &str) -> String {
    format!("Unsupported algorithm identifier : {oid}")
}

/// `RSASSAPSSParams`: the hash OID and salt length, each absent when it
/// takes its default.
pub struct PssParams {
    pub hash: Option<String>,
    pub salt: Option<u64>,
}

pub fn pss_params(algorithm: &AlgId) -> Result<PssParams, String> {
    let params = algorithm.params.as_ref().filter(|p| p.is_sequence()).ok_or_else(|| {
        "Object's schema was not verified against input data for RSASSAPSSParams".to_string()
    })?;
    let mut out = PssParams { hash: None, salt: None };
    for member in &params.children {
        match (member.class, member.tag) {
            (der::CONTEXT, 0) => {
                out.hash = member.children.first().and_then(AlgId::read).map(|a| a.oid);
            }
            (der::CONTEXT, 2) => {
                let int = member.children.first().map(|n| n.content.clone()).unwrap_or_default();
                out.salt = Some(int.iter().fold(0u64, |acc, &b| acc * 256 + u64::from(b)));
            }
            _ => {}
        }
    }
    Ok(out)
}

enum Key {
    Rsa(RsaPublicKey),
    P256(p256::ecdsa::VerifyingKey),
    P384(p384::ecdsa::VerifyingKey),
    P521(p521::ecdsa::VerifyingKey),
}

const IMPORT_FAILED: &str = "DataError: the key data could not be imported";

fn rsa_key(spki: &Spki) -> Result<RsaPublicKey, String> {
    let node = der::from_ber(&spki.key).filter(|n| n.is_sequence()).ok_or(IMPORT_FAILED)?;
    let n = node.children.first().filter(|n| n.is_universal(der::INTEGER)).ok_or(IMPORT_FAILED)?;
    let e = node.children.get(1).filter(|n| n.is_universal(der::INTEGER)).ok_or(IMPORT_FAILED)?;
    RsaPublicKey::new(BigUint::from_bytes_be(&n.content), BigUint::from_bytes_be(&e.content))
        .map_err(|_| IMPORT_FAILED.to_string())
}

fn import(name: &str, curve: Option<&str>, spki: &Spki) -> Result<Key, String> {
    match name {
        "RSAES-PKCS1-v1_5" | "RSASSA-PKCS1-v1_5" | "RSA-PSS" => {
            if spki.algorithm.oid != oids::RSA_ENCRYPTION && spki.algorithm.oid != oids::RSA_PSS {
                return Err(IMPORT_FAILED.into());
            }
            Ok(Key::Rsa(rsa_key(spki)?))
        }
        "ECDSA" => {
            let key_curve = spki.algorithm.params.as_ref().and_then(|p| p.oid());
            if spki.algorithm.oid != oids::EC_PUBLIC_KEY
                || key_curve.as_deref().and_then(algorithm_name) != curve
            {
                return Err(IMPORT_FAILED.into());
            }
            let fail = |_| IMPORT_FAILED.to_string();
            Ok(match curve {
                Some("P-256") => {
                    Key::P256(p256::ecdsa::VerifyingKey::from_sec1_bytes(&spki.key).map_err(fail)?)
                }
                Some("P-384") => {
                    Key::P384(p384::ecdsa::VerifyingKey::from_sec1_bytes(&spki.key).map_err(fail)?)
                }
                Some("P-521") => {
                    Key::P521(p521::ecdsa::VerifyingKey::from_sec1_bytes(&spki.key).map_err(fail)?)
                }
                _ => return Err(IMPORT_FAILED.into()),
            })
        }
        other => Err(format!("NotSupportedError: Unable to import {other} key with format spki")),
    }
}

/// The ECDSA curve a key names, checked as `verifyWithPublicKey` checks it.
fn curve_of(spki: &Spki, explicit: bool) -> Result<&'static str, String> {
    let params = spki.algorithm.params.as_ref();
    if !explicit && params.is_none() {
        return Err("Algorithm parameters for ECDSA public key are missed".into());
    }
    let oid =
        params.and_then(|p| p.oid()).ok_or("Incorrect type for ECDSA public key parameters")?;
    algorithm_name(&oid).ok_or_else(|| unsupported_identifier(&oid))
}

/// `createECDSASignatureFromCMS`: a SEQUENCE of two INTEGERs into `r || s`,
/// each right-aligned in `size` bytes; anything else is an empty signature.
fn ecdsa_raw(signature: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let node = der::from_ber(signature).ok_or_else(|| {
        "Error during parsing of ASN.1 data. Data is not correct for 'Signature value'.".to_string()
    })?;
    let ints = node.is_sequence()
        && node.children.len() == 2
        && node.children.iter().all(|c| c.is_universal(der::INTEGER));
    if !ints {
        return Ok(Vec::new());
    }
    let mut out = vec![0u8; size * 2];
    for (i, int) in node.children.iter().enumerate() {
        let value = der::integer_from_der(&int.content);
        if value.len() > size {
            return Err("offset is out of bounds".into());
        }
        let end = size * (i + 1);
        out[end - value.len()..end].copy_from_slice(&value);
    }
    Ok(out)
}

/// A prehash the curve's `bits2field` accepts: WebCrypto reads a hash shorter
/// than the field as a smaller integer.
fn prehash(digest: Vec<u8>, field: usize) -> Vec<u8> {
    if digest.len() * 2 >= field {
        return digest;
    }
    let mut out = vec![0u8; field / 2 + 1 - digest.len()];
    out.extend(digest);
    out
}

fn verify_ecdsa(key: &Key, raw: &[u8], digest: Vec<u8>) -> bool {
    match key {
        Key::P256(k) => p256::ecdsa::Signature::from_slice(raw)
            .is_ok_and(|s| k.verify_prehash(&prehash(digest, 32), &s).is_ok()),
        Key::P384(k) => p384::ecdsa::Signature::from_slice(raw)
            .is_ok_and(|s| k.verify_prehash(&prehash(digest, 48), &s).is_ok()),
        Key::P521(k) => p521::ecdsa::Signature::from_slice(raw)
            .is_ok_and(|s| k.verify_prehash(&prehash(digest, 66), &s).is_ok()),
        Key::Rsa(_) => false,
    }
}

fn pkcs1(hash: Digest) -> Pkcs1v15Sign {
    match hash {
        Digest::Sha1 => Pkcs1v15Sign::new::<sha1::Sha1>(),
        Digest::Sha256 => Pkcs1v15Sign::new::<sha2::Sha256>(),
        Digest::Sha384 => Pkcs1v15Sign::new::<sha2::Sha384>(),
        Digest::Sha512 => Pkcs1v15Sign::new::<sha2::Sha512>(),
    }
}

fn pss(hash: Digest, salt: usize) -> Pss {
    match hash {
        Digest::Sha1 => Pss::new_with_salt::<sha1::Sha1>(salt),
        Digest::Sha256 => Pss::new_with_salt::<sha2::Sha256>(salt),
        Digest::Sha384 => Pss::new_with_salt::<sha2::Sha384>(salt),
        Digest::Sha512 => Pss::new_with_salt::<sha2::Sha512>(salt),
    }
}

/// `verifyWithPublicKey(data, signature, publicKeyInfo, signatureAlgorithm,
/// shaAlgorithm?)`.
pub fn verify_with_public_key(
    data: &[u8],
    signature: &[u8],
    spki: &Spki,
    signature_algorithm: &AlgId,
    sha: Option<Digest>,
) -> Result<bool, String> {
    let unsupported_signature =
        || format!("Unsupported signature algorithm: {}", signature_algorithm.oid);
    let is_pss = signature_algorithm.oid == oids::RSA_PSS;
    let algorithm_id = if is_pss { &signature_algorithm.oid } else { &spki.algorithm.oid };
    let (sha, name, curve) = match sha {
        None => {
            let sha = hash_of(signature_algorithm).ok_or_else(unsupported_signature)?;
            let name =
                algorithm_name(algorithm_id).ok_or_else(|| unsupported_identifier(algorithm_id))?;
            let curve = if name == "ECDSA" { Some(curve_of(spki, false)?) } else { None };
            (sha, name, curve)
        }
        Some(sha) => {
            let name =
                algorithm_name(algorithm_id).ok_or_else(|| unsupported_identifier(algorithm_id))?;
            let curve = if name == "ECDSA" { Some(curve_of(spki, true)?) } else { None };
            (sha, name, curve)
        }
    };
    let key = import(name, curve, spki)?;
    match key {
        Key::Rsa(ref rsa) if name == "RSA-PSS" => {
            let params = pss_params(signature_algorithm)?;
            let salt = params.salt.unwrap_or(20) as usize;
            let hash = match params.hash {
                Some(oid) => match algorithm_name(&oid) {
                    Some(n) => {
                        Digest::from_webcrypto(n).ok_or_else(|| format!("Unrecognized name {n}"))?
                    }
                    None => return Err(unsupported_identifier(&oid)),
                },
                None => Digest::Sha1,
            };
            Ok(rsa.verify(pss(hash, salt), &hash.hash(data), signature).is_ok())
        }
        Key::Rsa(ref rsa) => Ok(rsa.verify(pkcs1(sha), &sha.hash(data), signature).is_ok()),
        ref ec => {
            let size = match curve {
                Some("P-256") => 32,
                Some("P-384") => 48,
                _ => 66,
            };
            let raw = ecdsa_raw(signature, size)?;
            Ok(verify_ecdsa(ec, &raw, sha.hash(data)))
        }
    }
}
