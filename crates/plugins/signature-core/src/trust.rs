//! `trust.ts`, and the part of pkijs it hands the work to:
//! `CertificateChainValidationEngine` (pkijs 3.4.0), ported rule for rule.
//!
//! Three of its rules decide most outcomes and are easy to miss:
//!
//! - The path starts from the LAST certificate the CMS carries, not from
//!   the signer. A CMS carrying `[signer, intermediate]` validates the
//!   intermediate, and the signer is never on the path.
//! - A certificate that is itself an anchor is a path of itself twice.
//! - Only the certificates after the first are checked for CA-ness, and
//!   every such failure reads "One of intermediate certificates is not a
//!   CA certificate".

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use serde_json::{Value, json};

use crate::cert::Certificate;
use crate::crypto;
use crate::oids;
use crate::parse::{Internal, parse_cms_internal};

/// `TrustPort`: where anchors come from.
pub trait TrustPort {
    fn anchors(&self) -> Promise<Vec<Vec<u8>>>;
}

/// `ValidationTime`.
#[derive(Clone, Debug, PartialEq)]
pub enum ValidationTime {
    Now,
    /// A verified RFC 3161 token's `genTime`, in epoch milliseconds.
    Timestamp {
        gen_time: f64,
    },
}

impl ValidationTime {
    pub fn to_json(&self) -> Value {
        match self {
            ValidationTime::Now => json!({ "kind": "now" }),
            ValidationTime::Timestamp { gen_time } => {
                json!({ "kind": "timestamp", "genTime": { "$date": gen_time }, "tokenVerified": true })
            }
        }
    }
}

/// `TrustStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustStatus {
    Trusted,
    Untrusted,
    Unknown,
}

impl TrustStatus {
    pub fn name(self) -> &'static str {
        match self {
            TrustStatus::Trusted => "trusted",
            TrustStatus::Untrusted => "untrusted",
            TrustStatus::Unknown => "unknown",
        }
    }
}

/// `TrustVerdict`.
#[derive(Clone, Debug, PartialEq)]
pub struct TrustVerdict {
    pub status: TrustStatus,
    pub reason: Option<String>,
    pub path: Option<Vec<Vec<u8>>>,
}

impl TrustVerdict {
    fn untrusted(reason: impl Into<String>) -> TrustVerdict {
        TrustVerdict { status: TrustStatus::Untrusted, reason: Some(reason.into()), path: None }
    }

    pub fn to_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        out.insert("status".into(), json!(self.status.name()));
        if let Some(reason) = &self.reason {
            out.insert("reason".into(), json!(reason));
        }
        if let Some(path) = &self.path {
            out.insert("path".into(), path.iter().map(|c| crate::parse::bytes_json(c)).collect());
        }
        Value::Object(out)
    }
}

/// A CMS as `validateChain` takes it: bytes, or an earlier parse.
#[derive(Clone)]
pub enum CmsInput {
    Bytes(Vec<u8>),
    Parsed(Rc<Internal>),
}

/// `validateChain`.
pub async fn validate_chain(
    cms: CmsInput,
    trust: Option<Rc<dyn TrustPort>>,
    at: ValidationTime,
) -> Result<TrustVerdict, Error> {
    let unknown = || TrustVerdict {
        status: TrustStatus::Unknown,
        reason: Some("no trust anchors configured".into()),
        path: None,
    };
    let Some(trust) = trust else { return Ok(unknown()) };
    let anchors = trust.anchors().await?;
    if anchors.is_empty() {
        return Ok(unknown());
    }
    let internal = match cms {
        CmsInput::Parsed(internal) => internal,
        CmsInput::Bytes(bytes) => match parse_cms_internal(&bytes) {
            Ok(internal) => Rc::new(internal),
            Err(e) => return Ok(TrustVerdict::untrusted(e.message())),
        },
    };
    let trusted =
        anchors.iter().map(|der| Certificate::from_ber(der)).collect::<Result<Vec<_>, _>>()?;
    let check_date = match at {
        ValidationTime::Now => epdf_kernel::rt::now(),
        ValidationTime::Timestamp { gen_time } => gen_time,
    };
    let result = ChainEngine::new(trusted, internal.certificates.clone(), check_date).verify();
    Ok(match result {
        Err(message) => TrustVerdict::untrusted(message),
        Ok(path) => TrustVerdict {
            status: TrustStatus::Trusted,
            reason: None,
            path: Some(path.iter().map(|c| c.raw.clone()).collect()),
        },
    })
}

