//! `compose.ts`: `composeApi`, the composition root's one primitive. A
//! controller builds its capability from area slices plus service-owned
//! members; this joins them and refuses a key two slices both define, so a
//! new verb can never silently shadow an old one.
//!
//! A Rust capability is a struct, whose fields the compiler already keeps
//! unique, so plugin ports rarely need this. It is here for the ones that
//! assemble a capability from named members at run time.

use crate::error::Error;

/// Join the slices in order; `[<capability>] api member '<key>' is defined
/// twice` for a repeated key.
pub fn compose_api<T>(
    capability: &str,
    slices: Vec<Vec<(String, T)>>,
) -> Result<Vec<(String, T)>, Error> {
    let mut api: Vec<(String, T)> = Vec::new();
    for slice in slices {
        for (key, member) in slice {
            if api.iter().any(|(k, _)| *k == key) {
                return Err(Error::new(format!(
                    "[{capability}] api member '{key}' is defined twice"
                )));
            }
            api.push((key, member));
        }
    }
    Ok(api)
}
