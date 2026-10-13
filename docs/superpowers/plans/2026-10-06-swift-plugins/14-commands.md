# Swift Wrapper for `commands` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export the workspace `commands` plugin through UniFFI as `CommandsHandle`, wire it into `ViewerHost` behind `ViewerConfig.commands`, and give Swift a `CommandsCapability` whose commands can run Swift code through a `CommandRunner` foreign trait.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, with metadata as the template and plan 13 (i18n) as the template for a workspace plugin with a foreign port. A command definition holds closures in Rust (`run`, `enabled`, `active`, `visible`, `icon_accent`). Swift supplies one `CommandRunner` per definition plus the list of hooks it implements; Rust builds only the closures for listed hooks, so an unlisted hook keeps the plugin's default and never crosses into Swift. `run` answers through a `CommandCompletion` object built on plan 13's `epdf_ffi::ForeignSettle`, so a Swift command can be async. Derivations are synchronous Swift calls on the kernel thread.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, `futures` 0.3, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator (iOS 26.0).

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Decisions, Appendix A §A.0 and §A.4 "commands"). Plan 13 (`13-i18n.md`) defines `ForeignSettle` and `HostError`, which this plan consumes.

## Global Constraints

Every line of the umbrella's Global Constraints applies. The ones this plan meets most often:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI type names are unique across the workspace. Generic plugin names get a `Command` prefix: `CommandExecuteResult`, `CommandRejectReason`, `CommandShortcutBinding`, `CommandMenuTarget`, `CommandKeyStroke`.
- No `usize`, `isize`, `Rc` or `serde_json::Value` across the boundary. Command `args` cross as JSON text (see Decisions).
- Facades don't decide semantics. Resolution, category gating, shortcut parsing and surface routing stay in `crates/plugins/commands/src`.
- Swift value assertions use `expectNoDifference`.
- No npm package changes, no changesets.

## Review Focus

1. A Swift derivation that throws. Expected: the plugin's own fallback applies (`enabled` and `visible` true, `active` false, no accent), as `controller.rs` `derive` does for a Rust throw, and nothing is reported as a crash. Pinned in Task 2 (`a_throwing_derivation_falls_back`).
2. A Swift `run` that drops its completion without answering. Expected: `execute` rejects with `operation-failed` and the `executionFailed` event fires, instead of a command that never finishes. Pinned in Task 2 (`a_dropped_completion_fails_the_command`).
3. A definition that names hooks but has no runner, or has a runner but names no hooks. Expected: `invalid-input` at registration, not a command that silently ignores its handler. Pinned in Task 1 (`hooks_without_a_runner_are_refused`, `a_runner_without_hooks_is_refused`).
4. `execute` with `args_json` that isn't JSON. Expected: `invalid-input` before the command runs, and no `executed` event. Pinned in Task 2 (`unparsable_args_are_invalid_input`).
5. A registration dropped by Swift without `unregister()`. Expected: the command stays registered (as the TypeScript remover is optional), and `unregister()` removes only the definition it registered, not a later replacement. Pinned in Task 2 (`unregistering_a_replaced_command_keeps_the_replacement`).

## Decisions

`ViewerConfig.commands` is a `bool`, as the umbrella's table has it. The plugin's config holds definitions, and definitions hold Swift objects; registering them after `ViewerHost::new` through `register_commands` behaves the same (a bad definition fails that call instead of the kernel), and keeps `ViewerConfig` free of foreign objects. Startup `disabled_categories` go through `set_disabled_categories` the same way.

`args` cross as JSON text (`args_json: Option<String>`). They're app data the plugin never reads; it only hands them back to `run` and to the `executed` event. A typed record would be a guess at every app's argument shape. The cost: a Swift caller encodes and decodes the JSON itself, and a typo fails at runtime. The Swift facade takes and returns `Data?` through `JSONEncoder` to keep that in one place.

`execute` takes no `CancelToken`. A command run has no abort in the plugin, and a Swift handler owns its own cancellation.

A `CommandRunner`'s derivations are synchronous calls on the kernel thread, made on every `resolve_command`, `list_commands` and `search_commands`. A Swift implementation must answer from state it already has. It must not hop to the main actor synchronously: if the main actor is awaiting the kernel, that deadlocks. The Swift doc comment says so, and the convenience `CommandHandler` takes `@Sendable` closures to steer people that way.

`get_command` (`command`) returns a `CommandSummary` without the runner: the Rust definition holds closures built from it, and the original foreign object can't be recovered from them. `has_runner` says whether one was registered.

## File Structure

```
crates/plugins/commands/Cargo.toml             uniffi feature, deps
crates/plugins/commands/src/lib.rs             scaffolding, mod ffi
crates/plugins/commands/src/contract.rs        derive in place: IconAccent, ResolvedCommand
crates/plugins/commands/src/ffi.rs             records, CommandsHandle, CommandsEvent, CommandRunner, completion
crates/plugins/commands/tests/ffi.rs
crates/epdf-viewer-host/Cargo.toml
crates/epdf-viewer-host/src/api.rs             ViewerConfig.commands
crates/epdf-viewer-host/src/session.rs
crates/epdf-viewer-host/src/lib.rs             ViewerHost::commands()
crates/epdf-viewer-host/tests/viewer_host.rs
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Commands.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/CommandsTests.swift
.github/workflows/plugin-rust.yml
platforms/apple/README.md
docs/superpowers/plans/2026-10-01-swift-package.md
```

## Cross-plugin tests

commands has optional `i18n` and `shell` (`crates/plugins/commands/src/lib.rs`).

- i18n (plan 13) merges before this plan (umbrella, Waves). This plan owns `a_command_label_resolves_through_i18n` (Task 3).
- shell (plan 12) merges before this plan too, but plan 12's Task 1 decides whether shell ships to Swift at all. If `ViewerConfig.shell` exists when this plan runs, this plan owns `a_panel_command_toggles_the_shell_surface` (Task 3). If it doesn't, skip that test and say so in the merge commit message; no later plan picks it up.

---

### Task 1: Records and definitions

**Files:**
- Modify: `crates/plugins/commands/Cargo.toml`
- Modify: `crates/plugins/commands/src/lib.rs`
- Modify: `crates/plugins/commands/src/contract.rs`
- Create: `crates/plugins/commands/src/ffi.rs`
- Test: `crates/plugins/commands/tests/ffi.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{ForeignSettle, HostError, KernelPost, PluginErrorInfo, RegistrationHandle, ViewerError}` (umbrella Task 0c adds `ForeignSettle`, `HostError` and `RegistrationHandle`).
- Produces: `CommandDefinition` record, `CommandHook { Run, Enabled, Active, Visible, IconAccent }`, `CommandPanel { id, exclusive }`, `CommandContext { command_id, document_id, args_json }`, `CommandRunner` foreign trait (`run`, `derive`, `icon_accent`), `CommandCompletion` object (`finish()`, `fail(detail)`), and `fn into_def(self, post: KernelPost) -> Result<CommandDef, Error>` on `CommandDefinition`.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-commands -b research/swift-commands research/swift-plugins
cd .claude/worktrees/swift-commands
test -f crates/epdf-ffi/src/foreign.rs || { echo "umbrella Task 0c must land first: ForeignSettle is missing"; exit 1; }
bash platforms/apple/build-xcframework.sh
stat -f%z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
```

Fetch PDFium first as `platforms/apple/README.md` describes. Keep the size for the final commit message.

- [x] **Step 2: Add the feature**

In `crates/plugins/commands/Cargo.toml`, under `[features]`:

```toml
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi"]
```

Under `[dependencies]`:

```toml
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }
```

Add:

```toml
[dev-dependencies]
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
plugin-conformance = { path = "../../plugin-conformance" }
epdf-plugin-i18n = { path = "../i18n" }
```

In `crates/plugins/commands/src/lib.rs`, after `pub mod registry;`:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

In `crates/plugins/commands/src/contract.rs`, add `#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]` under the `#[derive(...)]` line of `IconAccent` and of `ResolvedCommand`. Every field of both is a `String`, `bool`, `Vec<String>` or `Option` of those (or of `IconAccent`), so they derive in place.

