//! `host.ts`: the one script realm per document, and the transaction mutex
//! every script run goes through.
//!
//! The sandbox is a capability the embedder supplies ([`ScriptSandbox`]), not
//! part of this port. The TypeScript lazily imports a QuickJS one as its
//! default; Rust has no default, and a host with no sandbox never builds a
//! [`ScriptHost`] (the actions plugin then behaves as the TypeScript does with
//! scripting disabled).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::rt::{self, Promise};
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::{Error, engine::DocumentHandle};
use serde_json::Value;

use crate::types::{
    DEFAULT_SCRIPT_BUDGET, PartialScriptIdentity, ScriptAnnotInput, ScriptBudget, ScriptDiagnostic,
    ScriptDiagnosticCode, ScriptDocumentInput, ScriptEnvironment, ScriptEventInput,
    ScriptEventKind, ScriptFieldInput, ScriptIdentity, ScriptInput, ScriptOutput,
};

/// One isolated, stateful Acrobat-JavaScript realm for one PDF document: JSON
/// in, JSON out. `boot` and `run` return `Err` where a JavaScript
/// implementation would throw.
pub trait ScriptSandbox {
    /// True after explicit disposal or a resource/runtime fault.
    fn disposed(&self) -> bool;
    /// Evaluate the document's name-tree sources once.
    fn boot(
        &self,
        sources: &[String],
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error>;
    /// Run one event program against the realm's persistent globals.
    fn run(
        &self,
        source: &str,
        input: &ScriptInput,
        budget: Option<&ScriptBudget>,
    ) -> Result<ScriptOutput, Error>;
    /// Release the realm. Idempotent.
    fn dispose(&self);
}

/// `() => Promise<ScriptSandbox>`.
pub type ScriptSandboxFactory = Rc<dyn Fn() -> Promise<Rc<dyn ScriptSandbox>>>;

/// The per-run world a caller supplies. Document, identity and environment
/// come from the host.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptWorldInput {
    pub fields: Vec<ScriptFieldInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annots: Option<Vec<ScriptAnnotInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annot_pages: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annots_cover_document: Option<bool>,
    /// Zero-based current page for `this.pageNum`; overrides the host's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_number: Option<f64>,
    pub event: ScriptEventInput,
}

pub struct ScriptHostOptions {
    pub sandbox_factory: ScriptSandboxFactory,
    pub document: Rc<dyn Fn() -> ScriptDocumentInput>,
    pub identity: Rc<dyn Fn() -> ScriptIdentity>,
    /// Deterministic environment per run; the argument is the host's run
    /// counter (see [`seed_from`]).
    pub environment: Rc<dyn Fn(f64) -> ScriptEnvironment>,
    /// Name-tree programs, fetched lazily once per realm build. `Err` is a
    /// synchronous throw.
    pub boot_sources: Rc<dyn Fn() -> Result<Promise<Vec<String>>, Error>>,
    pub budget: Option<ScriptBudget>,
}

/// FNV-1a over `documentId:sequence`, the deterministic default seed.
///
/// `for...of` walks code points and `charCodeAt(0)` takes each one's first
/// UTF-16 unit, so only the high surrogate of an astral character is hashed.
pub fn seed_from(document_id: &str, sequence: f64) -> f64 {
    let mut hash: u32 = 2_166_136_261;
    let text = format!("{document_id}:{}", crate::js::number_to_string(sequence));
    for c in text.chars() {
        let mut units = [0u16; 2];
        hash ^= u32::from(c.encode_utf16(&mut units)[0]);
        hash = hash.wrapping_mul(16_777_619);
    }
    f64::from(hash)
}

/// The identity overrides an embedder configures: a value, or a function
/// called per run.
#[derive(Clone)]
pub enum IdentityOverrides {
    Value(PartialScriptIdentity),
    Function(Rc<dyn Fn() -> PartialScriptIdentity>),
}

/// Engine JWT identity composed under embedder overrides.
pub fn resolve_script_identity(
    doc: &dyn DocumentHandle,
    overrides: Option<&IdentityOverrides>,
) -> ScriptIdentity {
    resolve_script_identity_from(&doc.security().identity(), overrides)
}

