//! Keeping a document's XMP packet (the catalog's `/Metadata` stream) in
//! step with its `/Info`, so apps that read the packet see the same
//! metadata.
//!
//! Ports `services/src/features/metadata/internal/xmp/` (`xmpPacket.ts`,
//! `syncXmp.ts`, `xmpFields.ts`, `editXmp.ts`) and `shared/xml.ts`. A field a
//! patch sets is written to its XMP property (`dc:title`, `dc:creator`,
//! `dc:description`, `pdf:Keywords`, `xmp:CreatorTool`, `pdf:Producer`,
//! `xmp:CreateDate`, `xmp:ModifyDate`, `pdf:Trapped`), added when missing as
//! PDF/A asks; a clear removes it everywhere. An existing `dc:subject` list
//! follows the keywords. Custom keys change or remove the `pdfx:` property
//! Adobe apps copy them to, and never add one. A document without XMP gets
//! none; XMP that isn't well-formed is left as is.

pub mod edit;
pub mod xml;

use pdfium::Document;

use crate::clock::{format_pdf_date, pdf_date_to_iso};
use crate::dto::{CustomMetadataPatch, DocumentMetadataTrapped, MetadataPatch, Patch};
use crate::error::{EngineError, EngineErrorCode};
use edit::{XmpEdit, XmpProperty, XmpValue, edit_xmp};

/// The encodings a packet is read in and goes back in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

/// A document's XMP packet as text, and the encoding it goes back in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmpPacket {
    pub text: String,
    pub encoding: PacketEncoding,
}

/// `readXmpPacket`: the document's packet, or `None` when it has none, or
/// bytes that aren't UTF-8 or UTF-16 text (XMP allows UTF-32 too, which no
/// PDF tool writes). The byte order mark stays in the text, so the packet
/// goes back with it.
pub fn read_xmp_packet(document: &Document) -> Result<Option<XmpPacket>, EngineError> {
    let bytes = document.xmp_metadata().map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading the XMP metadata failed: {e}"),
        )
    })?;
    Ok(bytes.and_then(|bytes| decode_packet(&bytes)))
}

/// The packet's text, read in the encoding its first bytes name.
pub fn decode_packet(bytes: &[u8]) -> Option<XmpPacket> {
    let encoding = encoding_of(bytes)?;
    let text = match encoding {
        PacketEncoding::Utf8 => String::from_utf8(bytes.to_vec()).ok()?,
        PacketEncoding::Utf16Le | PacketEncoding::Utf16Be => {
            let (pairs, rest) = bytes.as_chunks::<2>();
            if !rest.is_empty() {
                return None;
            }
            let units: Vec<u16> = pairs
                .iter()
                .map(|pair| match encoding {
                    PacketEncoding::Utf16Le => u16::from_le_bytes(*pair),
                    _ => u16::from_be_bytes(*pair),
                })
                .collect();
            String::from_utf16(&units).ok()?
        }
    };
    Some(XmpPacket { text, encoding })
}

/// `writeXmpPacket`'s bytes: the text in the packet's encoding.
pub fn encode_packet(packet: &XmpPacket) -> Vec<u8> {
    match packet.encoding {
        PacketEncoding::Utf8 => packet.text.as_bytes().to_vec(),
        PacketEncoding::Utf16Le => packet.text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        PacketEncoding::Utf16Be => packet.text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
    }
}

/// The encoding XMP's rules let a reader tell from the first bytes: a byte
/// order mark, or the zero bytes around the packet's first `<`
/// (`encodingOf`). A byte past the end reads as 1, as upstream's defaults.
fn encoding_of(bytes: &[u8]) -> Option<PacketEncoding> {
    let at = |i: usize| bytes.get(i).copied().unwrap_or(1);
    let (b0, b1, b2, b3) = (at(0), at(1), at(2), at(3));
    if b0 == 0xfe && b1 == 0xff {
        return Some(PacketEncoding::Utf16Be);
    }
    if b0 == 0xff && b1 == 0xfe {
        return (!(b2 == 0 && b3 == 0)).then_some(PacketEncoding::Utf16Le);
    }
    if b0 == 0 && b1 == 0 {
        return None;
    }
    if b0 == 0 {
        return Some(PacketEncoding::Utf16Be);
    }
    if b1 == 0 {
        return (b2 != 0).then_some(PacketEncoding::Utf16Le);
    }
    Some(PacketEncoding::Utf8)
}