- [x] **Step 3: Write the failing definition tests**

Create `crates/plugins/commands/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::sync::{Arc, Mutex};

use epdf_ffi::{HostError, KernelThread, ViewerError};
use epdf_plugin_commands::IconAccent;
use epdf_plugin_commands::ffi::{
    CommandCompletion, CommandContext, CommandDefinition, CommandHook, CommandRunner,
};
use futures::executor::block_on;

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

/// What the test runner does when `run` is called.
#[derive(Clone, Copy)]
enum Run {
    Finish,
    Fail,
    Drop,
}

struct Runner {
    run: Run,
    enabled: Result<bool, ()>,
    ran: Mutex<Vec<CommandContext>>,
}

impl Runner {
    fn new(run: Run, enabled: Result<bool, ()>) -> Arc<Self> {
        Arc::new(Runner { run, enabled, ran: Mutex::default() })
    }
}

impl CommandRunner for Runner {
    fn run(&self, context: CommandContext, completion: Arc<CommandCompletion>) {
        self.ran.lock().unwrap().push(context);
        match self.run {
            Run::Finish => completion.finish(),
            Run::Fail => completion.fail("disk full".into()),
            Run::Drop => drop(completion),
        }
    }

    fn derive(&self, _context: CommandContext, hook: CommandHook) -> Result<bool, HostError> {
        match hook {
            CommandHook::Enabled => {
                self.enabled.map_err(|()| HostError::Failed { detail: "derive threw".into() })
            }
            _ => Ok(true),
        }
    }

    fn icon_accent(&self, _context: CommandContext) -> Result<Option<IconAccent>, HostError> {
        Ok(Some(IconAccent { primary: Some("#f00".into()), secondary: None }))
    }
}

fn definition(id: &str, runner: Option<Arc<Runner>>, hooks: Vec<CommandHook>) -> CommandDefinition {
    CommandDefinition {
        id: id.into(),
        label_key: format!("commands.{id}"),
        icon: None,
        shortcuts: vec![],
        categories: vec![],
        menu: None,
        panel: None,
        modal: None,
        runner: runner.map(|r| r as Arc<dyn CommandRunner>),
        hooks,
    }
}

/// `into_def` needs a kernel thread for its `KernelPost`.
fn into_def(def: CommandDefinition) -> Result<(), ViewerError> {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    block_on(thread.call(move |_| def.into_def(post).map(|_| ()).map_err(ViewerError::from)))
        .unwrap_or(Err(ViewerError::Closed))
}

#[test]
fn hooks_without_a_runner_are_refused() {
    let error = into_def(definition("save", None, vec![CommandHook::Run])).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_runner_without_hooks_is_refused() {
    let runner = Runner::new(Run::Finish, Ok(true));
    let error = into_def(definition("save", Some(runner), vec![])).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_definition_with_neither_is_a_routing_command() {
    into_def(definition("zoom-in", None, vec![])).unwrap();
}
```

- [x] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p epdf-plugin-commands --features uniffi --test ffi`
Expected: FAIL to compile with "unresolved import `epdf_plugin_commands::ffi`".

- [x] **Step 5: Write the definition records in `crates/plugins/commands/src/ffi.rs`**

```rust
//! The commands plugin through uniffi: typed records, one workspace handle,
//! one event enum, and Swift-defined commands through `CommandRunner`.
//!
//! A definition's closures are built only for the hooks Swift lists, so an
//! unlisted hook keeps the plugin's default and never crosses the boundary.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    ForeignSettle, HostError, KernelBound, KernelPost, PluginErrorInfo, RegistrationHandle,
    Subscription, ViewerError,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::store::Unsubscribe;
use epdf_ui_core::shortcuts::{KeyStroke, ShortcutOptions};
use serde_json::Value;

use crate::contract::{
    CommandCtx, CommandDef, CommandExecutedEvent, CommandExecutionFailedEvent, Derivation,
    ExecuteResult, IconAccent, MenuTarget, Panel, RegisterCommandOptions, RejectReason,
    ResolvedCommand, Shortcut, ShortcutBinding,
};
use crate::controller::CommandsCapability;

fn invalid(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "commands", message, None, None)
}

/// Which of a command's hooks its runner implements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CommandHook {
    /// `run`: the verb. Without it, the command routes to its menu, panel or
    /// modal.
    Run,
    Enabled,
    Active,
    Visible,
    IconAccent,
}

/// A shell surface the command toggles, optionally exclusive within a tag.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandPanel {
    pub id: String,
    #[uniffi(default = None)]
    pub exclusive: Option<String>,
}

/// What a hook sees.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandContext {
    pub command_id: String,
    /// The target document: explicit, else the active one, else `None`.
    pub document_id: Option<String>,
    /// The caller's arguments as JSON, when it gave any.
    pub args_json: Option<String>,
}

fn context(command_id: &str, ctx: &CommandCtx) -> CommandContext {
    CommandContext {
        command_id: command_id.to_string(),
        document_id: ctx.document_id().map(str::to_string),
        args_json: ctx.args().map(Value::to_string),
    }
}

/// A Swift-defined command's hooks. Called on the kernel thread. `derive`
/// and `icon_accent` run on every resolve and must answer from state the
/// implementation already has; a throw falls back to the default (`enabled`
/// and `visible` true, `active` false, no accent). `run` returns at once
/// and answers through `completion`, from any thread.
#[uniffi::export(with_foreign)]
pub trait CommandRunner: Send + Sync {
    fn run(&self, context: CommandContext, completion: Arc<CommandCompletion>);
    /// `hook` is `Enabled`, `Active` or `Visible`.
    fn derive(&self, context: CommandContext, hook: CommandHook) -> Result<bool, HostError>;
    fn icon_accent(&self, context: CommandContext) -> Result<Option<IconAccent>, HostError>;
}

/// The answer to one `CommandRunner::run`. The first call wins; dropping it
/// unanswered fails the command with `operation-failed`.
#[derive(uniffi::Object)]
pub struct CommandCompletion {
    settle: ForeignSettle<Value>,
}

#[uniffi::export]
impl CommandCompletion {
    pub fn finish(&self) {
        self.settle.settle(|| Ok(Value::Null));
    }

    pub fn fail(&self, detail: String) {
        self.settle.settle(move || Err(Error::new(detail).with("code", code::OPERATION_FAILED)));
    }
}

/// A command as Swift defines it.
#[derive(Clone, uniffi::Record)]
pub struct CommandDefinition {
    /// Convention: `domain:verb`.
    pub id: String,
    /// Resolved through i18n when it's installed, else shown as is.
    pub label_key: String,
    #[uniffi(default = None)]
    pub icon: Option<String>,
    /// `Mod+K` style.
    #[uniffi(default = [])]
    pub shortcuts: Vec<String>,
    /// A disabled category hides its commands everywhere.
    #[uniffi(default = [])]
    pub categories: Vec<String>,
    #[uniffi(default = None)]
    pub menu: Option<String>,
    #[uniffi(default = None)]
    pub panel: Option<CommandPanel>,
    #[uniffi(default = None)]
    pub modal: Option<String>,
    #[uniffi(default = None)]
    pub runner: Option<Arc<dyn CommandRunner>>,
    /// The hooks `runner` implements. Empty exactly when `runner` is `None`.
    #[uniffi(default = [])]
    pub hooks: Vec<CommandHook>,
}

