//! Two CAs that issue each other. pkijs's `buildPath` recurses on such a
//! chain and never returns, so no vector can pin it; the Rust refuses it
//! (the crate README lists this under its deliberate deviations).

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use epdf_kernel::rt::{self, Promise};
use epdf_signature_core::trust::{
    CmsInput, TrustPort, TrustStatus, ValidationTime, validate_chain,
};

fn fixture(section: &str, name: &str) -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tooling/plugin-conformance/fixtures/signature/fixtures.json"
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    STANDARD.decode(json[section][name].as_str().unwrap()).unwrap()
}

struct Anchors(Vec<Vec<u8>>);

impl TrustPort for Anchors {
    fn anchors(&self) -> Promise<Vec<Vec<u8>>> {
        Promise::resolved(self.0.clone())
    }
}

#[test]
fn issuers_that_issue_each_other_are_refused() {
    let out = Rc::new(RefCell::new(None));
    let slot = out.clone();
    let cms = CmsInput::Bytes(fixture("cms", "cyclicIssuers"));
    let trust: Rc<dyn TrustPort> = Rc::new(Anchors(vec![fixture("certs", "root")]));
    rt::spawn(async move {
        *slot.borrow_mut() = Some(
            validate_chain(cms, Some(trust), ValidationTime::Timestamp { gen_time: 1.7e12 }).await,
        );
    });
    rt::run_until_stalled();
    let verdict = out.borrow_mut().take().expect("validation settled").expect("no error");
    assert_eq!(verdict.status, TrustStatus::Untrusted);
    assert_eq!(verdict.reason.as_deref(), Some("Certificate issuers form a cycle"));
}
