//! `cms/oids.ts`: the object identifiers and digest tables.

use serde::{Deserialize, Serialize};

pub const DATA: &str = "1.2.840.113549.1.7.1";
pub const SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
pub const CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
pub const MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
pub const SIGNING_TIME: &str = "1.2.840.113549.1.9.5";
pub const SIGNING_CERTIFICATE_V2: &str = "1.2.840.113549.1.9.16.2.47";
pub const TIMESTAMP_TOKEN: &str = "1.2.840.113549.1.9.16.2.14";
pub const SHA1: &str = "1.3.14.3.2.26";
pub const SHA256: &str = "2.16.840.1.101.3.4.2.1";
pub const SHA384: &str = "2.16.840.1.101.3.4.2.2";
pub const SHA512: &str = "2.16.840.1.101.3.4.2.3";
pub const RSA_ENCRYPTION: &str = "1.2.840.113549.1.1.1";
pub const SHA256_WITH_RSA: &str = "1.2.840.113549.1.1.11";
pub const SHA384_WITH_RSA: &str = "1.2.840.113549.1.1.12";
pub const SHA512_WITH_RSA: &str = "1.2.840.113549.1.1.13";
pub const RSA_PSS: &str = "1.2.840.113549.1.1.10";
pub const MGF1: &str = "1.2.840.113549.1.1.8";
pub const ECDSA_SHA256: &str = "1.2.840.10045.4.3.2";
pub const ECDSA_SHA384: &str = "1.2.840.10045.4.3.3";
pub const ECDSA_SHA512: &str = "1.2.840.10045.4.3.4";
pub const SUBJECT_KEY_IDENTIFIER: &str = "2.5.29.14";
pub const KEY_USAGE: &str = "2.5.29.15";
pub const BASIC_CONSTRAINTS: &str = "2.5.29.19";
pub const AUTHORITY_KEY_IDENTIFIER: &str = "2.5.29.35";
pub const COMMON_NAME: &str = "2.5.4.3";
pub const EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";

/// `DigestAlgorithm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Digest {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Digest {
    /// `DIGEST_BY_OID`.
    pub fn from_oid(oid: &str) -> Option<Digest> {
        match oid {
            SHA1 => Some(Digest::Sha1),
            SHA256 => Some(Digest::Sha256),
            SHA384 => Some(Digest::Sha384),
            SHA512 => Some(Digest::Sha512),
            _ => None,
        }
    }

    /// `OID_BY_DIGEST`.
    pub fn oid(self) -> &'static str {
        match self {
            Digest::Sha1 => SHA1,
            Digest::Sha256 => SHA256,
            Digest::Sha384 => SHA384,
            Digest::Sha512 => SHA512,
        }
    }

    /// `DIGEST_LENGTH`.
    pub fn size(self) -> usize {
        match self {
            Digest::Sha1 => 20,
            Digest::Sha256 => 32,
            Digest::Sha384 => 48,
            Digest::Sha512 => 64,
        }
    }

    /// `WEBCRYPTO_HASH`.
    pub fn webcrypto(self) -> &'static str {
        match self {
            Digest::Sha1 => "SHA-1",
            Digest::Sha256 => "SHA-256",
            Digest::Sha384 => "SHA-384",
            Digest::Sha512 => "SHA-512",
        }
    }

    pub fn from_webcrypto(name: &str) -> Option<Digest> {
        match name.to_ascii_uppercase().as_str() {
            "SHA-1" => Some(Digest::Sha1),
            "SHA-256" => Some(Digest::Sha256),
            "SHA-384" => Some(Digest::Sha384),
            "SHA-512" => Some(Digest::Sha512),
            _ => None,
        }
    }

    /// `'sha256'` and the rest, as the engine and the TypeScript spell them.
    pub fn from_name(name: &str) -> Option<Digest> {
        match name {
            "sha1" => Some(Digest::Sha1),
            "sha256" => Some(Digest::Sha256),
            "sha384" => Some(Digest::Sha384),
            "sha512" => Some(Digest::Sha512),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Digest::Sha1 => "sha1",
            Digest::Sha256 => "sha256",
            Digest::Sha384 => "sha384",
            Digest::Sha512 => "sha512",
        }
    }

    pub fn hash(self, data: &[u8]) -> Vec<u8> {
        use sha2::Digest as _;
        match self {
            Digest::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Digest::Sha256 => sha2::Sha256::digest(data).to_vec(),
            Digest::Sha384 => sha2::Sha384::digest(data).to_vec(),
            Digest::Sha512 => sha2::Sha512::digest(data).to_vec(),
        }
    }
}