impl CommandDefinition {
    /// On the kernel thread.
    pub fn into_def(self, post: KernelPost) -> Result<CommandDef, Error> {
        let CommandDefinition {
            id, label_key, icon, shortcuts, categories, menu, panel, modal, runner, hooks,
        } = self;
        let runner = match (runner, hooks.is_empty()) {
            (None, true) => None,
            (Some(runner), false) => Some(runner),
            (None, false) => {
                return Err(invalid(format!("command '{id}' lists hooks {hooks:?} but has no runner")));
            }
            (Some(_), true) => {
                return Err(invalid(format!("command '{id}' has a runner but lists no hooks")));
            }
        };
        let mut def = CommandDef::new(id.clone(), label_key);
        def.icon = icon;
        def.shortcut = match shortcuts.len() {
            0 => None,
            1 => Some(Shortcut::One(shortcuts.into_iter().next().expect("one shortcut"))),
            _ => Some(Shortcut::Many(shortcuts)),
        };
        def.categories = (!categories.is_empty()).then_some(categories);
        def.menu = menu;
        def.panel = panel.map(|p| match p.exclusive {
            None => Panel::Id(p.id),
            Some(exclusive) => Panel::Target { id: p.id, exclusive: Some(exclusive) },
        });
        def.modal = modal;
        let Some(runner) = runner else { return Ok(def) };
        let derivation = |hook: CommandHook| -> Option<Derivation> {
            hooks.contains(&hook).then(|| {
                let (runner, id) = (runner.clone(), id.clone());
                Rc::new(move |ctx: &CommandCtx| {
                    runner.derive(context(&id, ctx), hook).map_err(Error::from)
                }) as Derivation
            })
        };
        def.enabled = derivation(CommandHook::Enabled);
        def.active = derivation(CommandHook::Active);
        def.visible = derivation(CommandHook::Visible);
        if hooks.contains(&CommandHook::IconAccent) {
            let (runner, id) = (runner.clone(), id.clone());
            def.icon_accent = Some(Rc::new(move |ctx: &CommandCtx| {
                runner.icon_accent(context(&id, ctx)).map_err(Error::from)
            }));
        }
        if hooks.contains(&CommandHook::Run) {
            let (runner, id) = (runner.clone(), id.clone());
            def.run = Some(Rc::new(move |ctx: &CommandCtx| {
                let (promise, resolver) = Promise::pending();
                let completion = Arc::new(CommandCompletion {
                    settle: ForeignSettle::new(resolver, post.clone(), "a CommandRunner completion"),
                });
                runner.run(context(&id, ctx), completion);
                Ok(Some(promise))
            }));
        }
        Ok(def)
    }
}
```

`CommandDef`'s fields are `pub` (`crates/plugins/commands/src/contract.rs`), so this builds it field by field from `CommandDef::new`.

- [x] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-commands --features uniffi --test ffi`
Expected: PASS, 3 tests.

- [x] **Step 7: Commit**

```bash
git add crates/plugins/commands
git commit -m "Add the commands plugin's uniffi definitions and the CommandRunner port"
```

### Task 2: `CommandsHandle` and its events

**Files:**
- Modify: `crates/plugins/commands/src/ffi.rs`
- Test: `crates/plugins/commands/tests/ffi.rs`

**Interfaces:**
- Consumes: Task 1's `CommandDefinition::into_def`.
- Produces: `CommandsHandle::new(cap: CommandsCapability, post: KernelPost) -> Arc<CommandsHandle>` (kernel thread) with async `register_command(definition, replace: bool) -> Arc<RegistrationHandle>`, `register_commands(definitions: Vec<CommandDefinition>, replace: bool) -> Arc<RegistrationHandle>`, `has_command(id) -> bool`, `command(id) -> Option<CommandSummary>`, `command_ids() -> Vec<String>`, `resolve_command(id, document_id: Option<String>) -> Option<ResolvedCommand>`, `commands(document_id) -> Vec<ResolvedCommand>`, `search_commands(query, document_id) -> Vec<ResolvedCommand>`, `shortcuts() -> Vec<CommandShortcutBinding>`, `execute(id, document_id, args_json: Option<String>) -> CommandExecuteResult`, `can_execute(id, document_id) -> bool`, `disabled_categories() -> Vec<String>`, `is_category_disabled(category) -> bool`, `disable_category(category)`, `enable_category(category)`, `set_disabled_categories(categories: Vec<String>)`, `match_stroke(stroke: CommandKeyStroke, is_mac: bool) -> Option<String>`, `menu_target(id) -> Option<CommandMenuTarget>`, and sync `subscribe(listener: Arc<dyn CommandsEventListener>) -> Arc<Subscription>`.

Members, from `crates/plugins/commands/src/harness.rs` `MEMBERS` (20): `registerCommand`, `registerCommands`, `hasCommand`, `getCommand` (`command`), `listCommandIds` (`command_ids`), `resolveCommand`, `listCommands` (`commands`), `searchCommands`, `listShortcuts` (`shortcuts`), `execute`, `canExecute`, `getDisabledCategories` (`disabled_categories`), `isCategoryDisabled`, `disableCategory`, `enableCategory`, `setDisabledCategories`, `onExecuted` and `onExecutionFailed` (`subscribe`), `matchStroke`, `getMenuTarget` (`menu_target`).

- [x] **Step 1: Write the failing handle tests**

Append to `crates/plugins/commands/tests/ffi.rs`:

