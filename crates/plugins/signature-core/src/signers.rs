//! `signers/*`: the signer ports over a host's WebCrypto.
//!
//! WebCrypto is the host's: [`WebCrypto`] is the four calls this package and
//! pkijs make of it (`generateKey`, `sign`, `exportKey('jwk')` and
//! `getRandomValues`). Everything around those calls, the parameters, the
//! self-signed certificate's DER, the key store protocol, is ported.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

use crate::cert::Certificate;
use crate::cms::{RawAlgorithm, RawSigner, cms_ecdsa_signature, pss_params};
use crate::der;
use crate::oids::{self, Digest};
use crate::sign::{CmsSignRequest, CmsSigner};

/// A WebCrypto `CryptoKey`: what JavaScript can see of it, and the host's
/// handle to the key itself.
#[derive(Clone)]
pub struct CryptoKey {
    pub key_type: String,
    pub extractable: bool,
    /// `key.algorithm`, as WebCrypto reports it.
    pub algorithm: Value,
    pub usages: Vec<String>,
    pub handle: Rc<dyn Any>,
}

impl CryptoKey {
    pub fn describe(&self) -> Value {
        json!({ "type": self.key_type, "extractable": self.extractable, "algorithm": self.algorithm, "usages": self.usages })
    }

    fn algorithm_name(&self) -> &str {
        self.algorithm.get("name").and_then(Value::as_str).unwrap_or_default()
    }

    fn hash_name(&self) -> Option<&str> {
        self.algorithm.get("hash").and_then(|h| h.get("name")).and_then(Value::as_str)
    }
}

#[derive(Clone)]
pub struct CryptoKeyPair {
    pub private_key: CryptoKey,
    pub public_key: CryptoKey,
}

/// The host's WebCrypto.
pub trait WebCrypto {
    fn generate_key(
        &self,
        params: Value,
        extractable: bool,
        usages: Vec<String>,
    ) -> Promise<CryptoKeyPair>;
    fn sign(&self, params: Value, key: &CryptoKey, data: Vec<u8>) -> Promise<Vec<u8>>;
    /// `exportKey('jwk', key)`.
    fn export_jwk(&self, key: &CryptoKey) -> Promise<Value>;
    fn get_random_values(&self, buffer: &mut [u8]);
}

fn hash_from_webcrypto(name: &str) -> Result<Digest, Error> {
    match name {
        "SHA-256" => Ok(Digest::Sha256),
        "SHA-384" => Ok(Digest::Sha384),
        "SHA-512" => Ok(Digest::Sha512),
        other => Err(Error::new(format!("unsupported key hash {other}"))),
    }
}

/// `webCryptoSigner`: a raw signer over a private key that never leaves the
/// host. RSA keys carry their hash; an ECDSA key takes `hash`, sha256 by
/// default.
pub fn web_crypto_signer(
    crypto: Rc<dyn WebCrypto>,
    private_key: CryptoKey,
    certificate_chain: Vec<Vec<u8>>,
    hash: Option<Digest>,
) -> Result<RawSigner, Error> {
    let (algorithm, hash) = match private_key.algorithm_name() {
        "RSASSA-PKCS1-v1_5" => (
            RawAlgorithm::RsaPkcs1,
            hash_from_webcrypto(private_key.hash_name().unwrap_or_default())?,
        ),
        "RSA-PSS" => (
            RawAlgorithm::RsaPss,
            hash_from_webcrypto(private_key.hash_name().unwrap_or_default())?,
        ),
        "ECDSA" => (RawAlgorithm::Ecdsa, hash.unwrap_or(Digest::Sha256)),
        other => return Err(Error::new(format!("unsupported WebCrypto key algorithm {other}"))),
    };
    let key = private_key.clone();
    Ok(RawSigner {
        certificate_chain,
        algorithm,
        hash,
        sign: Rc::new(move |data| {
            let params = match algorithm {
                RawAlgorithm::RsaPss => json!({ "name": "RSA-PSS", "saltLength": hash.size() }),
                RawAlgorithm::Ecdsa => json!({ "name": "ECDSA", "hash": hash.webcrypto() }),
                RawAlgorithm::RsaPkcs1 => json!({ "name": "RSASSA-PKCS1-v1_5" }),
            };
            crypto.sign(params, &key, data)
        }),
    })
}

