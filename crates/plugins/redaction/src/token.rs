//! `token.ts`: the package token. The host lens and the public capability
//! are one Rust type, so there is one token.

use std::sync::LazyLock;

use epdf_kernel::Token;

use crate::controller::RedactionCapability;

/// `RedactionToken`, `'redaction'`.
pub static REDACTION: LazyLock<Token<RedactionCapability>> =
    LazyLock::new(|| Token::new("redaction"));