```rust
use epdf_plugin_commands::ffi::{
    CommandExecuteResult, CommandKeyStroke, CommandRejectReason, CommandsEvent,
    CommandsEventListener, CommandsHandle,
};
use epdf_plugin_commands::{COMMANDS, CommandsConfig, commands_plugin};
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[derive(Default)]
struct Events(Mutex<Vec<CommandsEvent>>);

impl CommandsEventListener for Events {
    fn on_event(&self, event: CommandsEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn handle() -> (KernelThread<TestKernel>, Arc<CommandsHandle>) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(vec![commands_plugin(CommandsConfig::default())], one_document())
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| CommandsHandle::new(k.capability(&COMMANDS).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

fn run_hooks() -> Vec<CommandHook> {
    vec![CommandHook::Run, CommandHook::Enabled, CommandHook::IconAccent]
}

#[test]
fn a_swift_command_runs_with_its_context() {
    let (_thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Ok(true));
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    block_on(commands.register_command(definition("doc:save", Some(runner.clone()), run_hooks()), false)).unwrap();
    let result =
        block_on(commands.execute("doc:save".into(), None, Some(r#"{"copy":true}"#.into()))).unwrap();
    assert_eq!(result, CommandExecuteResult::Executed);
    let ran = runner.ran.lock().unwrap();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].command_id, "doc:save");
    assert_eq!(ran[0].args_json.as_deref(), Some(r#"{"copy":true}"#));
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(CommandsEvent::Executed { event }) if event.command_id == "doc:save"
            && event.args_json.as_deref() == Some(r#"{"copy":true}"#)),
        "{seen:?}"
    );
}

#[test]
fn a_failing_command_rejects_and_reports() {
    let (_thread, commands) = handle();
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    let runner = Runner::new(Run::Fail, Ok(true));
    block_on(commands.register_command(definition("doc:save", Some(runner), run_hooks()), false)).unwrap();
    let error = block_on(commands.execute("doc:save".into(), None, None)).unwrap_err();
    assert_eq!(code(&error), Some("operation-failed"), "{error:?}");
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(CommandsEvent::ExecutionFailed { event }) if event.error.message.contains("disk full")),
        "{seen:?}"
    );
}

#[test]
fn a_dropped_completion_fails_the_command() {
    let (_thread, commands) = handle();
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    let runner = Runner::new(Run::Drop, Ok(true));
    block_on(commands.register_command(definition("doc:save", Some(runner), run_hooks()), false)).unwrap();
    let error = block_on(commands.execute("doc:save".into(), None, None)).unwrap_err();
    assert_eq!(code(&error), Some("operation-failed"), "{error:?}");
    assert!(matches!(events.0.lock().unwrap().last(), Some(CommandsEvent::ExecutionFailed { .. })));
}

#[test]
fn a_disabled_command_is_rejected_without_running() {
    let (_thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Ok(false));
    block_on(commands.register_command(definition("doc:save", Some(runner.clone()), run_hooks()), false)).unwrap();
    let result = block_on(commands.execute("doc:save".into(), None, None)).unwrap();
    assert_eq!(result, CommandExecuteResult::Rejected { reason: CommandRejectReason::Disabled });
    assert!(runner.ran.lock().unwrap().is_empty());
    assert!(!block_on(commands.can_execute("doc:save".into(), None)).unwrap());
    let missing = block_on(commands.execute("nope".into(), None, None)).unwrap();
    assert_eq!(missing, CommandExecuteResult::Rejected { reason: CommandRejectReason::NotFound });
}

#[test]
fn a_throwing_derivation_falls_back() {
    let (_thread, commands) = handle();
    let runner = Runner::new(Run::Finish, Err(()));
    block_on(commands.register_command(definition("doc:save", Some(runner), run_hooks()), false)).unwrap();
    let resolved = block_on(commands.resolve_command("doc:save".into(), None)).unwrap().unwrap();
    assert!(resolved.enabled);
    assert!(resolved.visible);
    assert_eq!(resolved.icon_accent.and_then(|a| a.primary).as_deref(), Some("#f00"));
}

#[test]
fn unparsable_args_are_invalid_input() {
    let (_thread, commands) = handle();
    let events = Arc::new(Events::default());
    let _sub = commands.subscribe(events.clone());
    let runner = Runner::new(Run::Finish, Ok(true));
    block_on(commands.register_command(definition("doc:save", Some(runner.clone()), run_hooks()), false)).unwrap();
    let error = block_on(commands.execute("doc:save".into(), None, Some("{not json".into()))).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
    assert!(runner.ran.lock().unwrap().is_empty());
    assert!(events.0.lock().unwrap().is_empty());
}

#[test]
fn registry_reads_categories_and_shortcuts() {
    let (_thread, commands) = handle();
    let mut zoom = definition("zoom:in", None, vec![]);
    zoom.shortcuts = vec!["Mod+=".into()];
    zoom.categories = vec!["zoom".into()];
    zoom.menu = Some("zoom-menu".into());
    block_on(commands.register_commands(vec![zoom, definition("zoom:out", None, vec![])], false)).unwrap();
    assert_eq!(block_on(commands.command_ids()).unwrap(), ["zoom:in", "zoom:out"]);
    assert!(block_on(commands.has_command("zoom:in".into())).unwrap());
    let summary = block_on(commands.command("zoom:in".into())).unwrap().unwrap();
    assert_eq!(summary.shortcuts, ["Mod+="]);
    assert!(!summary.has_runner);
    let shortcuts = block_on(commands.shortcuts()).unwrap();
    assert_eq!(shortcuts.iter().map(|s| (s.command_id.as_str(), s.shortcut.as_str())).collect::<Vec<_>>(), [("zoom:in", "Mod+=")]);
    let stroke = CommandKeyStroke { key: "=".into(), code: None, ctrl: false, meta: true, alt: false, shift: false };
    assert_eq!(block_on(commands.match_stroke(stroke, true)).unwrap().as_deref(), Some("zoom:in"));
    assert_eq!(block_on(commands.menu_target("zoom:in".into())).unwrap().and_then(|m| m.menu).as_deref(), Some("zoom-menu"));
    block_on(commands.disable_category("zoom".into())).unwrap();
    assert!(block_on(commands.is_category_disabled("zoom".into())).unwrap());
    let found = block_on(commands.search_commands("zoom".into(), None)).unwrap();
    assert_eq!(found.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["zoom:out"]);
    block_on(commands.set_disabled_categories(vec![])).unwrap();
    assert_eq!(block_on(commands.disabled_categories()).unwrap(), Vec::<String>::new());
    block_on(commands.enable_category("zoom".into())).unwrap();
    assert_eq!(block_on(commands.commands(None)).unwrap().len(), 2);
}

#[test]
fn a_duplicate_id_conflicts_unless_replaced() {
    let (_thread, commands) = handle();
    block_on(commands.register_command(definition("zoom:in", None, vec![]), false)).unwrap();
    let error = block_on(commands.register_command(definition("zoom:in", None, vec![]), false)).err().unwrap();
    assert_eq!(code(&error), Some("conflict"), "{error:?}");
    block_on(commands.register_command(definition("zoom:in", None, vec![]), true)).unwrap();
}

#[test]
fn an_unparsable_shortcut_is_refused() {
    let (_thread, commands) = handle();
    let mut bad = definition("zoom:in", None, vec![]);
    bad.shortcuts = vec!["Mod+Hyper+".into()];
    assert!(block_on(commands.register_command(bad, false)).is_err());
}

#[test]
fn unregistering_a_replaced_command_keeps_the_replacement() {
    let (thread, commands) = handle();
    let first = block_on(commands.register_command(definition("zoom:in", None, vec![]), false)).unwrap();
    let mut second = definition("zoom:in", None, vec![]);
    second.icon = Some("plus".into());
    block_on(commands.register_command(second, true)).unwrap();
    first.unregister();
    block_on(thread.call(|_| ())).unwrap();
    let summary = block_on(commands.command("zoom:in".into())).unwrap().unwrap();
    assert_eq!(summary.icon.as_deref(), Some("plus"));
    // A registration dropped without `unregister` leaves its command alone.
    drop(block_on(commands.register_command(definition("zoom:out", None, vec![]), false)).unwrap());
    block_on(thread.call(|_| ())).unwrap();
    assert!(block_on(commands.has_command("zoom:out".into())).unwrap());
}
```

