//! Capability tokens: a typed handle whose identity is the object, as a
//! `createCapabilityToken` result is in TypeScript. Two tokens with the same
//! name are different tokens.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
struct Inner {
    id: u64,
    name: String,
    hint: Option<String>,
}

/// A token with its capability type erased: what plans and registries key on.
#[derive(Clone)]
pub struct AnyToken(Arc<Inner>);

impl AnyToken {
    pub fn new(name: impl Into<String>, hint: Option<String>) -> Self {
        AnyToken(Arc::new(Inner {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            name: name.into(),
            hint,
        }))
    }
    pub fn name(&self) -> &str {
        &self.0.name
    }
    /// The authored remedy for a missing provider.
    pub fn hint(&self) -> Option<&str> {
        self.0.hint.as_deref()
    }
}

impl PartialEq for AnyToken {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl Eq for AnyToken {}
impl Hash for AnyToken {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}
impl fmt::Debug for AnyToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Token({})", self.0.name)
    }
}

/// `CapabilityToken<C>`: resolving it yields a `C`.
pub struct Token<C> {
    any: AnyToken,
    _capability: PhantomData<fn() -> C>,
}

impl<C> Token<C> {
    pub fn new(name: impl Into<String>) -> Self {
        Token { any: AnyToken::new(name, None), _capability: PhantomData }
    }
    pub fn with_hint(name: impl Into<String>, hint: impl Into<String>) -> Self {
        Token { any: AnyToken::new(name, Some(hint.into())), _capability: PhantomData }
    }
    /// Reinterpret an erased token. The caller vouches for the type.
    pub fn from_any(any: AnyToken) -> Self {
        Token { any, _capability: PhantomData }
    }
    pub fn any(&self) -> &AnyToken {
        &self.any
    }
    pub fn name(&self) -> &str {
        self.any.name()
    }
}

impl<C> Clone for Token<C> {
    fn clone(&self) -> Self {
        Token { any: self.any.clone(), _capability: PhantomData }
    }
}

impl<C> fmt::Debug for Token<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.any.fmt(f)
    }
}