/// `syncXmp`: makes `edits` to the document's packet. A document without a
/// packet is left without one (apps read `/Info` then), and a packet that
/// can't be read is left as it is. A packet whose text doesn't change isn't
/// written. `Unknown` if the native write fails.
pub fn sync_xmp(document: &Document, edits: &[XmpEdit]) -> Result<(), EngineError> {
    if edits.is_empty() {
        return Ok(());
    }
    let Some(packet) = read_xmp_packet(document)? else {
        return Ok(());
    };
    let Some(text) = edit_xmp(&packet.text, edits)? else {
        return Ok(());
    };
    if text == packet.text {
        return Ok(());
    }
    let bytes = encode_packet(&XmpPacket { text, encoding: packet.encoding });
    if !document.set_xmp_metadata(Some(&bytes)) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "failed to write the XMP metadata",
        ));
    }
    Ok(())
}

const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";
/// Where Adobe apps copy the custom keys of a document's `/Info`.
const PDFX: &str = "http://ns.adobe.com/pdfx/1.3/";

fn property(ns: &'static str, prefix: &'static str, name: &str) -> XmpProperty {
    XmpProperty { ns, prefix, name: name.to_owned() }
}

/// The form a standard field's value takes in XMP.
enum Kind {
    Text,
    Alt,
    Seq,
}

/// `xmpEditsForMetadata`: the edits that keep a packet in step with a
/// metadata patch. A field the patch sets is written to its property, which
/// is added when the packet doesn't have it (PDF/A asks the two to match); a
/// field it clears is removed. Fields it leaves out stay as they are. A
/// keyword list the packet has follows the keywords, split at commas and
/// semicolons.
pub fn xmp_edits_for_metadata(patch: &MetadataPatch) -> Result<Vec<XmpEdit>, EngineError> {
    let mut edits = Vec::new();
    let mut edit = |property: XmpProperty, value: Option<XmpValue>, add: bool| {
        edits.push(XmpEdit { property, value, add });
    };
    // As PDF/A pairs them with /Info.
    let fields = [
        (&patch.title, property(DC, "dc", "title"), Kind::Alt),
        (&patch.author, property(DC, "dc", "creator"), Kind::Seq),
        (&patch.subject, property(DC, "dc", "description"), Kind::Alt),
        (&patch.keywords, property(PDF, "pdf", "Keywords"), Kind::Text),
        (&patch.creator, property(XMP, "xmp", "CreatorTool"), Kind::Text),
        (&patch.producer, property(PDF, "pdf", "Producer"), Kind::Text),
    ];
    for (value, property, kind) in fields {
        match value {
            Patch::Keep => {}
            Patch::Clear => edit(property, None, true),
            Patch::Set(text) => edit(
                property,
                Some(match kind {
                    Kind::Text => XmpValue::Text(text.clone()),
                    Kind::Alt => XmpValue::Alt(text.clone()),
                    Kind::Seq => XmpValue::Seq(vec![text.clone()]),
                }),
                true,
            ),
        }
    }
    // Adobe apps also list the keywords one by one here.
    match &patch.keywords {
        Patch::Keep => {}
        Patch::Clear => edit(property(DC, "dc", "subject"), None, false),
        Patch::Set(keywords) => edit(
            property(DC, "dc", "subject"),
            Some(XmpValue::Bag(split_keywords(keywords))),
            false,
        ),
    }
    for (value, name) in [(&patch.created_at, "CreateDate"), (&patch.modified_at, "ModifyDate")] {
        match value {
            Patch::Keep => {}
            Patch::Clear => edit(property(XMP, "xmp", name), None, true),
            Patch::Set(date) => {
                edit(property(XMP, "xmp", name), Some(date_value(date.as_str())?), true)
            }
        }
    }
    if let Some(trapped) = patch.trapped {
        let text = match trapped {
            DocumentMetadataTrapped::True => "True",
            DocumentMetadataTrapped::False => "False",
            DocumentMetadataTrapped::Unknown => "Unknown",
        };
        edit(property(PDF, "pdf", "Trapped"), Some(XmpValue::Text(text.to_owned())), true);
    }
    Ok(edits)
}

