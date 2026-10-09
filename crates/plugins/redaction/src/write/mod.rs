//! `write/`: marking (annotation authoring) and applying (the destructive
//! engine call).

pub mod apply;
pub mod marks;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};

/// `return promise` from an `async` function: the resolve-thenable job,
/// then the `.then` reaction one turn after `promise` settles.
pub(crate) async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.reaction().await
}
