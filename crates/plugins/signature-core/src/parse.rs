//! `cms/parse.ts`: a detached CMS SignedData with exactly one signer.

use epdf_kernel::Error;
use serde_json::{Value, json};

use crate::cert::{AlgId, Certificate, Name};
use crate::der::{self, Node};
use crate::oids::{self, Digest};

/// What a detached CMS says about itself. Nothing here is verified.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedCms {
    pub digest_algorithm: Digest,
    pub message_digest: Vec<u8>,
    /// Epoch milliseconds.
    pub signing_time: Option<f64>,
    pub signer_certificate: Vec<u8>,
    pub certificates: Vec<Vec<u8>>,
    pub signature_algorithm: String,
    pub signing_certificate_v2: bool,
    pub timestamp_token: Option<Vec<u8>>,
}

pub fn bytes_json(bytes: &[u8]) -> Value {
    epdf_kernel::bytes::Bytes(bytes.to_vec()).to_json()
}

impl ParsedCms {
    pub fn to_json(&self) -> Value {
        json!({
            "digestAlgorithm": self.digest_algorithm.name(),
            "messageDigest": bytes_json(&self.message_digest),
            "signingTime": self.signing_time.map_or(Value::Null, |ms| json!({ "$date": ms })),
            "signerCertificate": bytes_json(&self.signer_certificate),
            "certificates": self.certificates.iter().map(|c| bytes_json(c)).collect::<Vec<_>>(),
            "signatureAlgorithm": self.signature_algorithm,
            "signingCertificateV2": self.signing_certificate_v2,
            "timestampToken": self.timestamp_token.as_deref().map_or(Value::Null, bytes_json),
        })
    }
}

/// A parse failure: `CmsError` with its reason, or whatever else the
/// TypeScript would have thrown (a `TypeError` reading an empty attribute).
#[derive(Clone, Debug, PartialEq)]
pub enum ParseError {
    Cms { reason: &'static str, message: String },
    Other(Error),
}

impl ParseError {
    fn malformed(message: impl Into<String>) -> ParseError {
        ParseError::Cms { reason: "malformed", message: message.into() }
    }

    fn unsupported(message: impl Into<String>) -> ParseError {
        ParseError::Cms { reason: "unsupported", message: message.into() }
    }

    fn undefined_index() -> ParseError {
        ParseError::Other(Error::named(
            "TypeError",
            "Cannot read properties of undefined (reading '0')",
        ))
    }

    pub fn message(&self) -> &str {
        match self {
            ParseError::Cms { message, .. } => message,
            ParseError::Other(e) => &e.message,
        }
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            ParseError::Cms { reason, .. } => Some(reason),
            ParseError::Other(_) => None,
        }
    }

    pub fn to_error(&self) -> Error {
        match self {
            ParseError::Cms { reason, message } => {
                Error::named("CmsError", message.clone()).with("reason", *reason)
            }
            ParseError::Other(e) => e.clone(),
        }
    }

    /// `String(err)` for a non-CmsError, the message for one.
    pub fn text(&self) -> String {
        match self {
            ParseError::Cms { message, .. } => message.clone(),
            ParseError::Other(e) => format!("{}: {}", e.name, e.message),
        }
    }
}

/// A CMS `Attribute`: its values are absent (not empty) when the SET is
/// empty, as pkijs leaves them.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    pub oid: String,
    pub values: Option<Vec<Node>>,
}

impl Attribute {
    fn read(node: &Node) -> Option<Attribute> {
        if !node.is_sequence() || node.children.len() < 2 || !node.children[1].is_set() {
            return None;
        }
        let values = &node.children[1].children;
        Some(Attribute {
            oid: node.children[0].oid()?,
            values: (!values.is_empty()).then(|| values.clone()),
        })
    }