Check `parse_shortcut` (`crates/plugins/ui-core/src/shortcuts.rs`) refuses `Mod+Hyper+` before relying on `an_unparsable_shortcut_is_refused`: an unknown modifier token is the case it errs on. If it accepts it, pick a string the parser's own tests show it rejecting and use that.

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p epdf-plugin-commands --features uniffi --test ffi`
Expected: FAIL to compile with "cannot find type `CommandsHandle` in module `epdf_plugin_commands::ffi`".

- [x] **Step 3: Write the result records, events and handle**

Append to `crates/plugins/commands/src/ffi.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CommandRejectReason {
    NotFound,
    Disabled,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CommandExecuteResult {
    Executed,
    Rejected { reason: CommandRejectReason },
}

impl From<ExecuteResult> for CommandExecuteResult {
    fn from(result: ExecuteResult) -> Self {
        match result {
            ExecuteResult::Executed => CommandExecuteResult::Executed,
            ExecuteResult::Rejected { reason } => CommandExecuteResult::Rejected {
                reason: match reason {
                    RejectReason::NotFound => CommandRejectReason::NotFound,
                    RejectReason::Disabled => CommandRejectReason::Disabled,
                    RejectReason::Hidden => CommandRejectReason::Hidden,
                },
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandShortcutBinding {
    pub command_id: String,
    pub shortcut: String,
}

impl From<ShortcutBinding> for CommandShortcutBinding {
    fn from(binding: ShortcutBinding) -> Self {
        CommandShortcutBinding { command_id: binding.command_id, shortcut: binding.shortcut }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandMenuTarget {
    pub menu: Option<String>,
}

impl From<MenuTarget> for CommandMenuTarget {
    fn from(target: MenuTarget) -> Self {
        CommandMenuTarget { menu: target.menu }
    }
}

/// A key press, as `matchStroke` reads it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandKeyStroke {
    pub key: String,
    #[uniffi(default = None)]
    pub code: Option<String>,
    #[uniffi(default = false)]
    pub ctrl: bool,
    #[uniffi(default = false)]
    pub meta: bool,
    #[uniffi(default = false)]
    pub alt: bool,
    #[uniffi(default = false)]
    pub shift: bool,
}

impl From<CommandKeyStroke> for KeyStroke {
    fn from(stroke: CommandKeyStroke) -> Self {
        KeyStroke {
            key: stroke.key,
            code: stroke.code,
            ctrl_key: stroke.ctrl,
            meta_key: stroke.meta,
            alt_key: stroke.alt,
            shift_key: stroke.shift,
        }
    }
}

/// A registered definition, without its runner.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandSummary {
    pub id: String,
    pub label_key: String,
    pub icon: Option<String>,
    pub shortcuts: Vec<String>,
    pub categories: Vec<String>,
    pub menu: Option<String>,
    pub panel: Option<CommandPanel>,
    pub modal: Option<String>,
    pub has_runner: bool,
}

impl From<CommandDef> for CommandSummary {
    fn from(def: CommandDef) -> Self {
        CommandSummary {
            id: def.id,
            label_key: def.label_key,
            icon: def.icon,
            shortcuts: match def.shortcut {
                None => vec![],
                Some(Shortcut::One(s)) => vec![s],
                Some(Shortcut::Many(list)) => list,
            },
            categories: def.categories.unwrap_or_default(),
            menu: def.menu,
            panel: def.panel.map(|panel| match panel {
                Panel::Id(id) => CommandPanel { id, exclusive: None },
                Panel::Target { id, exclusive } => CommandPanel { id, exclusive },
            }),
            modal: def.modal,
            has_runner: def.run.is_some()
                || def.enabled.is_some()
                || def.active.is_some()
                || def.visible.is_some()
                || def.icon_accent.is_some(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandExecuted {
    pub command_id: String,
    pub document_id: Option<String>,
    pub args_json: Option<String>,
}

impl From<&CommandExecutedEvent> for CommandExecuted {
    fn from(event: &CommandExecutedEvent) -> Self {
        CommandExecuted {
            command_id: event.command_id.clone(),
            document_id: event.document_id.clone(),
            args_json: event.args.as_ref().map(Value::to_string),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommandExecutionFailure {
    pub command_id: String,
    pub document_id: Option<String>,
    pub error: PluginErrorInfo,
}

impl TryFrom<&CommandExecutionFailedEvent> for CommandExecutionFailure {
    type Error = Error;

    fn try_from(event: &CommandExecutionFailedEvent) -> Result<Self, Error> {
        let field = |key: &str| {
            event.error.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| {
                Error::named("InvalidState", format!("[commands] failure without \"{key}\""))
                    .with("code", "invalid-state")
            })
        };
        Ok(CommandExecutionFailure {
            command_id: event.command_id.clone(),
            document_id: event.document_id.clone(),
            error: PluginErrorInfo {
                code: field("code")?,
                message: field("message")?,
                capability: field("capability")?,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CommandsEvent {
    Executed { event: CommandExecuted },
    ExecutionFailed { event: CommandExecutionFailure },
}

/// Workspace events: the stream lasts as long as the viewer.
#[uniffi::export(with_foreign)]
pub trait CommandsEventListener: Send + Sync {
    fn on_event(&self, event: CommandsEvent);
}

// A `register_command` or `register_commands` call answers with an
// `epdf_ffi::RegistrationHandle` (umbrella Task 0c). Dropping it keeps the
// commands; `unregister` removes them, each only while it is still the
// definition registered under its id.

fn args(args_json: Option<String>) -> Result<Option<Value>, Error> {
    args_json
        .map(|json| {
            serde_json::from_str(&json).map_err(|e| invalid(format!("command args are not JSON: {e}")))
        })
        .transpose()
}

#[derive(uniffi::Object)]
pub struct CommandsHandle {
    cap: KernelBound<CommandsCapability>,
    post: KernelPost,
}

impl CommandsHandle {
    /// On the kernel thread.
    pub fn new(cap: CommandsCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(CommandsHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl CommandsHandle {
    /// A duplicate id throws `conflict` unless `replace`; an unparsable
    /// shortcut throws too.
    pub async fn register_command(
        &self,
        definition: CommandDefinition,
        replace: bool,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let def = definition.into_def(post.clone())?;
                let remove = c.register_command(def, RegisterCommandOptions { replace })?;
                Ok(RegistrationHandle::new(post, move || remove()))
            })
            .await
    }

    /// In order; a failure stops there, with the earlier ones registered.
    pub async fn register_commands(
        &self,
        definitions: Vec<CommandDefinition>,
        replace: bool,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let defs = definitions
                    .into_iter()
                    .map(|d| d.into_def(post.clone()))
                    .collect::<Result<Vec<_>, _>>()?;
                let remove = c.register_commands(defs, RegisterCommandOptions { replace })?;
                Ok(RegistrationHandle::new(post, move || remove()))
            })
            .await
    }

    pub async fn has_command(&self, id: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.has_command(&id))).await
    }

    pub async fn command(&self, id: String) -> Result<Option<CommandSummary>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_command(&id).map(CommandSummary::from))).await
    }

    pub async fn command_ids(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(c.list_command_ids())).await
    }

    /// Every derivation runs on every call.
    pub async fn resolve_command(
        &self,
        id: String,
        document_id: Option<String>,
    ) -> Result<Option<ResolvedCommand>, ViewerError> {
        self.cap.call(move |c| Ok(c.resolve_command(&id, document_id.as_deref()))).await
    }

    pub async fn commands(
        &self,
        document_id: Option<String>,
    ) -> Result<Vec<ResolvedCommand>, ViewerError> {
        self.cap.call(move |c| Ok(c.list_commands(document_id.as_deref()))).await
    }

    /// Visible commands whose label or id contains the trimmed, lowercased
    /// query.
    pub async fn search_commands(
        &self,
        query: String,
        document_id: Option<String>,
    ) -> Result<Vec<ResolvedCommand>, ViewerError> {
        self.cap.call(move |c| Ok(c.search_commands(&query, document_id.as_deref()))).await
    }

    pub async fn shortcuts(&self) -> Result<Vec<CommandShortcutBinding>, ViewerError> {
        self.cap
            .call(|c| Ok(c.list_shortcuts().into_iter().map(CommandShortcutBinding::from).collect()))
            .await
    }

    /// `args_json` that isn't JSON throws `invalid-input` before anything
    /// runs.
    pub async fn execute(
        &self,
        id: String,
        document_id: Option<String>,
        args_json: Option<String>,
    ) -> Result<CommandExecuteResult, ViewerError> {
        self.cap
            .call_promise(
                move |c| Ok(c.execute(&id, document_id.as_deref(), args(args_json)?)),
                CommandExecuteResult::from,
            )
            .await
    }

    pub async fn can_execute(
        &self,
        id: String,
        document_id: Option<String>,
    ) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.can_execute(&id, document_id.as_deref()))).await
    }

    pub async fn disabled_categories(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(c.get_disabled_categories())).await
    }

    pub async fn is_category_disabled(&self, category: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.is_category_disabled(&category))).await
    }

    pub async fn disable_category(&self, category: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(c.disable_category(&category))).await
    }

    pub async fn enable_category(&self, category: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(c.enable_category(&category))).await
    }

    pub async fn set_disabled_categories(
        &self,
        categories: Vec<String>,
    ) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(c.set_disabled_categories(&categories))).await
    }

    /// The first command with a matching shortcut, enabled or not.
    pub async fn match_stroke(
        &self,
        stroke: CommandKeyStroke,
        is_mac: bool,
    ) -> Result<Option<String>, ViewerError> {
        self.cap
            .call(move |c| Ok(c.match_stroke(&stroke.into(), ShortcutOptions { is_mac })))
            .await
    }

    pub async fn menu_target(&self, id: String) -> Result<Option<CommandMenuTarget>, ViewerError> {
        self.cap.call(move |c| Ok(c.get_menu_target(&id).map(CommandMenuTarget::from))).await
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect.
    pub fn subscribe(&self, listener: Arc<dyn CommandsEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let executed = {
                let listener = listener.clone();
                c.on_executed(move |event| {
                    listener.on_event(CommandsEvent::Executed { event: event.into() });
                    Ok(())
                })
            };
            let failed = c.on_execution_failed(move |event| {
                let event = CommandExecutionFailure::try_from(event)?;
                listener.on_event(CommandsEvent::ExecutionFailed { event });
                Ok(())
            });
            slot.fill(Unsubscribe::new(move || {
                executed.call();
                failed.call();
            }));
        });
        subscription
    }
}
```

`execute` returns a `Promise` whose rejection carries the plugin error the `executionFailed` event saw (`controller.rs` `execute`), so the Rust future answers `operation-failed` for a runner that called `fail`.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-plugin-commands --features uniffi --test ffi`
Expected: PASS, 13 tests.

