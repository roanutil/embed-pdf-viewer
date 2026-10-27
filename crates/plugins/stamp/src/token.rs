//! `token.ts`: the package token.

use std::sync::LazyLock;

use epdf_kernel::Token;

use crate::controller::StampCapability;

/// `StampToken`.
pub static STAMP: LazyLock<Token<StampCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "stamp",
        "add stampPlugin() from '@embedpdf/plugin-stamp' to your plugins list",
    )
});