/// `generateSigningKeyPair`.
pub fn generate_signing_key_pair(
    crypto: &dyn WebCrypto,
    algorithm: RawAlgorithm,
    hash: Digest,
    extractable: bool,
) -> Promise<CryptoKeyPair> {
    let params = match algorithm {
        RawAlgorithm::Ecdsa => json!({ "name": "ECDSA", "namedCurve": "P-256" }),
        _ => json!({
            "name": if algorithm == RawAlgorithm::RsaPss { "RSA-PSS" } else { "RSASSA-PKCS1-v1_5" },
            "modulusLength": 2048,
            "publicExponent": Bytes(vec![1, 0, 1]).to_json(),
            "hash": hash.webcrypto(),
        }),
    };
    crypto.generate_key(params, extractable, vec!["sign".into(), "verify".into()])
}

fn jwk_bytes(jwk: &Value, key: &str) -> Vec<u8> {
    jwk.get(key)
        .and_then(Value::as_str)
        .and_then(|s| URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok())
        .unwrap_or_default()
}

/// pkijs's `exportKey('spki')`: the JWK the host exports, re-encoded as
/// `PublicKeyInfo.fromJSON` builds it.
pub fn spki_from_jwk(jwk: &Value) -> Result<Vec<u8>, Error> {
    let incorrect = || Error::new("Error during exporting public key: Incorrect key data");
    match jwk.get("kty").and_then(Value::as_str).map(str::to_ascii_uppercase).as_deref() {
        Some("RSA") => {
            let n = jwk_bytes(jwk, "n");
            let e = jwk_bytes(jwk, "e");
            if n.is_empty() || e.is_empty() {
                return Err(incorrect());
            }
            // `array.slice(0, 2 ** nearestPowerOf2(length))`, then `convertToDER()`.
            let power = (n.len() as f64).log2();
            let exponent =
                if power.floor() == power.round() { power.floor() } else { power.round() };
            let modulus = &n[..n.len().min(2f64.powf(exponent) as usize)];
            let key = der::sequence(&[
                der::integer_der(modulus),
                der::integer_bytes(&e[..e.len().min(3)]),
            ]);
            Ok(der::sequence(&[
                der::sequence(&[der::oid(oids::RSA_ENCRYPTION), der::null()]),
                der::bit_string(0, &key),
            ]))
        }
        Some("EC") => {
            let (curve, size) = match jwk.get("crv").and_then(Value::as_str) {
                Some("P-256") => ("1.2.840.10045.3.1.7", 32),
                Some("P-384") => ("1.3.132.0.34", 48),
                Some("P-521") => ("1.3.132.0.35", 66),
                _ => return Err(incorrect()),
            };
            let coordinate = |key| {
                let raw = jwk_bytes(jwk, key);
                if raw.len() < size {
                    let mut out = vec![0u8; size];
                    let end = (1 + raw.len()).min(size);
                    out[1..end].copy_from_slice(&raw[..end - 1]);
                    out
                } else {
                    raw[..size].to_vec()
                }
            };
            let mut point = vec![0x04];
            point.extend(coordinate("x"));
            point.extend(coordinate("y"));
            Ok(der::sequence(&[
                der::sequence(&[der::oid(oids::EC_PUBLIC_KEY), der::oid(curve)]),
                der::bit_string(0, &point),
            ]))
        }
        _ => Err(incorrect()),
    }
}

/// pkijs's `getSignatureParameters(privateKey, hashAlgorithm)`: the
/// WebCrypto sign parameters and the certificate's signature algorithm.
fn signature_parameters(key: &CryptoKey, hash_name: &str) -> Result<(Value, Vec<u8>), Error> {
    let hash = key.hash_name().unwrap_or(hash_name).to_string();
    let digest = Digest::from_webcrypto(&hash)
        .ok_or_else(|| Error::new(format!("Unsupported hash algorithm: {hash}")))?;
    match key.algorithm_name().to_ascii_uppercase().as_str() {
        "RSASSA-PKCS1-V1_5" => {
            let oid = match digest {
                Digest::Sha1 => "1.2.840.113549.1.1.5",
                Digest::Sha256 => oids::SHA256_WITH_RSA,
                Digest::Sha384 => oids::SHA384_WITH_RSA,
                Digest::Sha512 => oids::SHA512_WITH_RSA,
            };
            Ok((
                json!({ "name": "RSASSA-PKCS1-v1_5", "hash": { "name": hash } }),
                der::sequence(&[der::oid(oid)]),
            ))
        }
        "ECDSA" => {
            let oid = match digest {
                Digest::Sha1 => "1.2.840.10045.4.1",
                Digest::Sha256 => oids::ECDSA_SHA256,
                Digest::Sha384 => oids::ECDSA_SHA384,
                Digest::Sha512 => oids::ECDSA_SHA512,
            };
            Ok((
                json!({ "name": "ECDSA", "hash": { "name": hash } }),
                der::sequence(&[der::oid(oid)]),
            ))
        }
        "RSA-PSS" => {
            let salt = match digest {
                Digest::Sha256 => 32,
                Digest::Sha384 => 48,
                Digest::Sha512 => 64,
                Digest::Sha1 => 20,
            };
            Ok((
                json!({ "name": "RSA-PSS", "hash": { "name": hash }, "saltLength": salt }),
                der::sequence(&[der::oid(oids::RSA_PSS), pss_params(digest, salt)]),
            ))
        }
        _ => Err(Error::new(format!("Unsupported signature algorithm: {}", key.algorithm_name()))),
    }
}

