#![allow(dead_code)]
use epdf_capi::*;
use serde_json::{Value, json};
pub const PLAIN: &[u8] =
    include_bytes!("../../../../tooling/engine-conformance/fixtures/annotation_kinds.pdf");
pub const LOCKED: &[u8] =
    include_bytes!("../../../../tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf");
pub struct Engine(pub u64);
impl Engine {
    pub fn new(options: Value) -> Self {
        let bytes = options.to_string();
        let (mut engine, mut error) = (0, 0);
        assert_eq!(
            unsafe {
                epdf_engine_create(
                    bytes.as_ptr(),
                    bytes.len() as u64,
                    std::ptr::null(),
                    0,
                    &mut engine,
                    &mut error,
                )
            },
            0
        );
        assert_eq!(error, 0);
        Self(engine)
    }
    pub fn submit(
        &self,
        document: u64,
        operation: u32,
        args: Value,
        payload: Option<&[u8]>,
    ) -> (u32, u64) {
        let args = args.to_string();
        let mut frame = Vec::new();
        if let Some(payload) = payload {
            frame.extend_from_slice(&1u32.to_le_bytes());
            frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            frame.extend_from_slice(payload);
        }
        let (mut job, mut error) = (0, 0);
        let status = unsafe {
            epdf_engine_submit(
                self.0,
                document,
                operation,
                args.as_ptr(),
                args.len() as u64,
                frame.as_ptr(),
                frame.len() as u64,
                &mut job,
                &mut error,
            )
        };
        if error != 0 {
            assert_eq!(epdf_result_release(error), 0);
        }
        (status, job)
    }
    pub fn call(
        &self,
        document: u64,
        operation: u32,
        args: Value,
        payload: Option<&[u8]>,
    ) -> (Reply, Value, Vec<Vec<u8>>) {
        let (status, job) = self.submit(document, operation, args, payload);
        assert_eq!(status, 0);
        let mut reply = Reply::default();
        assert_eq!(unsafe { epdf_job_wait(job, u32::MAX, &mut reply) }, 0);
        assert_eq!(epdf_job_release(job), 0);
        let (json, payloads) = result(reply.result);
        (reply, json, payloads)
    }
    pub fn open(&self, bytes: &[u8]) -> u64 {
        let (reply, json, _) = self.call(0, 1, json!({}), Some(bytes));
        assert!(json.get("error").is_none(), "{json}");
        assert_ne!(reply.document, 0);
        reply.document
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        epdf_engine_close(self.0, u32::MAX);
        epdf_engine_release(self.0);
    }
}
pub fn result(handle: u64) -> (Value, Vec<Vec<u8>>) {
    let (mut ptr, mut len) = (std::ptr::null(), 0);
    assert_eq!(unsafe { epdf_result_view(handle, &mut ptr, &mut len) }, 0);
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let mut at = 0;
    fn word(bytes: &[u8], at: &mut usize) -> usize {
        let value = u32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap()) as usize;
        *at += 4;
        value
    }
    let len = word(bytes, &mut at);
    let json = serde_json::from_slice(&bytes[at..at + len]).unwrap();
    at += len;
    let count = word(bytes, &mut at);
    let mut payloads = Vec::new();
    for _ in 0..count {
        let len = word(bytes, &mut at);
        payloads.push(bytes[at..at + len].to_vec());
        at += len;
    }
    assert_eq!(at, bytes.len());
    assert_eq!(epdf_result_release(handle), 0);
    (json, payloads)
}

pub fn rotated_crop_pdf() -> Vec<u8> {
    let stream = "1 0 0 rg 0 30 30 30 re f BT /F1 12 Tf 10 65 Td (needle) Tj ET";
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R] /Count 4 >>".to_owned(),
    ];
    for rotation in [0, 90, 180, 270] {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [-20 0 200 200] /CropBox [-10 20 150 100] /Rotate {rotation} /Resources << /Font << /F1 7 0 R >> >> /Contents 8 0 R >>"));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects.push(format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()));
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets.iter().skip(1) {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Root 1 0 R /Size {} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

pub fn translucent_pdf() -> Vec<u8> {
    let stream = "/GS1 gs 1 0 0 rg 10 10 30 30 re f";
    let objects=["<< /Type /Catalog /Pages 2 0 R >>".to_owned(),"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /ExtGState << /GS1 << /Type /ExtGState /ca 0.5 >> >> >> /Contents 4 0 R >>".to_owned(),format!("<< /Length {} >>\nstream\n{stream}\nendstream",stream.len())];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Root 1 0 R /Size {} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}
