//! `EPDFCatalog_GetMetadata` and `EPDFCatalog_SetMetadata`: the catalog's
//! `/Metadata` stream, the document's XMP packet.

use pdfium::{Document, Library, PdfSaveMode};

fn open(bytes: &[u8]) -> Document {
    Document::from_bytes(Library::acquire(), bytes, None).expect("the PDF opens")
}

/// One empty page; with `packet`, a `/Metadata` stream holding it.
fn pdf(packet: Option<&str>) -> Vec<u8> {
    let mut objects = vec![
        format!(
            "<< /Type /Catalog /Pages 2 0 R {}>>",
            if packet.is_some() { "/Metadata 4 0 R " } else { "" }
        ),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_owned(),
    ];
    if let Some(packet) = packet {
        objects.push(format!(
            "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{packet}\nendstream",
            packet.len()
        ));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

const PACKET: &str = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>";

#[test]
fn a_document_without_a_packet_reads_none() {
    assert_eq!(open(&pdf(None)).xmp_metadata().unwrap(), None);
}

#[test]
fn the_packet_reads_as_the_stream_holds_it() {
    let doc = open(&pdf(Some(PACKET)));
    assert_eq!(doc.xmp_metadata().unwrap().as_deref(), Some(PACKET.as_bytes()));
}

#[test]
fn a_written_packet_reads_back_and_survives_a_save() {
    let doc = open(&pdf(Some(PACKET)));
    let replaced = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">changed</x:xmpmeta>";
    assert!(doc.set_xmp_metadata(Some(replaced.as_bytes())));
    assert_eq!(doc.xmp_metadata().unwrap().as_deref(), Some(replaced.as_bytes()));

    let saved = doc.save_to_buffer(PdfSaveMode::Full).expect("save");
    // Unfiltered, so the packet is in the file as text.
    assert!(saved.windows(replaced.len()).any(|w| w == replaced.as_bytes()));
    assert_eq!(open(&saved).xmp_metadata().unwrap().as_deref(), Some(replaced.as_bytes()));
}

#[test]
fn a_document_without_a_packet_gets_a_new_stream() {
    let doc = open(&pdf(None));
    assert!(doc.set_xmp_metadata(Some(PACKET.as_bytes())));
    assert_eq!(doc.xmp_metadata().unwrap().as_deref(), Some(PACKET.as_bytes()));
}

#[test]
fn none_removes_the_packet() {
    let doc = open(&pdf(Some(PACKET)));
    assert!(doc.set_xmp_metadata(None));
    assert_eq!(doc.xmp_metadata().unwrap(), None);
}