Run: `cargo test -p epdf-plugin-commands`
Expected: the conformance vectors still pass without `uniffi`.

- [x] **Step 5: Commit**

```bash
git add crates/plugins/commands
git commit -m "Export the commands plugin through uniffi"
```

### Task 3: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`
- Modify: `crates/epdf-viewer-host/src/api.rs`
- Modify: `crates/epdf-viewer-host/src/session.rs`
- Modify: `crates/epdf-viewer-host/src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `epdf_plugin_commands::ffi::CommandsHandle`; plan 13's `ViewerConfig.i18n` and `epdf_plugin_i18n::ffi::{I18nConfig, LocalePack}`.
- Produces: `ViewerConfig.commands: bool` and `ViewerHost::commands() -> Result<Arc<CommandsHandle>, ViewerError>`.

- [x] **Step 1: Write the failing host tests**

Append to `crates/epdf-viewer-host/tests/viewer_host.rs` (the `I18nConfig` and `LocalePack` imports and `en_pack` may already exist from plan 13; reuse them):

```rust
use epdf_plugin_commands::ffi::{CommandDefinition, CommandExecuteResult};

fn routing_command(id: &str, label_key: &str) -> CommandDefinition {
    CommandDefinition {
        id: id.into(),
        label_key: label_key.into(),
        icon: None,
        shortcuts: vec![],
        categories: vec![],
        menu: None,
        panel: None,
        modal: None,
        runner: None,
        hooks: vec![],
    }
}

#[test]
fn the_commands_plugin_registers_and_resolves() {
    let config = ViewerConfig { commands: true, ..ViewerConfig::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let commands = block_on(host.commands()).unwrap();
    block_on(commands.register_command(routing_command("zoom:in", "commands.zoomIn"), false)).unwrap();
    let resolved = block_on(commands.resolve_command("zoom:in".into(), None)).unwrap().unwrap();
    // No i18n: the label is the key.
    assert_eq!(resolved.label, "commands.zoomIn");
    let result = block_on(commands.execute("zoom:in".into(), None, None)).unwrap();
    assert_eq!(result, CommandExecuteResult::Executed);
}

#[test]
fn without_its_flag_the_commands_plugin_is_not_installed() {
    let host = started();
    assert!(block_on(host.commands()).is_err());
}

#[test]
fn a_command_label_resolves_through_i18n() {
    let pack = LocalePack {
        code: "en".into(),
        name: "English".into(),
        dir: None,
        translations: [("commands.zoomIn".to_string(), "Zoom in".to_string())].into(),
    };
    let config = ViewerConfig {
        commands: true,
        i18n: Some(I18nConfig {
            locale: None,
            fallback_locale: None,
            locales: vec![pack],
            lazy_locales: vec![],
            loader: None,
        }),
        ..ViewerConfig::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let commands = block_on(host.commands()).unwrap();
    block_on(commands.register_command(routing_command("zoom:in", "commands.zoomIn"), false)).unwrap();
    let resolved = block_on(commands.resolve_command("zoom:in".into(), None)).unwrap().unwrap();
    assert_eq!(resolved.label, "Zoom in");
}
```

If plan 12 shipped `ViewerConfig.shell`, also add this test, using the field and accessor names plan 12's row in the umbrella's Cross-plan names table gives (`shell: bool`, `ViewerHost::shell(document_id)`, `ShellHandle::is_open`); check plan 12's merged code for the exact `is_open` signature first:

```rust
use epdf_plugin_commands::ffi::CommandPanel;

#[test]
fn a_panel_command_toggles_the_shell_surface() {
    let config = ViewerConfig { commands: true, shell: true, ..ViewerConfig::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let commands = block_on(host.commands()).unwrap();
    let mut toggle = routing_command("panel:search", "commands.search");
    toggle.panel = Some(CommandPanel { id: "search".into(), exclusive: None });
    block_on(commands.register_command(toggle, false)).unwrap();
    block_on(commands.execute("panel:search".into(), Some(doc.id.clone()), None)).unwrap();
    let shell = block_on(host.shell(doc.id)).unwrap();
    assert!(block_on(shell.is_open("search".into())).unwrap());
}
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host commands`
Expected: FAIL to compile with "unresolved import `epdf_plugin_commands`".

- [x] **Step 3: Wire the plugin in**

`crates/epdf-viewer-host/Cargo.toml`, alphabetically:

```toml
epdf-plugin-commands = { path = "../plugins/commands", default-features = false, features = ["uniffi"] }
```

`crates/epdf-viewer-host/src/api.rs`, at the end of `ViewerConfig` (after plan 13's `i18n`, keeping plan order):

```rust
    /// Install `commandsPlugin()`: the workspace command registry. Register
    /// definitions through `ViewerHost::commands()`.
    #[uniffi(default = false)]
    pub commands: bool,
```

`crates/epdf-viewer-host/src/session.rs`, after the i18n block:

```rust
        if config.commands {
            plugins.push(epdf_plugin_commands::commands_plugin(
                epdf_plugin_commands::CommandsConfig::default(),
            ));
        }
```

`crates/epdf-viewer-host/src/lib.rs`: add `use epdf_plugin_commands::ffi::CommandsHandle;` and, after `i18n`:

```rust
    /// The workspace command registry. Needs `ViewerConfig.commands`.
    pub async fn commands(&self) -> Result<Arc<CommandsHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_commands::COMMANDS, None)?;
            Ok(CommandsHandle::new(cap, post))
        })
        .await
    }
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS, including the new tests.

