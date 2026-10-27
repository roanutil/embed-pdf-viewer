//! X.509 certificates as `pkijs.Certificate` reads them: enough of the TBS
//! for names, validity, the key, the extensions the chain engine consults,
//! and the signature.

use epdf_kernel::Error;

use crate::der::{self, Node};
use crate::oids;

/// An `AlgorithmIdentifier`.
#[derive(Clone, Debug, PartialEq)]
pub struct AlgId {
    pub oid: String,
    pub params: Option<Node>,
}

impl AlgId {
    pub fn read(node: &Node) -> Option<AlgId> {
        if !node.is_sequence() {
            return None;
        }
        let oid = node.children.first()?.oid()?;
        Some(AlgId { oid, params: node.children.get(1).cloned() })
    }

    /// pkijs's `AlgorithmIdentifier.toSchema()`: the OID and, when present,
    /// the parameters.
    pub fn encode(&self) -> Vec<u8> {
        let mut members = vec![der::oid(&self.oid)];
        if let Some(params) = &self.params {
            members.push(params.raw.clone());
        }
        der::sequence(&members)
    }
}

/// A distinguished name: its attributes flattened across the RDN SETs, as
/// `RelativeDistinguishedNames.typesAndValues` holds them.
#[derive(Clone, Debug, PartialEq)]
pub struct Name {
    pub raw: Vec<u8>,
    pub attributes: Vec<(String, Node)>,
}

impl Name {
    pub fn read(node: &Node) -> Option<Name> {
        if !node.is_sequence() {
            return None;
        }
        let mut attributes = Vec::new();
        for rdn in &node.children {
            if !rdn.is_set() {
                return None;
            }
            for pair in &rdn.children {
                if !pair.is_sequence() || pair.children.len() < 2 {
                    return None;
                }
                attributes.push((pair.children[0].oid()?, pair.children[1].clone()));
            }
        }
        Some(Name { raw: node.raw.clone(), attributes })
    }

    /// `RelativeDistinguishedNames.isEqual`: pairwise, a string compared after
    /// `stringPrep` (trimmed, inner spaces collapsed, lower-cased), anything
    /// else by its encoding.
    pub fn is_equal(&self, other: &Name) -> bool {
        self.attributes.len() == other.attributes.len()
            && self.attributes.iter().zip(&other.attributes).all(|((ta, va), (tb, vb))| {
                if ta != tb || va.is_string() != vb.is_string() {
                    return false;
                }
                match (va.text(), vb.text()) {
                    (Some(a), Some(b)) => string_prep(&a) == string_prep(&b),
                    _ => va.raw == vb.raw,
                }
            })
    }
}