/// [`resolve_script_identity`] over the claims themselves.
pub fn resolve_script_identity_from(
    claims: &Value,
    overrides: Option<&IdentityOverrides>,
) -> ScriptIdentity {
    let supplied = match overrides {
        Some(IdentityOverrides::Function(f)) => f(),
        Some(IdentityOverrides::Value(v)) => v.clone(),
        None => PartialScriptIdentity::default(),
    };
    let claim = |key: &str| claims.get(key).and_then(Value::as_str).map(str::to_string);
    ScriptIdentity {
        name: supplied
            .name
            .or_else(|| claim("display_name"))
            .or_else(|| claim("user_id"))
            .unwrap_or_default(),
        login_name: supplied.login_name.or_else(|| claim("user_id")).unwrap_or_default(),
        corporation: supplied.corporation.or_else(|| claim("group_id")).unwrap_or_default(),
        email: supplied.email.unwrap_or_default(),
    }
}

struct Inner {
    options: ScriptHostOptions,
    budget: ScriptBudget,
    queue: SerialQueue,
    sandbox: RefCell<Option<Promise<Rc<dyn ScriptSandbox>>>>,
    booted: Cell<bool>,
    pending_boot: RefCell<Option<ScriptOutput>>,
    boot_sources: RefCell<Option<Promise<Vec<String>>>>,
    sequence: Cell<f64>,
    disposed: Cell<bool>,
}

/// The one realm per document. `transaction(body)` is the realm mutex; the
/// body does everything for one logical script transaction (prefetch, runs,
/// commits through the owner sinks) before it releases. A sandbox that
/// reports itself disposed poisons the realm for the rest of the transaction;
/// the next one rebuilds it and boots again.
#[derive(Clone)]
pub struct ScriptHost(Rc<Inner>);

/// What a transaction body drives.
#[derive(Clone)]
pub struct ScriptTransaction(Rc<Inner>);

fn ensure_sandbox(inner: &Rc<Inner>) -> Promise<Rc<dyn ScriptSandbox>> {
    let existing = inner.sandbox.borrow().clone();
    existing.unwrap_or_else(|| {
        let sandbox = (inner.options.sandbox_factory)();
        *inner.sandbox.borrow_mut() = Some(sandbox.clone());
        sandbox
    })
}

fn input_for(inner: &Inner, world: &ScriptWorldInput) -> ScriptInput {
    let mut document = (inner.options.document)();
    if let Some(page_number) = world.page_number {
        document.page_number = page_number;
    }
    let identity = (inner.options.identity)();
    let sequence = inner.sequence.get();
    inner.sequence.set(sequence + 1.0);
    let environment = (inner.options.environment)(sequence);
    ScriptInput {
        document,
        identity,
        environment,
        fields: world.fields.clone(),
        annots: world.annots.clone(),
        annot_pages: world.annot_pages.clone(),
        annots_cover_document: world.annots_cover_document,
        event: world.event.clone(),
    }
}

fn note_fault(inner: &Inner, sandbox: &Rc<dyn ScriptSandbox>) {
    if !sandbox.disposed() {
        return;
    }
    *inner.sandbox.borrow_mut() = None;
    inner.booted.set(false);
    *inner.pending_boot.borrow_mut() = None;
}

async fn boot_now(
    inner: Rc<Inner>,
    world: ScriptWorldInput,
    budget: Option<ScriptBudget>,
) -> Result<Option<ScriptOutput>, Error> {
    if inner.booted.get() {
        return Ok(None);
    }
    // Degrade, never brick: booted before running, so a hostile boot script
    // does not re-run per event.
    inner.booted.set(true);
    let sandbox = ensure_sandbox(&inner).await?;
    let cached = inner.boot_sources.borrow().clone();
    let sources = match cached {
        Some(sources) => sources,
        None => {
            let sources = (inner.options.boot_sources)()?;
            *inner.boot_sources.borrow_mut() = Some(sources.clone());
            sources
        }
    };
    let sources = match sources.await {
        Ok(sources) => sources,
        Err(error) => {
            // A transient read failure: retry on rebuild, and say so.
            *inner.boot_sources.borrow_mut() = None;
            let mut output = ScriptOutput::empty();
            output.diagnostics.push(ScriptDiagnostic {
                code: ScriptDiagnosticCode::ScriptError,
                message: format!(
                    "Document boot script failed (continuing without it): {}",
                    error.message
                ),
            });
            return Ok(Some(output));
        }
    };
    if sources.is_empty() {
        return Ok(None);
    }
    let boot_world =
        ScriptWorldInput { event: ScriptEventInput::of(ScriptEventKind::NameTreeBoot), ..world };
    let input = input_for(&inner, &boot_world);
    let output = sandbox.boot(&sources, &input, Some(budget.as_ref().unwrap_or(&inner.budget)))?;
    note_fault(&inner, &sandbox);
    Ok(Some(output))
}

