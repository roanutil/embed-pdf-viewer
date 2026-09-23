//! The lease state machine, and a session that owns a document behind it.
//!
//! See the module doc for what this is and when it gets deleted.

use std::ffi::c_void;

use pdfium::Document;

use crate::error::{EngineError, EngineErrorCode};

/// Identifies one checkout. Opaque to the host, which only hands it back.
///
/// A counter rather than a pointer or an index: a stale token from a previous
/// lease has to be rejected rather than silently accepted as the current one.
/// Monotonic is enough for that; this is a protocol check, not a security
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseToken(u64);

impl LeaseToken {
    pub fn get(self) -> u64 {
        self.0
    }

    /// Rebuilds a token from the wire.
    ///
    /// The host round-trips the value as an integer, so it has to come back
    /// somehow. Safe because the session compares it against the outstanding
    /// token and refuses anything else: a fabricated value is a rejection, not
    /// a forged lease.
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }
}

/// What the host did with the lease, reported when it returns.
///
/// `wrote` is load-bearing and the host is trusted for it, because nothing on
/// the Rust side can observe whether a mutation ran through a pointer it handed
/// away. That is a real weakness of the Stage B shape, worth stating rather
/// than hiding: a host that returns `wrote: false` after mutating leaves the
/// session's revision stale, and no check here would notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseOutcome {
    /// Whether the host applied any change to the document.
    pub wrote: bool,
    /// Whether the host's own work failed partway.
    ///
    /// A different question from `wrote`. Section 7: mutations are not
    /// rollback-atomic, and no throw path may pretend no write occurred. A
    /// host that wrote and then failed reports both, and finalization still
    /// runs.
    pub failed: bool,
    /// Whether the host panicked, crashed, or otherwise stopped without
    /// knowing where it got to.
    ///
    /// Distinct from `failed`, and the distinction is the whole point.
    /// `failed` is a reported error: the host knows what happened and the
    /// document is in a state it can describe. `panicked` means nobody knows.
    /// Section 5.3: "Panic after a potentially mutating operation poisons the
    /// session unless an operation-specific invariant says otherwise."
    pub panicked: bool,
}

/// What finalization settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finalized {
    /// The sequence after this mutation, unchanged when nothing was written.
    pub mutation_sequence: u64,
    /// Whether the cached form model was dropped.
    pub form_model_invalidated: bool,
    /// Carried through from the outcome, so a caller that sees only the
    /// finalization still learns the write failed.
    pub failed: bool,
    /// Whether this finalization poisoned the session.
    pub poisoned: bool,
}

/// Where the session is in the lease protocol.
///
/// Closing is deliberately NOT a state here. A close that arrives mid-lease
/// sets a flag and leaves the protocol where it was, because the return and
/// the finalize still have to happen and they need their token and outcome.
/// Folding those into a `Closing` variant loses exactly the information the
/// close is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No lease out, nothing pending. The only state that accepts a checkout
    /// or an ordinary read.
    Idle,
    /// The host holds a pointer into this session's document.
    LeasedOut(LeaseToken),
    /// The pointer is back and the outcome is not settled. Still not a state
    /// that accepts other work: the revision is stale until finalize.
    AwaitingFinalize(LeaseToken, LeaseOutcome),
    Closed,
}

/// A document Rust owns, lent to the host one scoped operation at a time.
#[derive(Debug)]
pub struct SpikeSession {
    document: Option<Document>,
    state: State,
    /// Set by `close`, honoured when the protocol next reaches a settled
    /// point. Separate from `state` so a close cannot erase an outstanding
    /// lease.
    close_requested: bool,
    next_token: u64,
    mutation_sequence: u64,
    /// Stands in for `formModelCache.ts`'s one `EPDFForm_LoadModel` snapshot
    /// per session, keyed on mutation sequence. The spike only needs to know
    /// whether a write dropped it.
    form_model_loaded: bool,
    /// Set when a host panicked partway through a write.
    ///
    /// A poisoned session serves nothing but `close`. Not even reads: the
    /// document may be halfway through a structural edit, and a read of that
    /// is a plausible-looking wrong answer, which is worse than an error.
    poisoned: bool,
}

impl SpikeSession {
    pub fn new(document: Document) -> Self {
        Self {
            document: Some(document),
            state: State::Idle,
            close_requested: false,
            next_token: 1,
            mutation_sequence: 0,
            form_model_loaded: false,
            poisoned: false,
        }
    }

    pub fn mutation_sequence(&self) -> u64 {
        self.mutation_sequence
    }

    pub fn form_model_loaded(&self) -> bool {
        self.form_model_loaded
    }

    pub fn is_closed(&self) -> bool {
        self.state == State::Closed
    }

    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Loads the cached form model, so a test can watch a write drop it.
    pub fn load_form_model(&mut self) -> Result<(), EngineError> {
        self.require_open_and_idle("load the form model")?;
        self.form_model_loaded = true;
        Ok(())
    }

    /// How many annotations a page carries, read through the owned document.
    ///
    /// The spike's proof that a host write through a leased pointer actually
    /// landed: Rust counts before and after, and the number it reads comes
    /// from the document it owns rather than from anything the host said.
    pub fn annotation_count(&self, page_index: u32) -> Result<u32, EngineError> {
        self.require_open_and_idle("count annotations")?;
        let document = self.document.as_ref().expect("idle implies open");
        let page = document
            .page(page_index)
            .map_err(|e| EngineError::with_message(EngineErrorCode::NotFound, e.to_string()))?;
        Ok(page.annotation_count())
    }

