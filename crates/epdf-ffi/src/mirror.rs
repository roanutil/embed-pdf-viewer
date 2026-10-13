//! Helpers every plugin's `ffi` module uses for its mirrors.

/// `From` both ways between two fieldless enums with the same variants.
#[macro_export]
macro_rules! mirror_enum {
    ($ffi:ident <=> $rust:path { $($variant:ident),+ $(,)? }) => {
        impl From<$rust> for $ffi {
            fn from(value: $rust) -> Self {
                match value {
                    $(<$rust>::$variant => $ffi::$variant,)+
                }
            }
        }
        impl From<$ffi> for $rust {
            fn from(value: $ffi) -> Self {
                match value {
                    $($ffi::$variant => <$rust>::$variant,)+
                }
            }
        }
    };
}

/// A count or index as a `u32`, or `invalid-state`: never a clamp.
pub fn to_u32(n: usize, what: &str) -> Result<u32, epdf_kernel::Error> {
    u32::try_from(n).map_err(|_| {
        epdf_kernel::Error::named("InvalidState", format!("{what} ({n}) does not fit a u32"))
            .with("code", "invalid-state")
    })
}