- [x] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Install the commands plugin from ViewerConfig and expose ViewerHost.commands"
```

### Task 4: Swift capability and `CommandHandler`

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Commands.swift`
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/CommandsTests.swift`

**Interfaces:**
- Consumes: generated `CommandsHandle`, `CommandDefinition`, `CommandRunner`, `CommandCompletion`, `CommandHook`, `CommandsEvent` and the result records.
- Produces: Swift `CommandsCapability` (one method per handle member, with `execute(_:document:args:)` taking `Data?`), `PDFViewer.commands()`, `CommandHandler` (a closure-backed `CommandRunner`), and `CommandDefinition.init(id:labelKey:icon:shortcuts:categories:menu:panel:modal:handler:)` (handler required).

- [ ] **Step 1: Write the failing Swift tests**

`platforms/apple/EmbedPDF/Tests/EmbedPDFTests/CommandsTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct CommandsTests {
    func started() async throws -> (PDFViewer, CommandsCapability) {
      let viewer = try PDFViewer(ViewerConfig(commands: true))
      try await viewer.start()
      return (viewer, try await viewer.commands())
    }

    @Test func aSwiftCommandRunsAsync() async throws {
      let (_, commands) = try await started()
      let ran = LockIsolated<[String]>([])
      let handler = CommandHandler(run: { context in
        try await Task.sleep(for: .milliseconds(10))
        ran.withValue { $0.append(context.commandId) }
      })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      let events = commands.events
      let result = try await commands.execute("doc:save", args: Data(#"{"copy":true}"#.utf8))
      expectNoDifference(result, .executed)
      expectNoDifference(ran.value, ["doc:save"])
      for await event in events {
        guard case let .executed(event) = event else { continue }
        expectNoDifference(event.argsJson, #"{"copy":true}"#)
        break
      }
    }

    @Test func aThrowingCommandFails() async throws {
      let (_, commands) = try await started()
      struct Full: Error {}
      let handler = CommandHandler(run: { _ in throw Full() })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      await #expect {
        _ = try await commands.execute("doc:save")
      } throws: { error in
        (error as? PDFError)?.code == "operation-failed"
      }
    }

    @Test func aDerivationGatesExecution() async throws {
      let (_, commands) = try await started()
      let handler = CommandHandler(run: { _ in }, enabled: { _ in false })
      _ = try await commands.register(CommandDefinition(id: "doc:save", labelKey: "Save", handler: handler))
      let result = try await commands.execute("doc:save")
      expectNoDifference(result, .rejected(reason: .disabled))
      let canExecute = try await commands.canExecute("doc:save")
      expectNoDifference(canExecute, false)
    }

    @Test func aShortcutMatches() async throws {
      let (_, commands) = try await started()
      _ = try await commands.register(
        CommandDefinition(id: "zoom:in", labelKey: "Zoom in", shortcuts: ["Mod+="])
      )
      let matched = try await commands.matchStroke(CommandKeyStroke(key: "=", meta: true), isMac: true)
      expectNoDifference(matched, "zoom:in")
    }

    @Test func unregisterRemovesTheCommand() async throws {
      let (_, commands) = try await started()
      let registration = try await commands.register(CommandDefinition(id: "zoom:in", labelKey: "Zoom in"))
      registration.unregister()
      let has = try await commands.hasCommand("zoom:in")
      expectNoDifference(has, false)
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, commands) = try await started()
      _ = try await commands.register(CommandDefinition(id: "zoom:in", labelKey: "Zoom in"))
      let task = Task { for await _ in commands.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await commands.execute("zoom:in")
    }

    @Test func aDestroyedViewersCapabilityThrows() async throws {
      let (viewer, commands) = try await started()
      try await viewer.destroy()
      await #expect {
        _ = try await commands.commandIds()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }
  }
}

/// A value shared between a test and a `@Sendable` closure.
final class LockIsolated<Value>: @unchecked Sendable {
  private let lock = NSLock()
  private var _value: Value
  init(_ value: Value) { _value = value }
  var value: Value { lock.withLock { _value } }
  func withValue(_ body: (inout Value) -> Void) { lock.withLock { body(&_value) } }
}
```

`unregisterRemovesTheCommand` relies on `unregister()`'s fire job running before `hasCommand`'s, which the kernel thread's FIFO queue guarantees. commands is workspace-scoped, so `aClosedDocumentsCapabilityThrows` doesn't apply, and no member takes a `CancelToken`.

If another test file already defines `LockIsolated`, delete the copy here.

- [ ] **Step 2: Rebuild and run the tests to verify they fail**

```bash
bash platforms/apple/build-xcframework.sh
cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/CommandsTests
```

Expected: FAIL to compile with "cannot find type 'CommandsCapability' in scope".

- [ ] **Step 3: Write `Plugins/Commands.swift`**

```swift
public import EmbedPDFBindings
import Foundation

public typealias CommandDefinition = EmbedPDFBindings.CommandDefinition
public typealias CommandHook = EmbedPDFBindings.CommandHook
public typealias CommandPanel = EmbedPDFBindings.CommandPanel
public typealias CommandContext = EmbedPDFBindings.CommandContext
public typealias CommandRunner = EmbedPDFBindings.CommandRunner
public typealias CommandCompletion = EmbedPDFBindings.CommandCompletion
public typealias CommandSummary = EmbedPDFBindings.CommandSummary
public typealias ResolvedCommand = EmbedPDFBindings.ResolvedCommand
public typealias IconAccent = EmbedPDFBindings.IconAccent
public typealias CommandExecuteResult = EmbedPDFBindings.CommandExecuteResult
public typealias CommandRejectReason = EmbedPDFBindings.CommandRejectReason
public typealias CommandShortcutBinding = EmbedPDFBindings.CommandShortcutBinding
public typealias CommandMenuTarget = EmbedPDFBindings.CommandMenuTarget
public typealias CommandKeyStroke = EmbedPDFBindings.CommandKeyStroke
public typealias CommandsEvent = EmbedPDFBindings.CommandsEvent
public typealias CommandExecuted = EmbedPDFBindings.CommandExecuted
public typealias CommandExecutionFailure = EmbedPDFBindings.CommandExecutionFailure

/// A `CommandRunner` from closures. `enabled`, `active`, `visible` and
/// `iconAccent` run on the viewer's kernel thread on every resolve: answer
/// from state you already have, and never wait on the main actor there.
public final class CommandHandler: CommandRunner, Sendable {
  let runBody: (@Sendable (CommandContext) async throws -> Void)?
  let enabled: (@Sendable (CommandContext) -> Bool)?
  let active: (@Sendable (CommandContext) -> Bool)?
  let visible: (@Sendable (CommandContext) -> Bool)?
  let accent: (@Sendable (CommandContext) -> IconAccent?)?

  public init(
    run: (@Sendable (CommandContext) async throws -> Void)? = nil,
    enabled: (@Sendable (CommandContext) -> Bool)? = nil,
    active: (@Sendable (CommandContext) -> Bool)? = nil,
    visible: (@Sendable (CommandContext) -> Bool)? = nil,
    iconAccent: (@Sendable (CommandContext) -> IconAccent?)? = nil
  ) {
    self.runBody = run
    self.enabled = enabled
    self.active = active
    self.visible = visible
    self.accent = iconAccent
  }

  /// The hooks this handler implements, for `CommandDefinition.hooks`.
  public var hooks: [CommandHook] {
    var hooks: [CommandHook] = []
    if runBody != nil { hooks.append(.run) }
    if enabled != nil { hooks.append(.enabled) }
    if active != nil { hooks.append(.active) }
    if visible != nil { hooks.append(.visible) }
    if accent != nil { hooks.append(.iconAccent) }
    return hooks
  }

  public func run(context: CommandContext, completion: CommandCompletion) {
    guard let runBody else {
      completion.finish()
      return
    }
    Task {
      do {
        try await runBody(context)
        completion.finish()
      } catch {
        completion.fail(detail: "\(error)")
      }
    }
  }

  public func derive(context: CommandContext, hook: CommandHook) throws -> Bool {
    switch hook {
    case .enabled: enabled?(context) ?? true
    case .active: active?(context) ?? false
    case .visible: visible?(context) ?? true
    case .run, .iconAccent: true
    }
  }

  public func iconAccent(context: CommandContext) throws -> IconAccent? {
    accent?(context)
  }
}

extension CommandDefinition {
  /// A definition whose hooks come from `handler`. A routing command
  /// (menu, panel or modal, no handler) uses the generated initializer.
  /// `handler` has no default so the two never compete for the same call.
  public init(
    id: String,
    labelKey: String,
    icon: String? = nil,
    shortcuts: [String] = [],
    categories: [String] = [],
    menu: String? = nil,
    panel: CommandPanel? = nil,
    modal: String? = nil,
    handler: CommandHandler
  ) {
    self.init(
      id: id, labelKey: labelKey, icon: icon, shortcuts: shortcuts, categories: categories,
      menu: menu, panel: panel, modal: modal, runner: handler, hooks: handler.hooks
    )
  }
}

/// The workspace command registry. Get it with `PDFViewer.commands()`; the
/// viewer needs `ViewerConfig(commands: true)`.
public struct CommandsCapability: Sendable {
  let handle: CommandsHandle
  let viewer: PDFViewer

  /// Throws `conflict` for a duplicate id unless `replace`. Dropping the
  /// registration keeps the command; `unregister()` removes it.
  public func register(_ definition: CommandDefinition, replace: Bool = false)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerCommand(definition: definition, replace: replace) })
  }

  public func register(_ definitions: [CommandDefinition], replace: Bool = false)
    async throws(PDFError) -> Registration
  {
    Registration(handle: try await bridged { try await handle.registerCommands(definitions: definitions, replace: replace) })
  }

  public func hasCommand(_ id: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasCommand(id: id) }
  }

  public func command(_ id: String) async throws(PDFError) -> CommandSummary? {
    try await bridged { try await handle.command(id: id) }
  }

  public func commandIds() async throws(PDFError) -> [String] {
    try await bridged { try await handle.commandIds() }
  }

  public func resolve(_ id: String, document: PDFDocument? = nil) async throws(PDFError) -> ResolvedCommand? {
    try await bridged { try await handle.resolveCommand(id: id, documentId: document?.id) }
  }

  public func commands(document: PDFDocument? = nil) async throws(PDFError) -> [ResolvedCommand] {
    try await bridged { try await handle.commands(documentId: document?.id) }
  }

  public func search(_ query: String, document: PDFDocument? = nil) async throws(PDFError) -> [ResolvedCommand] {
    try await bridged { try await handle.searchCommands(query: query, documentId: document?.id) }
  }

  public func shortcuts() async throws(PDFError) -> [CommandShortcutBinding] {
    try await bridged { try await handle.shortcuts() }
  }

  /// `args` is JSON, handed back to the command's `run` and to the
  /// `executed` event. Throws `invalid-input` when it isn't valid UTF-8
  /// JSON. Cancelling the task doesn't cancel the command.
  public func execute(_ id: String, document: PDFDocument? = nil, args: Data? = nil)
    async throws(PDFError) -> CommandExecuteResult
  {
    let json = args.map { String(decoding: $0, as: UTF8.self) }
    return try await bridged { try await handle.execute(id: id, documentId: document?.id, argsJson: json) }
  }

  public func canExecute(_ id: String, document: PDFDocument? = nil) async throws(PDFError) -> Bool {
    try await bridged { try await handle.canExecute(id: id, documentId: document?.id) }
  }

  public func disabledCategories() async throws(PDFError) -> [String] {
    try await bridged { try await handle.disabledCategories() }
  }

  public func isCategoryDisabled(_ category: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.isCategoryDisabled(category: category) }
  }

  public func disableCategory(_ category: String) async throws(PDFError) {
    try await bridged { try await handle.disableCategory(category: category) }
  }

  public func enableCategory(_ category: String) async throws(PDFError) {
    try await bridged { try await handle.enableCategory(category: category) }
  }

  public func setDisabledCategories(_ categories: [String]) async throws(PDFError) {
    try await bridged { try await handle.setDisabledCategories(categories: categories) }
  }

  public func matchStroke(_ stroke: CommandKeyStroke, isMac: Bool) async throws(PDFError) -> String? {
    try await bridged { try await handle.matchStroke(stroke: stroke, isMac: isMac) }
  }

  public func menuTarget(_ id: String) async throws(PDFError) -> CommandMenuTarget? {
    try await bridged { try await handle.menuTarget(id: id) }
  }

  /// Finishes when the viewer is destroyed.
  public var events: AsyncStream<CommandsEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: CommandsEventListener where Event == CommandsEvent {
  func onEvent(event: CommandsEvent) {
    continuation.yield(event)
  }
}