    /// An ordinary read, standing in for every document operation that is not
    /// part of the lease protocol.
    ///
    /// Exists to be rejected. Section 6: "While a lease or unfinalized outcome
    /// exists, reject all other document operations; enforce this at runtime
    /// because no Rust borrow spans the host call."
    pub fn page_count(&self) -> Result<u32, EngineError> {
        self.require_open_and_idle("read the page count")?;
        Ok(self.document.as_ref().expect("idle implies open").page_count())
    }

    /// Hands the host a token and a copy of the document pointer.
    ///
    /// The pointer is a copy, never a borrow of Rust state: nothing here holds
    /// a `&` across the host's call, because nothing could.
    ///
    /// # Safety of what the caller receives
    /// The returned pointer is valid only until [`SpikeSession::return_lease`]
    /// is called with the same token. Using it afterwards is use-after-free,
    /// and no check in this module can catch that.
    pub fn checkout(&mut self) -> Result<(LeaseToken, *mut c_void), EngineError> {
        self.require_open_and_idle("check out a lease")?;

        let token = LeaseToken(self.next_token);
        self.next_token += 1;
        // SAFETY: the raw handle is copied out while the Document is alive and
        // stays alive. `close` cannot drop it until the protocol settles,
        // which requires this token to come home.
        let handle = unsafe { self.document.as_ref().expect("idle implies open").as_raw() };
        self.state = State::LeasedOut(token);
        Ok((token, handle.cast()))
    }

    /// Takes the lease back, with what the host did.
    ///
    /// Accepted while a close is pending: the return is what that close is
    /// waiting for, so refusing it would deadlock the session it is trying to
    /// shut down.
    pub fn return_lease(
        &mut self,
        token: LeaseToken,
        outcome: LeaseOutcome,
    ) -> Result<(), EngineError> {
        let outstanding = match self.state {
            State::LeasedOut(outstanding) => outstanding,
            State::Idle => {
                return Err(Self::violation("no lease is outstanding"));
            }
            State::AwaitingFinalize(..) => {
                return Err(Self::violation("this lease was already returned"));
            }
            State::Closed => return Err(Self::closed()),
        };
        Self::require_token(token, outstanding, "the outstanding lease")?;
        self.state = State::AwaitingFinalize(token, outcome);
        Ok(())
    }

    /// Settles the outcome: advances the sequence and invalidates what the
    /// write invalidated.
    ///
    /// Runs even when the host reported a failure. A write that happened and
    /// then failed is still a write, and leaving the sequence behind it would
    /// be the session claiming a mutation did not occur.
    ///
    /// This is also where a pending close completes, which is why the document
    /// is dropped here rather than in `close`.
    pub fn finalize(&mut self, token: LeaseToken) -> Result<Finalized, EngineError> {
        let (outstanding, outcome) = match self.state {
            State::AwaitingFinalize(outstanding, outcome) => (outstanding, outcome),
            State::LeasedOut(_) => {
                return Err(Self::violation("the lease has not been returned yet"));
            }
            State::Idle => {
                return Err(Self::violation("there is no outcome to finalize"));
            }
            State::Closed => return Err(Self::closed()),
        };
        Self::require_token(token, outstanding, "the returned lease")?;

        // A panic partway through a write leaves the document in a state
        // nobody can describe. Poison before anything else, so the rest of
        // finalization cannot hand back a number that implies the session is
        // still usable.
        if outcome.panicked && outcome.wrote {
            self.poisoned = true;
        }

        let mut form_model_invalidated = false;
        if outcome.wrote {
            self.mutation_sequence += 1;
            // `formModelCache.ts` keys its one snapshot on the mutation
            // sequence, so any write invalidates it. Dropping a flag rather
            // than a real handle is the whole of what the spike needs: the
            // point is that invalidation is wired to the write and not to the
            // write having succeeded.
            form_model_invalidated = self.form_model_loaded;
            self.form_model_loaded = false;
        }

        if self.close_requested {
            self.state = State::Closed;
            self.document = None;
        } else {
            self.state = State::Idle;
        }

        Ok(Finalized {
            mutation_sequence: self.mutation_sequence,
            form_model_invalidated,
            failed: outcome.failed,
            poisoned: self.poisoned,
        })
    }

    /// Rejects new work immediately; completes once the protocol settles.
    ///
    /// Returns whether the document was actually released. `false` is the
    /// close-under-lease case Section 6 names: a close may not drop a document
    /// the host still holds a pointer into, so it records the intent and the
    /// release happens in `finalize`.
    pub fn close(&mut self) -> bool {
        self.close_requested = true;
        match self.state {
            State::Idle | State::Closed => {
                self.state = State::Closed;
                self.document = None;
                true
            }
            State::LeasedOut(_) | State::AwaitingFinalize(..) => false,
        }
    }

    fn require_open_and_idle(&self, what: &str) -> Result<(), EngineError> {
        if self.poisoned {
            return Err(EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                "the session is poisoned: a host panicked partway through a write",
            ));
        }
        if self.close_requested {
            return Err(Self::closed());
        }
        match self.state {
            State::Idle => Ok(()),
            State::LeasedOut(_) => {
                Err(Self::violation(format!("cannot {what} while a lease is outstanding")))
            }
            State::AwaitingFinalize(..) => {
                Err(Self::violation(format!("cannot {what} while an outcome is unfinalized")))
            }
            State::Closed => Err(Self::closed()),
        }
    }

    fn require_token(
        given: LeaseToken,
        expected: LeaseToken,
        what: &str,
    ) -> Result<(), EngineError> {
        if given == expected {
            return Ok(());
        }
        Err(Self::violation(format!("token {} is not {what}", given.get())))
    }

    fn violation(message: impl Into<String>) -> EngineError {
        EngineError::with_message(EngineErrorCode::InvalidArg, message)
    }

    fn closed() -> EngineError {
        EngineError::with_message(EngineErrorCode::DocNotOpen, "the session is closed")
    }
}