/// `xmpEditsForCustomMetadata`: a key's `pdfx:` property follows it when
/// the packet has one, and goes when the key does. None is added: a
/// namespace PDF/A doesn't know needs a schema description in the packet. A
/// key that can't be an XML name can't be in the packet.
pub fn xmp_edits_for_custom_metadata(patch: &CustomMetadataPatch) -> Vec<XmpEdit> {
    patch
        .0
        .iter()
        .filter(|(key, _)| xml_name_like(key))
        .map(|(key, value)| XmpEdit {
            property: property(PDFX, "pdfx", key),
            value: value.as_ref().map(|text| XmpValue::Text(text.clone())),
            add: false,
        })
        .collect()
}

/// `/^[A-Za-z_][A-Za-z0-9._-]*$/`.
fn xml_name_like(key: &str) -> bool {
    let mut bytes = key.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn split_keywords(keywords: &str) -> Vec<String> {
    keywords
        .split([',', ';'])
        .map(|keyword| keyword.trim_matches(js_trimmed))
        .filter(|keyword| !keyword.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What JavaScript's `trim` takes off: `\s` and line terminators.
fn js_trimmed(c: char) -> bool {
    xml::js_space(c)
}

/// The date as `/Info` gets it (whole seconds, the same offset), written
/// the XMP way.
fn date_value(value: &str) -> Result<XmpValue, EngineError> {
    let pdf_date = format_pdf_date(value)?;
    Ok(XmpValue::Text(pdf_date_to_iso(&pdf_date).unwrap_or(pdf_date)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_encoding_is_told_from_the_first_bytes() {
        assert_eq!(encoding_of(b"<?xp"), Some(PacketEncoding::Utf8));
        assert_eq!(encoding_of(&[0xfe, 0xff, 0, b'<']), Some(PacketEncoding::Utf16Be));
        assert_eq!(encoding_of(&[0xff, 0xfe, b'<', 0]), Some(PacketEncoding::Utf16Le));
        assert_eq!(encoding_of(&[0xff, 0xfe, 0, 0]), None, "UTF-32");
        assert_eq!(encoding_of(&[0, b'<', 0, b'?']), Some(PacketEncoding::Utf16Be));
        assert_eq!(encoding_of(&[b'<', 0, b'?', 0]), Some(PacketEncoding::Utf16Le));
        assert_eq!(encoding_of(&[b'<', 0, 0, 0]), None);
        assert_eq!(encoding_of(&[0, 0, 0, b'<']), None);
    }

    #[test]
    fn a_utf16_packet_goes_back_in_its_encoding_with_its_mark() {
        let bytes: Vec<u8> = "\u{FEFF}<a/>".encode_utf16().flat_map(u16::to_be_bytes).collect();
        let packet = decode_packet(&bytes).unwrap();
        assert_eq!(packet.encoding, PacketEncoding::Utf16Be);
        assert_eq!(packet.text, "\u{FEFF}<a/>");
        assert_eq!(encode_packet(&packet), bytes);
        assert_eq!(decode_packet(&[0xfe, 0xff, 0]), None, "odd length");
        assert_eq!(decode_packet(b"\xff\xfe\x00\xd8"), None, "a lone surrogate");
    }

    #[test]
    fn keywords_split_at_commas_and_semicolons() {
        assert_eq!(split_keywords(" a, b;;c ,"), ["a", "b", "c"]);
        assert!(xml_name_like("Case-Id.v2"));
        assert!(!xml_name_like("2nd"));
        assert!(!xml_name_like("has space"));
        assert!(!xml_name_like(""));
    }
}
