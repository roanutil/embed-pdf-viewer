//! `blank-library.ts`: the seed of a new library, a one-page PDF whose page
//! no `/Names /Pages` key registers, the shape Acrobat gives a fresh stamp
//! library.

const BLANK_OBJECTS: [&str; 3] = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
];

/// `blankLibraryPdf()`: a fresh copy each call.
pub fn blank_library_pdf() -> Vec<u8> {
    let mut body = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (index, object) in BLANK_OBJECTS.iter().enumerate() {
        offsets.push(body.len());
        body.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = body.len();
    body.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", BLANK_OBJECTS.len() + 1));
    for offset in offsets {
        body.push_str(&format!("{offset:010} 00000 n \n"));
    }
    body.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        BLANK_OBJECTS.len() + 1
    ));
    body.into_bytes()
}