const DAY_MS: f64 = 24.0 * 3600.0 * 1000.0;

/// `selfSignedCertificate`: a certificate over a WebCrypto key pair, its own
/// issuer, valid from a day back for `validity_days` (365 by default).
pub async fn self_signed_certificate(
    crypto: Rc<dyn WebCrypto>,
    public_key: CryptoKey,
    private_key: CryptoKey,
    common_name: String,
    hash: Digest,
    validity_days: Option<f64>,
) -> Result<Vec<u8>, Error> {
    let mut serial = [0u8; 8];
    crypto.get_random_values(&mut serial);
    serial[0] &= 0x7f;
    let name = der::sequence(&[der::set(&[der::sequence(&[
        der::oid(oids::COMMON_NAME),
        der::utf8_string(&common_name),
    ])])]);
    let now = epdf_kernel::rt::now();
    let validity = der::sequence(&[
        der::utc_time(now - DAY_MS),
        der::utc_time(now + validity_days.unwrap_or(365.0) * DAY_MS),
    ]);
    let extensions = der::context(
        3,
        &[der::sequence(&[
            der::sequence(&[
                der::oid(oids::BASIC_CONSTRAINTS),
                der::boolean(true),
                der::octet_string(&der::sequence(&[der::boolean(true)])),
            ]),
            der::sequence(&[
                der::oid(oids::KEY_USAGE),
                der::boolean(true),
                der::octet_string(&der::bit_string(2, &[0x84])),
            ]),
        ])],
    );
    let jwk = crypto.export_jwk(&public_key).await?;
    let spki = spki_from_jwk(&jwk)?;
    let (params, algorithm) = signature_parameters(&private_key, hash.webcrypto())?;
    let tbs = der::sequence(&[
        der::context(0, &[der::integer(2)]),
        der::integer_bytes(&serial),
        algorithm.clone(),
        name.clone(),
        validity,
        name,
        spki,
        extensions,
    ]);
    let signature = crypto.sign(params, &private_key, tbs.clone()).await?;
    let signature = if private_key.algorithm_name() == "ECDSA" {
        cms_ecdsa_signature(&signature)
    } else {
        signature
    };
    Ok(der::sequence(&[tbs, algorithm, der::bit_string(0, &signature)]))
}

/// `certificateCommonName`.
pub fn certificate_common_name(der: &[u8]) -> Result<Option<String>, Error> {
    let cert = Certificate::from_ber(der)?;
    Ok(cert
        .subject
        .attributes
        .iter()
        .find(|(t, _)| t == oids::COMMON_NAME)
        .and_then(|(_, v)| v.text())
        .filter(|t| !t.is_empty()))
}

/// `TestSigner`.
#[derive(Clone)]
pub struct TestSigner {
    pub signer: RawSigner,
    pub certificate: Vec<u8>,
    pub private_key: CryptoKey,
}

/// `createTestSigner`: an extractable key pair, a self-signed certificate,
/// never persisted.
pub async fn create_test_signer(
    crypto: Rc<dyn WebCrypto>,
    common_name: Option<String>,
    algorithm: Option<RawAlgorithm>,
    hash: Option<Digest>,
) -> Result<TestSigner, Error> {
    let hash = hash.unwrap_or(Digest::Sha256);
    let keys = generate_signing_key_pair(
        &*crypto,
        algorithm.unwrap_or(RawAlgorithm::RsaPkcs1),
        hash,
        true,
    )
    .await?;
    let certificate = self_signed_certificate(
        crypto.clone(),
        keys.public_key.clone(),
        keys.private_key.clone(),
        common_name.unwrap_or_else(|| "EmbedPDF test signer".into()),
        hash,
        None,
    )
    .await?;
    let signer =
        web_crypto_signer(crypto, keys.private_key.clone(), vec![certificate.clone()], Some(hash))?;
    Ok(TestSigner { signer, certificate, private_key: keys.private_key })
}

