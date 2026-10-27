//! UUIDs share one byte source; hosts may scope deterministic entropy and time.
use std::{cell::RefCell, rc::Rc};
type Bytes = Rc<dyn Fn(&mut [u8; 16])>;
type Now = Rc<dyn Fn() -> f64>;
thread_local! { static SOURCE: RefCell<Option<(Bytes, Now)>> = const { RefCell::new(None) }; }
pub struct UuidSourceGuard(Option<(Bytes, Now)>);
impl Drop for UuidSourceGuard {
    fn drop(&mut self) {
        SOURCE.with(|source| *source.borrow_mut() = self.0.take());
    }
}
/// Install for this runtime thread; nested guards restore the prior provider.
pub fn with_uuid_source(bytes: Bytes, now: Now) -> UuidSourceGuard {
    UuidSourceGuard(SOURCE.with(|source| source.replace(Some((bytes, now)))))
}
fn random_bytes() -> [u8; 16] {
    let mut bytes = [0; 16];
    let source = SOURCE.with(|source| source.borrow().as_ref().map(|(bytes, _)| bytes.clone()));
    if let Some(source) = source {
        source(&mut bytes);
    } else {
        // Retain the kernel's native RandomState-backed entropy source.
        use std::{
            hash::{BuildHasher, Hasher},
            sync::atomic::{AtomicU64, Ordering},
        };
        static IDS: AtomicU64 = AtomicU64::new(0);
        for chunk in bytes.chunks_mut(8) {
            let mut hash = std::collections::hash_map::RandomState::new().build_hasher();
            hash.write_u64(IDS.fetch_add(1, Ordering::Relaxed));
            chunk.copy_from_slice(&hash.finish().to_be_bytes());
        }
    }
    bytes
}
fn format(mut bytes: [u8; 16], version: u8) -> String {
    bytes[6] = (bytes[6] & 15) | version << 4;
    bytes[8] = (bytes[8] & 63) | 128;
    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}
pub fn generate_uuid() -> String {
    format(random_bytes(), 4)
}
pub fn generate_uuid_v7(now: Option<f64>) -> String {
    let now = now.unwrap_or_else(|| {
        let source = SOURCE.with(|source| source.borrow().as_ref().map(|(_, now)| now.clone()));
        source.map_or_else(
            || {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as f64
            },
            |now| now(),
        )
    });
    let mut bytes = random_bytes();
    let mut time = now.floor().max(0.0);
    for i in (0..6).rev() {
        bytes[i] = (time % 256.0) as u8;
        time = (time / 256.0).floor();
    }
    format(bytes, 7)
}
