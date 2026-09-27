//! `blank-page.ts`: a one-page blank PDF, the appearance that clears a
//! visual fill.

use std::sync::OnceLock;

/// A content stream that draws nothing but is not empty (`q Q`): the engine
/// turns the page into a form XObject, and a page with no content at all
/// has nothing to turn.
const CONTENT: &str = "q Q";

fn objects() -> [String; 4] {
    [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 60] /Contents 4 0 R /Resources << >> >>"
            .into(),
        format!("<< /Length {} >>\nstream\n{CONTENT}\nendstream", CONTENT.len()),
    ]
}

/// `blankPagePdf()`: a fresh copy of the cached bytes.
pub fn blank_page_pdf() -> Vec<u8> {
    static CACHED: OnceLock<Vec<u8>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let objects = objects();
            let mut body = String::from("%PDF-1.7\n");
            let mut offsets = Vec::new();
            for (index, object) in objects.iter().enumerate() {
                offsets.push(body.len());
                body += &format!("{} 0 obj\n{object}\nendobj\n", index + 1);
            }
            let xref = body.len();
            body += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
            for offset in offsets {
                body += &format!("{offset:010} 00000 n \n");
            }
            body += &format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            );
            body.into_bytes()
        })
        .clone()
}
