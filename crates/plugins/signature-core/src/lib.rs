//! `packages/core/signature` in Rust: the cryptographic half of digital
//! signatures.
//!
//! - [`parse`]: a detached CMS SignedData with one signer, read as pkijs
//!   reads it, refusals and all.
//! - [`cms`]: verify its signature ([`cms::verify_cms_signature`]) and build
//!   one around a raw signature ([`cms::build_detached_cms`]).
//! - [`gate`]: the check between a signer's answer and `signatures.complete`.
//! - [`trust`]: certificate chains, validated as pkijs's chain engine does,
//!   with [`policy`] for its certificate-policy and name-constraint half.
//! - [`sign`] and [`verdict`]: `sign()` and `validateSignatures()` over a
//!   document handle's `signatures` service.
//! - [`signers`]: the signer ports over a host's WebCrypto.
//!
//! The crate links no PDFium. WebCrypto is the host's ([`signers::WebCrypto`]),
//! as are trust anchors ([`trust::TrustPort`]) and key storage
//! ([`signers::PersonalKeyStore`]).

pub mod cert;
pub mod cms;
pub mod crypto;
pub mod der;
pub mod gate;
pub mod oids;
pub mod parse;
pub mod policy;
pub mod sign;
pub mod signers;
pub mod trust;
pub mod verdict;

#[cfg(feature = "harness")]
pub mod harness;

pub use cms::{
    BuildInput, Cryptography, Profile, RawAlgorithm, RawSigner, build_detached_cms,
    verify_cms_signature,
};
pub use gate::{Gate, Prepared, verify_for_completion};
pub use oids::Digest;
pub use parse::{ParseError, ParsedCms, parse_detached_cms};
pub use sign::{CmsSignRequest, CmsSigner, SignerPort, profile_for, sign};
pub use trust::{TrustPort, TrustStatus, TrustVerdict, ValidationTime, validate_chain};
pub use verdict::{ValidateOptions, validate_signatures};

/// A number as JavaScript's `String(n)` prints it.
pub fn number_text(n: f64) -> String {
    epdf_geometry::js::number_to_string(n)
}