// ── the chain engine ─────────────────────────────────────────────────────────

/// Certificates are identified by their position in `trusted ++ certs`, as
/// pkijs compares them by object identity.
struct ChainEngine {
    pool: Vec<Certificate>,
    trusted: Vec<usize>,
    certs: Vec<usize>,
    check_date: f64,
}

const NO_VALID_PATH: &str = "No valid certificate paths found";

impl ChainEngine {
    fn new(trusted: Vec<Certificate>, certs: Vec<Certificate>, check_date: f64) -> ChainEngine {
        let t = trusted.len();
        let c = certs.len();
        let mut pool = trusted;
        pool.extend(certs);
        ChainEngine { pool, trusted: (0..t).collect(), certs: (t..t + c).collect(), check_date }
    }

    fn tbs_equal(&self, a: usize, b: usize) -> bool {
        self.pool[a].tbs == self.pool[b].tbs
    }

    fn is_trusted(&self, id: usize) -> bool {
        self.trusted.iter().any(|&t| self.tbs_equal(id, t))
    }

    /// `certificate.verify(issuer?)`.
    fn verify_signature(&self, subject: usize, issuer: Option<usize>) -> Result<bool, String> {
        let cert = &self.pool[subject];
        let spki = match issuer {
            Some(i) => &self.pool[i].spki,
            None if cert.issuer.is_equal(&cert.subject) => &cert.spki,
            None => return Err("Please provide issuer certificate as a parameter".into()),
        };
        crypto::verify_with_public_key(
            &cert.tbs,
            &cert.signature,
            spki,
            &cert.signature_algorithm,
            None,
        )
    }

    /// `defaultFindIssuer`.
    fn find_issuer(&self, id: usize) -> Vec<usize> {
        let cert = &self.pool[id];
        if cert.subject.is_equal(&cert.issuer) && self.verify_signature(id, None) == Ok(true) {
            return vec![id];
        }
        let aki = cert.authority_key_identifier();
        let key_identifier = aki.as_ref().and_then(|a| a.key_identifier.clone());
        // An identifier without a key id matches on `authorityCertIssuer`, a
        // GeneralNames array that no name equals, so it matches nothing. Its
        // serial number only ever counts alongside that issuer.
        let authority_issuer =
            aki.as_ref().is_some_and(|a| a.key_identifier.is_none() && a.authority_cert_issuer);
        let mut result = Vec::new();
        let candidates: Vec<usize> = self.trusted.iter().chain(&self.certs).copied().collect();
        for possible in candidates {
            let issuer = &self.pool[possible];
            if let Some(key_identifier) = &key_identifier
                && let Some(extensions) = &issuer.extensions
            {
                let ski = extensions
                    .iter()
                    .find(|e| e.oid == oids::SUBJECT_KEY_IDENTIFIER && e.parsed().is_some());
                if let Some(ski) = ski {
                    if ski.parsed().map(|n| n.octets()).as_ref() == Some(key_identifier) {
                        result.push(possible);
                    }
                    continue;
                }
            }
            if authority_issuer {
                continue;
            }
            if cert.issuer.is_equal(&issuer.subject) {
                result.push(possible);
            }
        }
        let mut i = result.len();
        while i > 0 {
            i -= 1;
            if self.verify_signature(id, Some(result[i])) != Ok(true) {
                result.remove(i);
            }
        }
        result
    }

    fn build_path(&self, id: usize) -> Result<Vec<Vec<usize>>, String> {
        if self.is_trusted(id) {
            return Ok(vec![vec![id]]);
        }
        let issuers = self.find_issuer(id);
        if issuers.is_empty() {
            return Err(NO_VALID_PATH.into());
        }
        let mut result = Vec::new();
        for issuer in issuers {
            if self.tbs_equal(issuer, id) {
                result.push(vec![issuer]);
                continue;
            }
            for path in self.build_path(issuer)? {
                let mut copy = vec![issuer];
                copy.extend(&path);
                let unique =
                    copy.iter().enumerate().all(|(i, a)| copy.iter().skip(i + 1).all(|b| a != b));
                result.push(if unique { copy } else { path });
            }
        }
        Ok(result)
    }

