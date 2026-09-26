//! `token.ts`: the package token, which `host-contract.ts` carries for
//! siblings. The host lens equals the public one, so there is one token.

use std::sync::LazyLock;

use epdf_kernel::Token;

use crate::controller::MeasurementCapability;

/// `MeasurementToken`, `'measurement'`.
pub static MEASUREMENT: LazyLock<Token<MeasurementCapability>> =
    LazyLock::new(|| Token::new("measurement"));
