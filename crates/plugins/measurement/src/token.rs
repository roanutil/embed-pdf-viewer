//! `token.ts`: the package token. The host lens and the public capability
//! are one Rust type, so there is one token.

use std::sync::LazyLock;

use epdf_kernel::Token;

use crate::controller::MeasurementCapability;

/// `MeasurementToken`, `'measurement'`.
pub static MEASUREMENT: LazyLock<Token<MeasurementCapability>> =
    LazyLock::new(|| Token::new("measurement"));
