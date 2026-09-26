//! `token.ts`: the package token.

use std::sync::LazyLock;

use epdf_kernel::Token;

use crate::controller::RedactionCapability;

/// `RedactionToken`, `'redaction'`.
pub static REDACTION: LazyLock<Token<RedactionCapability>> =
    LazyLock::new(|| Token::new("redaction"));