/// pkijs's `stringPrep`.
pub fn string_prep(input: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in input.trim().chars() {
        if c == ' ' {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    out.to_lowercase()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spki {
    pub raw: Vec<u8>,
    pub algorithm: AlgId,
    pub key: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Extension {
    pub oid: String,
    pub critical: bool,
    pub value: Vec<u8>,
}

impl Extension {
    /// `extension.parsedValue`: null only when the value is not BER.
    pub fn parsed(&self) -> Option<Node> {
        der::from_ber(&self.value)
    }
}

/// `AuthorityKeyIdentifier`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Aki {
    pub key_identifier: Option<Vec<u8>>,
    pub authority_cert_issuer: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Certificate {
    pub raw: Vec<u8>,
    pub tbs: Vec<u8>,
    pub serial: Vec<u8>,
    pub issuer: Name,
    pub subject: Name,
    pub not_before: f64,
    pub not_after: f64,
    pub spki: Spki,
    pub extensions: Option<Vec<Extension>>,
    pub signature_algorithm: AlgId,
    pub signature: Vec<u8>,
}

impl Certificate {
    /// A certificate from its decoded element; `None` where pkijs's schema
    /// check would fail.
    pub fn read(node: &Node) -> Option<Certificate> {
        if !node.is_sequence() || node.children.len() < 3 {
            return None;
        }
        let tbs = &node.children[0];
        let signature_algorithm = AlgId::read(&node.children[1])?;
        let signature_value = &node.children[2];
        if !tbs.is_sequence() || !signature_value.is_universal(der::BIT_STRING) {
            return None;
        }
        let mut members = tbs.children.iter().peekable();
        if members.peek().is_some_and(|m| m.is_context(0)) {
            members.next();
        }
        let serial = members.next().filter(|n| n.is_universal(der::INTEGER))?.content.clone();
        AlgId::read(members.next()?)?;
        let issuer = Name::read(members.next()?)?;
        let validity = members.next().filter(|n| n.is_sequence())?;
        let not_before = validity.children.first()?.time()?;
        let not_after = validity.children.get(1)?.time()?;
        let subject = Name::read(members.next()?)?;
        let spki_node = members.next().filter(|n| n.is_sequence())?;
        let spki = Spki {
            raw: spki_node.raw.clone(),
            algorithm: AlgId::read(spki_node.children.first()?)?,
            key: spki_node.children.get(1).filter(|n| n.is_universal(der::BIT_STRING))?.bits(),
        };
        let mut extensions = None;
        for member in members {
            if member.is_context(3) {
                let list = member.children.first().filter(|n| n.is_sequence())?;
                let mut out = Vec::new();
                for ext in &list.children {
                    let oid = ext.children.first()?.oid()?;
                    let (critical, value) = match ext.children.get(1) {
                        Some(b) if b.is_universal(1) => {
                            (b.content.first().is_some_and(|&v| v != 0), ext.children.get(2)?)
                        }
                        Some(v) => (false, v),
                        None => return None,
                    };
                    // pkijs's schema names a primitive OCTET STRING.
                    if !value.is_octet_string() || value.constructed {
                        return None;
                    }
                    out.push(Extension { oid, critical, value: value.content.clone() });
                }
                extensions = Some(out);
            }
        }
        Some(Certificate {
            raw: node.raw.clone(),
            tbs: tbs.raw.clone(),
            serial,
            issuer,
            subject,
            not_before,
            not_after,
            spki,
            extensions,
            signature_algorithm,
            signature: signature_value.bits(),
        })
    }

    /// `pkijs.Certificate.fromBER(der)`, errors and all.
    pub fn from_ber(bytes: &[u8]) -> Result<Certificate, Error> {
        let not_ber = || {
            Error::named(
                "AsnError",
                "Error during parsing of ASN.1 data. Data is not correct for 'Certificate'.",
            )
        };
        match crate::asn1js::from_ber(bytes) {
            crate::asn1js::Verdict::Accepted => {}
            crate::asn1js::Verdict::Refused => return Err(not_ber()),
            crate::asn1js::Verdict::Throws(error) => return Err(error),
        }
        let node = der::from_ber(bytes).ok_or_else(not_ber)?;
        Certificate::read(&node).ok_or_else(|| {
            Error::named("AsnError", "Cannot create 'Certificate' from ASN.1 object")
        })
    }

    pub fn extension(&self, oid: &str) -> Option<&Extension> {
        self.extensions.as_ref()?.iter().find(|e| e.oid == oid)
    }

    /// The subject key identifier's bytes, when the extension parses.
    pub fn subject_key_identifier(&self) -> Option<Vec<u8>> {
        let node = self.extension(oids::SUBJECT_KEY_IDENTIFIER)?.parsed()?;
        Some(node.octets())
    }

    /// `AuthorityKeyIdentifier`, when the extension parses as one.
    pub fn authority_key_identifier(&self) -> Option<Aki> {
        let node = self.extension(oids::AUTHORITY_KEY_IDENTIFIER)?.parsed()?;
        if !node.is_sequence() {
            return None;
        }
        let mut aki = Aki::default();
        for member in &node.children {
            match (member.class, member.tag) {
                (der::CONTEXT, 0) => aki.key_identifier = Some(member.octets()),
                (der::CONTEXT, 1) => aki.authority_cert_issuer = true,
                _ => {}
            }
        }
        Some(aki)
    }
}