extension PDFViewer {
  public func commands() async throws(PDFError) -> CommandsCapability {
    let handle = try await bridged { try await host.commands() }
    return CommandsCapability(handle: handle, viewer: self)
  }
}
```

The generated `CommandRunner` protocol's methods are `run(context:completion:)`, `derive(context:hook:) throws -> Bool` and `iconAccent(context:) throws -> IconAccent?`; if the generated spelling differs (check `Sources/EmbedPDFBindings/epdf_plugin_commands.swift` after the build), match it.

- [ ] **Step 4: Rebuild and run the tests to verify they pass**

```bash
bash platforms/apple/build-xcframework.sh
cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/CommandsTests
```

Expected: PASS, 7 tests.

- [ ] **Step 5: Commit**

```bash
git add platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Commands.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests/CommandsTests.swift
git commit -m "Add EmbedPDF's CommandsCapability and a closure-backed CommandHandler"
```

### Task 5: Docs, CI, merge and cleanup

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`
- Modify: `platforms/apple/README.md`
- Modify: `docs/superpowers/plans/2026-10-01-swift-package.md`

- [ ] **Step 1: Add the crate to CI**

Append `commands` to the `crates=( ... )` list in `.github/workflows/plugin-rust.yml`, in plan order.

- [ ] **Step 2: Add the README row**

```markdown
| commands | `CommandsCapability`, `CommandHandler` | `commands: true` | `2026-10-06-swift-plugins/14-commands.md` |
```

- [ ] **Step 3: Mark the spec entry delivered**

In `docs/superpowers/plans/2026-10-01-swift-package.md` Appendix A §A.4, change `**commands** (W, optional `i18n`, `shell`).` to `**commands** (W, optional `i18n`, `shell`), delivered by 2026-10-06-swift-plugins/14-commands.md.` and leave the rest of the entry alone.

- [ ] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-commands --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-commands -p epdf-viewer-host --all-targets --features epdf-plugin-commands/uniffi -- -D warnings
cargo test -p epdf-plugin-commands
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
stat -f%z target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
```

Expected: every command passes.

- [ ] **Step 5: Commit, with the archive sizes**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "Test the commands uniffi exports in CI and document the Swift capability

libepdf_viewer_host.a (aarch64-apple-ios-sim, release): <before> -> <after> bytes"
```

Add a line saying whether `a_panel_command_toggles_the_shell_surface` was added or skipped (plan 12's decision).

- [ ] **Step 6: Merge into the integration branch**

From `.claude/worktrees/swift-plugins-integration`:

```bash
git merge --no-ff research/swift-commands -m "Merge the commands Swift wrapper"
```

Resolve shared-file conflicts by keeping both blocks in plan-number order.

- [ ] **Step 7: Rerun on the integration branch**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

- [ ] **Step 8: Remove the worktree and branch**

```bash
git worktree remove .claude/worktrees/swift-commands
git branch -d research/swift-commands
git worktree prune
```

No `--force`. Then tick `14 commands` in the umbrella.