    /// `attribute.values[0]`, or the TypeError reading it throws.
    fn first(&self) -> Result<Option<&Node>, ParseError> {
        match &self.values {
            Some(values) => Ok(values.first()),
            None => Err(ParseError::undefined_index()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Sid {
    IssuerAndSerial {
        issuer: Name,
        serial: Vec<u8>,
    },
    /// `[0]` primitive: the subject key identifier. `None` for the `[0]`
    /// constructed form, which carries no `valueHexView`.
    KeyId(Option<Vec<u8>>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignerInfo {
    pub sid: Sid,
    pub digest_algorithm: AlgId,
    pub signed_attrs: Option<Vec<Attribute>>,
    /// The content octets of `[0] IMPLICIT SignedAttributes`.
    pub signed_attrs_content: Vec<u8>,
    pub signature_algorithm: AlgId,
    pub signature: Vec<u8>,
    pub unsigned_attrs: Option<Vec<Attribute>>,
}

impl SignerInfo {
    fn read(node: &Node) -> Option<SignerInfo> {
        if !node.is_sequence() {
            return None;
        }
        let mut members = node.children.iter().peekable();
        members.next().filter(|n| n.is_universal(der::INTEGER))?;
        let sid_node = members.next()?;
        let sid = if sid_node.is_sequence() {
            if sid_node.children.len() < 2 || !sid_node.children[1].is_universal(der::INTEGER) {
                return None;
            }
            Sid::IssuerAndSerial {
                issuer: Name::read(&sid_node.children[0])?,
                serial: sid_node.children[1].content.clone(),
            }
        } else if sid_node.is_context(0) {
            if sid_node.constructed {
                sid_node.children.first().filter(|n| n.is_octet_string())?;
                Sid::KeyId(None)
            } else {
                Sid::KeyId(Some(sid_node.content.clone()))
            }
        } else {
            return None;
        };
        let digest_algorithm = AlgId::read(members.next()?)?;
        let attributes =
            |node: &Node| node.children.iter().map(Attribute::read).collect::<Option<Vec<_>>>();
        let mut signed_attrs = None;
        let mut signed_attrs_content = Vec::new();
        if members.peek().is_some_and(|n| n.is_context(0) && n.constructed) {
            let node = members.next()?;
            signed_attrs = Some(attributes(node)?);
            signed_attrs_content = node.children.iter().flat_map(|c| c.raw.clone()).collect();
        }
        let signature_algorithm = AlgId::read(members.next()?)?;
        let signature = members.next().filter(|n| n.is_octet_string())?.octets();
        let mut unsigned_attrs = None;
        if let Some(node) = members.next() {
            if !(node.is_context(1) && node.constructed) {
                return None;
            }
            unsigned_attrs = Some(attributes(node)?);
        }
        Some(SignerInfo {
            sid,
            digest_algorithm,
            signed_attrs,
            signed_attrs_content,
            signature_algorithm,
            signature,
            unsigned_attrs,
        })
    }

    /// `signedAttributesDer`: the attributes re-tagged as the SET they were
    /// signed as.
    pub fn signed_attributes_der(&self) -> Vec<u8> {
        der::tlv(0x31, &self.signed_attrs_content)
    }
}

struct SignedData {
    e_content_type: String,
    has_econtent: bool,
    /// The element inside `[0]`, when it carries one.
    e_content: Option<Node>,
    certificates: Vec<Certificate>,
    signer_infos: Vec<SignerInfo>,
}

impl SignedData {
    fn read(node: &Node) -> Option<SignedData> {
        if !node.is_sequence() {
            return None;
        }
        let mut members = node.children.iter().peekable();
        members.next().filter(|n| n.is_universal(der::INTEGER))?;
        let digests = members.next().filter(|n| n.is_set())?;
        for d in &digests.children {
            AlgId::read(d)?;
        }
        let encap = members.next().filter(|n| n.is_sequence())?;
        let e_content_type = encap.children.first()?.oid()?;
        let (has_econtent, e_content) = match encap.children.get(1) {
            Some(n) if n.is_context(0) && n.constructed => (true, n.children.first().cloned()),
            Some(_) => return None,
            None => (false, None),
        };
        let mut certificates = Vec::new();
        if members.peek().is_some_and(|n| n.is_context(0) && n.constructed) {
            for cert in &members.next()?.children {
                if cert.is_sequence() {
                    certificates.push(Certificate::read(cert)?);
                } else if !(cert.class == der::CONTEXT && cert.constructed && cert.tag <= 3) {
                    return None;
                }
            }
        }
        if members.peek().is_some_and(|n| n.is_context(1) && n.constructed) {
            members.next();
        }
        let infos = members.next().filter(|n| n.is_set())?;
        let signer_infos =
            infos.children.iter().map(SignerInfo::read).collect::<Option<Vec<_>>>()?;
        Some(SignedData { e_content_type, has_econtent, e_content, certificates, signer_infos })
    }
}

/// What the rest of the package reads from a parse.
#[derive(Clone, Debug, PartialEq)]
pub struct Internal {
    pub parsed: ParsedCms,
    pub signer_info: SignerInfo,
    pub signer_certificate: Certificate,
    /// Signer first.
    pub certificates: Vec<Certificate>,
    pub ess_cert_hash: Option<Vec<u8>>,
    /// A timestamp token's encapsulated TSTInfo (`CmsContent::TstInfo`);
    /// `None` for a detached CMS.
    pub e_content: Option<Vec<u8>>,
}

/// `CmsContent`: what the CMS carries. Nothing (a detached PDF signature
/// over the `/ByteRange`), or an RFC 3161 TSTInfo (a document timestamp's
/// token, whose message imprint is the `/ByteRange` digest).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CmsContent {
    #[default]
    Detached,
    TstInfo,
}

/// `parseDetachedCms`.
pub fn parse_detached_cms(cms: &[u8]) -> Result<ParsedCms, ParseError> {
    parse_cms_internal(cms).map(|i| i.parsed)
}

/// `parseCmsInternal(cms)`: a detached CMS.
pub fn parse_cms_internal(cms: &[u8]) -> Result<Internal, ParseError> {
    parse_cms_content(cms, CmsContent::Detached)
}

/// `parseCmsInternal(cms, content)`.
pub fn parse_cms_content(cms: &[u8], content_kind: CmsContent) -> Result<Internal, ParseError> {
    if cms.is_empty() {
        return Err(ParseError::malformed("empty CMS"));
    }
    let root = der::from_ber(cms).ok_or_else(|| ParseError::malformed("CMS is not valid BER"))?;
    let wrap = |target: &str| {
        ParseError::malformed(format!(
            "CMS does not parse as SignedData: {}",
            der::schema_error(target).message
        ))
    };
    let content_type = root.children.first().and_then(Node::oid);
    let content = root
        .children
        .get(1)
        .filter(|n| n.is_context(0) && n.constructed)
        .and_then(|n| n.children.first());
    let (Some(content_type), Some(content), true) = (content_type, content, root.is_sequence())
    else {
        return Err(wrap("ContentInfo"));
    };
    if content_type != oids::SIGNED_DATA {
        return Err(ParseError::unsupported(format!(
            "CMS content type {content_type} is not SignedData"
        )));
    }
    let signed_data = SignedData::read(content).ok_or_else(|| wrap("SignedData"))?;
    let (expected_content_type, expected_name) = match content_kind {
        CmsContent::Detached => (oids::DATA, "id-data"),
        CmsContent::TstInfo => (oids::TST_INFO, "id-ct-TSTInfo"),
    };
    let mut e_content = None;
    match content_kind {
        CmsContent::Detached => {
            if signed_data.has_econtent {
                return Err(ParseError::unsupported(
                    "CMS carries encapsulated content; a PDF signature is detached",
                ));
            }
        }
        CmsContent::TstInfo => {
            let Some(node) = signed_data
                .e_content
                .as_ref()
                .filter(|_| signed_data.e_content_type == oids::TST_INFO)
            else {
                return Err(ParseError::malformed("a timestamp token must carry a TSTInfo"));
            };
            // pkijs keeps an OCTET STRING; any other element has no
            // `getValue`. asn1js's `getValue()` joins a constructed one's
            // primitive pieces only: a nested constructed piece adds nothing.
            if !node.is_octet_string() {
                return Err(ParseError::Other(Error::named(
                    "TypeError",
                    "encapsulated.eContent.getValue is not a function",
                )));
            }
            e_content = Some(if node.constructed {
                node.children
                    .iter()
                    .filter(|c| !c.constructed)
                    .flat_map(|c| c.content.clone())
                    .collect()
            } else {
                node.content.clone()
            });
        }
    }
    if signed_data.signer_infos.len() != 1 {
        return Err(ParseError::unsupported(format!(
            "CMS has {} signers; exactly one is expected",
            signed_data.signer_infos.len()
        )));
    }
    let signer_info = signed_data.signer_infos.into_iter().next().unwrap();
    let digest_algorithm =
        Digest::from_oid(&signer_info.digest_algorithm.oid).ok_or_else(|| {
            ParseError::unsupported(format!(
                "digest algorithm {}",
                signer_info.digest_algorithm.oid
            ))
        })?;
    let certificates = signed_data.certificates;
    let signer_index = find_signer_certificate(&signer_info.sid, &certificates)
        .ok_or_else(|| ParseError::malformed("the signer certificate is not carried by the CMS"))?;

    let empty = Vec::new();
    let signed_attrs = signer_info.signed_attrs.as_ref().unwrap_or(&empty);
    if signed_attrs.is_empty() {
        return Err(ParseError::unsupported(
            "CMS has no signed attributes; a PDF signature needs message-digest",
        ));
    }
    let attr = |oid: &str| signed_attrs.iter().find(|a| a.oid == oid);

    if let Some(content_type) = attr(oids::CONTENT_TYPE) {
        let first = content_type.first()?;
        if first.and_then(Node::oid).as_deref() != Some(expected_content_type) {
            return Err(ParseError::unsupported(format!(
                "CMS content-type attribute is not {expected_name}"
            )));
        }
    }
    let message_digest = match attr(oids::MESSAGE_DIGEST) {
        Some(a) => a.first()?.filter(|n| n.is_octet_string()).map(Node::octets),
        None => None,
    }
    .ok_or_else(|| ParseError::malformed("CMS has no message-digest attribute"))?;

    let signing_time = match attr(oids::SIGNING_TIME) {
        Some(a) => a.first()?.and_then(Node::time),
        None => None,
    };

    let ess = attr(oids::SIGNING_CERTIFICATE_V2);
    let mut ess_cert_hash = None;
    if let Some(ess) = ess {
        let seq = ess.first()?;
        let children = |n: Option<&Node>| {
            n.filter(|n| n.constructed).map(|n| n.children.clone()).unwrap_or_default()
        };
        let certs = children(seq).into_iter().next();
        let first = children(certs.as_ref()).into_iter().next();
        let members = children(first.as_ref());
        let mut cursor = 0;
        if let Some(head) = members.first()
            && head.is_sequence()
            && head.children.first().is_some_and(|c| c.is_universal(der::OID))
        {
            if head.children[0].oid().as_deref() != Some(oids::SHA256) {
                return Err(ParseError::unsupported(
                    "signing-certificate-v2 uses a hash other than SHA-256",
                ));
            }
            cursor = 1;
        }
        let hash = members
            .get(cursor)
            .filter(|n| n.is_octet_string())
            .ok_or_else(|| ParseError::malformed("signing-certificate-v2 carries no certHash"))?;
        ess_cert_hash = Some(hash.octets());
    }

    let mut timestamp_token = None;
    if let Some(token) = signer_info
        .unsigned_attrs
        .as_ref()
        .and_then(|attrs| attrs.iter().find(|a| a.oid == oids::TIMESTAMP_TOKEN))
        && let Some(value) = token.first()?
    {
        timestamp_token = Some(value.raw.clone());
    }

    let signer_certificate = certificates[signer_index].clone();
    let mut ordered = vec![signer_certificate.clone()];
    ordered.extend(
        certificates.iter().enumerate().filter(|(i, _)| *i != signer_index).map(|(_, c)| c.clone()),
    );
    Ok(Internal {
        parsed: ParsedCms {
            digest_algorithm,
            message_digest,
            signing_time,
            signer_certificate: signer_certificate.raw.clone(),
            certificates: ordered.iter().map(|c| c.raw.clone()).collect(),
            signature_algorithm: signer_info.signature_algorithm.oid.clone(),
            signing_certificate_v2: ess.is_some(),
            timestamp_token,
        },
        signer_info,
        signer_certificate,
        certificates: ordered,
        ess_cert_hash,
        e_content,
    })
}

fn find_signer_certificate(sid: &Sid, certificates: &[Certificate]) -> Option<usize> {
    match sid {
        Sid::IssuerAndSerial { issuer, serial } => {
            certificates.iter().position(|c| c.issuer.is_equal(issuer) && &c.serial == serial)
        }
        Sid::KeyId(None) => None,
        Sid::KeyId(Some(ski)) => certificates.iter().position(|cert| {
            let inner = cert
                .extension(oids::SUBJECT_KEY_IDENTIFIER)
                .and_then(|e| der::from_ber(&e.value))
                .map(|n| if n.is_octet_string() { n.octets() } else { Vec::new() });
            inner.is_some_and(|inner| &inner == ski)
        }),
    }
}
