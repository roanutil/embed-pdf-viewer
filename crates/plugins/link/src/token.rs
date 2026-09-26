//! `token.ts`: the package token, which `host-contract.ts` widens to the
//! host capability. The Rust capability is one type, so there is one token.

use std::sync::LazyLock;

use epdf_kernel::Token;

use crate::controller::LinkCapability;

/// `LinkToken`, `'link'`.
pub static LINK: LazyLock<Token<LinkCapability>> = LazyLock::new(|| Token::new("link"));