async fn run_now(
    inner: Rc<Inner>,
    program: String,
    world: ScriptWorldInput,
    budget: Option<ScriptBudget>,
) -> Result<ScriptOutput, Error> {
    if !inner.booted.get() {
        // Defensive auto-boot; the output waits for the next explicit boot().
        let output = rt::returned(boot_now(inner.clone(), world.clone(), budget.clone())).await?;
        *inner.pending_boot.borrow_mut() = output;
    }
    let sandbox = ensure_sandbox(&inner).await?;
    let input = input_for(&inner, &world);
    let output = sandbox.run(&program, &input, Some(budget.as_ref().unwrap_or(&inner.budget)))?;
    note_fault(&inner, &sandbox);
    Ok(output)
}

impl ScriptTransaction {
    /// Run the name-tree boot if this realm has not run it; the boot output
    /// comes back exactly once, `None` thereafter.
    pub fn boot(
        &self,
        world: ScriptWorldInput,
        budget: Option<ScriptBudget>,
    ) -> Promise<Option<ScriptOutput>> {
        let inner = self.0.clone();
        Promise::spawn(async move {
            let pending = inner.pending_boot.borrow_mut().take();
            if let Some(output) = pending {
                return Ok(Some(output));
            }
            // `return bootNow(...)` from an async function: the returned
            // promise is adopted, a turn for the thenable job and one for
            // the reaction.
            let boot = Promise::spawn(boot_now(inner, world, budget));
            rt::tick().await;
            boot.await
        })
    }

    /// One deterministic run against the shared realm. Auto-boots
    /// defensively.
    pub fn run(
        &self,
        program: &str,
        world: ScriptWorldInput,
        budget: Option<ScriptBudget>,
    ) -> Promise<ScriptOutput> {
        Promise::spawn(run_now(self.0.clone(), program.to_string(), world, budget))
    }
}

impl ScriptHost {
    pub fn new(options: ScriptHostOptions) -> Self {
        let budget = options.budget.clone().unwrap_or(DEFAULT_SCRIPT_BUDGET);
        ScriptHost(Rc::new(Inner {
            options,
            budget,
            queue: SerialQueue::new(),
            sandbox: RefCell::new(None),
            booted: Cell::new(false),
            pending_boot: RefCell::new(None),
            boot_sources: RefCell::new(None),
            sequence: Cell::new(0.0),
            disposed: Cell::new(false),
        }))
    }

    /// The realm mutex. `body` returns the promise of its work, or `Err` for
    /// a synchronous throw.
    pub fn transaction<T: Clone + 'static>(
        &self,
        body: impl FnOnce(ScriptTransaction) -> Result<Promise<T>, Error> + 'static,
    ) -> Promise<T> {
        if self.0.disposed.get() {
            return Promise::rejected(Error::new("script host is disposed"));
        }
        let txn = ScriptTransaction(self.0.clone());
        self.0.queue.run(move || body(txn))
    }

    pub fn dispose(&self) {
        self.0.disposed.set(true);
        let dying = self.0.sandbox.borrow_mut().take();
        self.0.booted.set(false);
        *self.0.pending_boot.borrow_mut() = None;
        if let Some(dying) = dying {
            rt::spawn(async move {
                if let Ok(sandbox) = dying.await {
                    sandbox.dispose();
                }
            });
        }
    }
}

/// `createScriptHost(options)`.
pub fn create_script_host(options: ScriptHostOptions) -> ScriptHost {
    ScriptHost::new(options)
}
