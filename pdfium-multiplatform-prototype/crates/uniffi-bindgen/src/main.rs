//! The local bindgen binary uniffi 0.32 wants, so the generator version can
//! never drift from the runtime version: both come from the one `=0.32.0` pin.
fn main() {
    uniffi::uniffi_bindgen_main()
}