/// `PersonalKeyRecord`.
#[derive(Clone)]
pub struct PersonalKeyRecord {
    pub private_key: CryptoKey,
    pub certificate: Vec<u8>,
}

impl PersonalKeyRecord {
    /// As a store callback sees it: a `CryptoKey` has no own members.
    pub fn to_json(&self) -> Value {
        json!({ "privateKey": {}, "certificate": Bytes(self.certificate.clone()).to_json() })
    }
}

/// `PersonalKeyStore`.
pub trait PersonalKeyStore {
    fn load(&self, subject: &str) -> Promise<Option<PersonalKeyRecord>>;
    fn save(&self, subject: &str, record: PersonalKeyRecord) -> Promise<()>;
    fn remove(&self, subject: &str) -> Promise<()>;
}

/// `memoryKeyStore`: tests, and runtimes without IndexedDB. Generic over
/// the record, which it never looks into.
pub struct MemoryKeyStore<R> {
    records: RefCell<HashMap<String, R>>,
}

impl<R: Clone + 'static> MemoryKeyStore<R> {
    pub fn new() -> Self {
        MemoryKeyStore { records: RefCell::new(HashMap::new()) }
    }

    pub fn load(&self, subject: &str) -> Promise<Option<R>> {
        Promise::resolved(self.records.borrow().get(subject).cloned())
    }

    pub fn save(&self, subject: &str, record: R) -> Promise<()> {
        self.records.borrow_mut().insert(subject.to_string(), record);
        Promise::resolved(())
    }

    pub fn remove(&self, subject: &str) -> Promise<()> {
        self.records.borrow_mut().remove(subject);
        Promise::resolved(())
    }
}

impl<R: Clone + 'static> Default for MemoryKeyStore<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl PersonalKeyStore for MemoryKeyStore<PersonalKeyRecord> {
    fn load(&self, subject: &str) -> Promise<Option<PersonalKeyRecord>> {
        MemoryKeyStore::load(self, subject)
    }

    fn save(&self, subject: &str, record: PersonalKeyRecord) -> Promise<()> {
        MemoryKeyStore::save(self, subject, record)
    }

    fn remove(&self, subject: &str) -> Promise<()> {
        MemoryKeyStore::remove(self, subject)
    }
}

/// `PersonalSigner`.
#[derive(Clone)]
pub struct PersonalSigner {
    pub signer: RawSigner,
    pub certificate: Vec<u8>,
    pub subject: String,
}

/// `personalSigner`: one self-signed identity per subject, created with a
/// non-extractable key on first use and kept in the caller's store.
pub async fn personal_signer(
    crypto: Rc<dyn WebCrypto>,
    subject: String,
    store: Rc<dyn PersonalKeyStore>,
    algorithm: Option<RawAlgorithm>,
    hash: Option<Digest>,
) -> Result<PersonalSigner, Error> {
    let hash = hash.unwrap_or(Digest::Sha256);
    let record = match store.load(&subject).await? {
        Some(record) => record,
        None => {
            let keys = generate_signing_key_pair(
                &*crypto,
                algorithm.unwrap_or(RawAlgorithm::RsaPkcs1),
                hash,
                false,
            )
            .await?;
            let certificate = self_signed_certificate(
                crypto.clone(),
                keys.public_key,
                keys.private_key.clone(),
                subject.clone(),
                hash,
                None,
            )
            .await?;
            let record = PersonalKeyRecord { private_key: keys.private_key, certificate };
            store.save(&subject, record.clone()).await?;
            record
        }
    };
    let signer = web_crypto_signer(
        crypto,
        record.private_key.clone(),
        vec![record.certificate.clone()],
        Some(hash),
    )?;
    Ok(PersonalSigner { signer, certificate: record.certificate, subject })
}

/// `remoteSigner`: a CMS signer over the customer's own signing service.
pub fn remote_signer(sign: Rc<dyn Fn(CmsSignRequest) -> Promise<Vec<u8>>>) -> CmsSigner {
    CmsSigner { sign }
}
