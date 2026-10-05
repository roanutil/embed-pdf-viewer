//! How a C caller reaches a viewer's kernel thread without uniffi: a token
//! per `ViewerHost`, registered by the host when it is built with
//! `manual-bridge` and unregistered at shutdown. The host crate depends on
//! this one (it links these exports into its staticlib), so the registry
//! lives here and the host fills it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use epdf_ffi::KernelPost;
use epdf_kernel::Kernel;

/// A job for the kernel thread that needs the kernel itself.
pub type KernelJob = Box<dyn FnOnce(&Kernel) + Send>;

pub struct Registered {
    /// Posts a job with no access to the kernel (the hop alone).
    pub post: KernelPost,
    /// Posts a job that gets the host's kernel. A host that has shut down
    /// drops the job unrun.
    pub run: Box<dyn Fn(KernelJob) + Send + Sync>,
    /// The bytes behind a raster's `$host` id, on the kernel thread: the
    /// host's table, which the render plugin cannot name.
    pub pixels: fn(u64) -> Option<Arc<Vec<u8>>>,
}

static NEXT: AtomicU64 = AtomicU64::new(1);

fn table() -> &'static Mutex<HashMap<u64, Arc<Registered>>> {
    static TABLE: OnceLock<Mutex<HashMap<u64, Arc<Registered>>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

pub fn register(entry: Registered) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    table().lock().unwrap().insert(id, Arc::new(entry));
    id
}

pub fn lookup(id: u64) -> Option<Arc<Registered>> {
    table().lock().unwrap().get(&id).cloned()
}

pub fn unregister(id: u64) {
    table().lock().unwrap().remove(&id);
}