    /// `checkForCA`: `Err` with pkijs's reason.
    fn check_for_ca(&self, id: usize) -> Result<(), String> {
        let cert = &self.pool[id];
        let mut is_ca = false;
        let mut must_be_ca = false;
        let mut key_usage = false;
        if let Some(extensions) = &cert.extensions {
            for extension in extensions {
                let parsed = extension.parsed();
                if extension.critical && parsed.is_none() {
                    return Err(format!(
                        "Unable to parse critical certificate extension: {}",
                        extension.oid
                    ));
                }
                if extension.oid == oids::KEY_USAGE {
                    key_usage = true;
                    let first = parsed
                        .clone()
                        .map(|n| n.bits())
                        .and_then(|b| b.first().copied())
                        .unwrap_or(0);
                    if first & 0x04 != 0 {
                        must_be_ca = true;
                    }
                }
                if extension.oid == oids::BASIC_CONSTRAINTS
                    && parsed.and_then(|n| n.children.first().cloned()).is_some_and(|b| {
                        b.is_universal(1) && b.content.first().is_some_and(|&v| v != 0)
                    })
                {
                    is_ca = true;
                }
            }
            if must_be_ca && !is_ca {
                return Err("keyCertSign without BasicConstraints".into());
            }
            if key_usage && is_ca && !must_be_ca {
                return Err("keyCertSign not set".into());
            }
        }
        if !is_ca {
            return Err("more than one possible end-user certificate".into());
        }
        Ok(())
    }

    fn basic_check(&self, path: &[usize]) -> Result<(), String> {
        for &id in path {
            let cert = &self.pool[id];
            if cert.not_before > self.check_date || cert.not_after < self.check_date {
                return Err("The certificate is either not yet valid or expired".into());
            }
        }
        if path.len() < 2 {
            return Err("Too short certificate path".into());
        }
        for i in (0..path.len() - 1).rev() {
            let cert = &self.pool[path[i]];
            if !cert.issuer.is_equal(&cert.subject)
                && !cert.issuer.is_equal(&self.pool[path[i + 1]].subject)
            {
                return Err("Incorrect name chaining".into());
            }
        }
        for &id in &path[1..] {
            if self.check_for_ca(id).is_err() {
                return Err("One of intermediate certificates is not a CA certificate".into());
            }
        }
        Ok(())
    }

    /// `sort()`: the path, or the message the engine reports.
    fn sort(&self) -> Result<Vec<usize>, String> {
        let mut local: Vec<usize> = self.trusted.iter().chain(&self.certs).copied().collect();
        // pkijs's de-duplication, restart quirk and all: after a removal the
        // outer loop resumes at index 1.
        let mut i = 0;
        while i < local.len() {
            let mut j = 0;
            while j < local.len() {
                if i != j && self.tbs_equal(local[i], local[j]) {
                    local.remove(j);
                    i = 0;
                    break;
                }
                j += 1;
            }
            i += 1;
        }
        let leaf = *local.last().ok_or("Empty certificate array")?;
        let mut result = self.build_path(leaf)?;
        if result.is_empty() {
            return Err("Unable to find certificate path".into());
        }
        result.retain(|path| path.iter().any(|&id| self.is_trusted(id)));
        if result.is_empty() {
            return Err(NO_VALID_PATH.into());
        }
        let mut shortest = 0;
        for (index, path) in result.iter().enumerate() {
            if path.len() < result[shortest].len() {
                shortest = index;
            }
        }
        let mut path = vec![leaf];
        path.extend(&result[shortest]);
        self.basic_check(&path)?;
        Ok(path)
    }

    /// `verify()`: the certificate path, leaf first, or the reason.
    fn verify(&self) -> Result<Vec<Certificate>, String> {
        if self.certs.is_empty() {
            return Err("Empty certificate array".into());
        }
        let path = self.sort()?;
        crate::policy::check(&path.iter().map(|&id| &self.pool[id]).collect::<Vec<_>>())?;
        Ok(path.iter().map(|&id| self.pool[id].clone()).collect())
    }
}
