//! The typed API beyond what the vectors print, against
//! `packages/plugin/commands/src/{controller,registry}.ts`,
//! `services/availability.ts` and `write/turning-off.ts`: settings changes
//! that change nothing, refusals by category, id and permission, the label
//! fallbacks, the turns `execute` settles on, a run that completes later,
//! cancellation, a walk that changes the registry under it, the identity of
//! every memoized read, families, the keystroke matcher, a focused panel,
//! the scoped view and what survives the kernel.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::rt::Resolver;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{
    AnyPlugin, Error, Kernel, KernelOptions, OpenOptions, OpenSource, Promise, Unsubscribe, rt,
};
use epdf_plugin_commands::*;
use epdf_plugin_i18n::{I18nConfig, Locale, i18n_plugin_warning_to};
use epdf_plugin_shell::{SHELL, shell_plugin};
use epdf_ui_core::shortcuts::KeyStroke;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use plugin_conformance::test_kernel::settle;
use serde_json::{Value, json};

struct Rig {
    kernel: Kernel,
    commands: CommandsCapability,
    reports: Rc<RefCell<Vec<String>>>,
}

/// A kernel over the fake engine with `documents` (their security as the
/// spec gives it), the commands plugin with `config`, and `extra` plugins.
fn rig_with(config: Option<CommandsConfig>, extra: Vec<AnyPlugin>, documents: Value) -> Rig {
    rt::reset();
    let reports: Rc<RefCell<Vec<String>>> = Rc::default();
    let heard = reports.clone();
    let mut plugins = extra;
    plugins.push(commands_plugin(config));
    let kernel = Kernel::new(
        Rc::new(AsEngine(FakeEngine::new(json!({ "documents": documents })))),
        plugins,
        KernelOptions {
            report: Some(Rc::new(move |error: &Error| {
                heard.borrow_mut().push(error.message.clone())
            })),
            ..KernelOptions::default()
        },
    )
    .expect("a kernel");
    settle(&kernel.start()).expect("started");
    let commands = kernel.capability(&COMMANDS, None).expect("the commands");
    Rig { kernel, commands, reports }
}

fn rig(config: CommandsConfig) -> Rig {
    rig_with(Some(config), Vec::new(), json!({}))
}

fn document(allows: Value) -> Value {
    json!({ "pages": [], "security": { "allows": allows } })
}

impl Rig {
    fn open(&self, id: &str) {
        let opened = self.kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": id, "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        settle(&opened).expect("opened");
    }

    fn resolve(&self, id: &str) -> Rc<ResolvedCommand> {
        self.commands.resolve_command(id, None).unwrap().expect("a command")
    }
}

/// An i18n plugin with English app strings, its warnings kept in `warnings`.
fn i18n_with(translations: Value, warnings: &Rc<RefCell<Vec<String>>>) -> AnyPlugin {
    let warned = warnings.clone();
    i18n_plugin_warning_to(
        I18nConfig {
            locale: Some(Some("en".into())),
            locales: Some(vec![Locale {
                code: "en".into(),
                name: "English".into(),
                direction: None,
                translations: serde_json::from_value(translations).expect("a dictionary"),
            }]),
            ..I18nConfig::default()
        },
        Rc::new(move |message: &str| warned.borrow_mut().push(message.to_string())),
    )
}

fn def(id: &str) -> CommandDef {
    CommandDef { id: id.into(), ..CommandDef::default() }
}

fn derivation(answer: bool) -> Derivation {
    Rc::new(move |_| Ok(answer))
}

fn config(defs: Vec<CommandDef>) -> CommandsConfig {
    CommandsConfig {
        commands: Some(defs.into_iter().map(CommandItem::from).collect()),
        ..CommandsConfig::default()
    }
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn settings_events(rig: &Rig) -> (Rc<RefCell<Vec<Vec<String>>>>, Unsubscribe) {
    let events: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let log = events.clone();
    let off = rig.commands.on_settings_changed(move |event| {
        log.borrow_mut().push(event.changed.clone());
        Ok(())
    });
    (events, off)
}

#[test]
fn a_settings_change_that_changes_nothing_fires_nothing() {
    let run: RunFn = Rc::new(|_| Ok(None));
    let first = Rc::new(CommandDef { run: Some(run.clone()), ..def("save") });
    let rig = rig(CommandsConfig {
        commands: Some(vec![CommandItem::Command(first.clone())]),
        disabled_categories: Some(strings(&["pro"])),
        ..CommandsConfig::default()
    });
    let (events, _off) = settings_events(&rig);
    let settings = rig.commands.get_settings();
    let categories = rig.commands.get_disabled_categories();

    // A name already off, a name not off, an equal list, the registered
    // settings: no change, no event, the same objects.
    rig.commands.disable_category("pro");
    rig.commands.enable_category("beta");
    rig.commands.set_disabled_categories(&strings(&["pro"]));
    rig.commands.enable_command("save");
    rig.commands.reset_settings();
    // A definition equal by value (its data and the same function) is the
    // same setting, and reads back as the definition already there.
    let equal = Rc::new(CommandDef { run: Some(run), ..def("save") });
    rig.commands.update_settings(&CommandsConfig {
        commands: Some(vec![CommandItem::Command(equal)]),
        ..CommandsConfig::default()
    });
    assert!(events.borrow().is_empty());
    assert!(Rc::ptr_eq(&settings, &rig.commands.get_settings()));
    assert!(Rc::ptr_eq(&categories, &rig.commands.get_disabled_categories()));
    assert!(Rc::ptr_eq(&rig.commands.get_command("save").unwrap().unwrap(), &first));

    // A change fires once, with the settings it changed; the members it
    // left keep their objects.
    rig.commands.disable_command("save");
    assert_eq!(*events.borrow(), [strings(&["disabledCommands"])]);
    assert!(Rc::ptr_eq(&categories, &rig.commands.get_disabled_categories()));
    // A definition with another function is another setting.
    rig.commands.update_settings(&config(vec![CommandDef {
        run: Some(Rc::new(|_| Ok(None))),
        ..def("save")
    }]));
    rig.commands.update_settings(&CommandsConfig {
        not_allowed: Some(NotAllowedMode::Disable),
        disabled_categories: Some(Vec::new()),
        ..CommandsConfig::default()
    });
    // Reset goes back to what the app registered, once.
    rig.commands.reset_settings();
    rig.commands.reset_settings();
    assert_eq!(
        *events.borrow(),
        [
            strings(&["disabledCommands"]),
            strings(&["commands"]),
            strings(&["disabledCategories", "notAllowed"]),
            strings(&["commands", "disabledCategories", "disabledCommands", "notAllowed"]),
        ]
    );
    assert!(Rc::ptr_eq(&rig.commands.get_command("save").unwrap().unwrap(), &first));
    assert_eq!(*rig.commands.get_disabled_categories(), ["pro"]);
    assert!(rig.reports.borrow().is_empty());
}

/// The vector `a-bad-config-refuses-the-kernel` lists one id for two
/// configured definitions but can't tell which won: the later one, in the
/// earlier one's place (`configuredCommands`, a `Map`). Construction is not
/// refused, and the runtime registry still refuses a taken id.
#[test]
fn a_configured_duplicate_is_the_later_definition_in_the_first_place() {
    let rig = rig(config(vec![
        CommandDef { label: Some("first".into()), ..def("a") },
        def("b"),
        CommandDef { label: Some("second".into()), ..def("a") },
    ]));
    assert_eq!(*rig.commands.list_command_ids().unwrap(), ["a", "b"]);
    assert_eq!(rig.resolve("a").label, "second");
    let taken = rig.commands.register_command(def("a"), RegisterCommandOptions::default());
    assert_eq!(
        taken.err().and_then(|e| e.plugin_code().map(str::to_string)).as_deref(),
        Some("conflict")
    );
}

#[test]
fn refusals_follow_categories_ids_and_permissions() {
    let warnings = Rc::default();
    let rig = rig_with(
        Some(config(vec![
            CommandDef { requires: Some(strings(&["doc.print"])), ..def("print") },
            CommandDef { requires: Some(strings(&["annotations:create"])), ..def("draw") },
            CommandDef {
                allowed: Some(derivation(false)),
                not_allowed_key: Some("commands.notAllowed.doc-download".into()),
                ..def("download")
            },
            CommandDef { categories: Some(strings(&["annotations.highlight"])), ..def("mark") },
            CommandDef { categories: Some(strings(&["annotationsExtra"])), ..def("extra") },
        ])),
        vec![i18n_with(
            json!({ "commands": { "notAllowed": {
                "doc-print": "{label} may not print here",
                "default": "No {label} here"
            } } }),
            &warnings,
        )],
        json!({ "a": document(json!(["doc.annotate.create"])) }),
    );
    let refusal = |id: &str| rig.commands.get_refusal(id, None).unwrap();
    // With no document open there is no session to ask.
    assert_eq!(refusal("print"), None);
    assert!(rig.resolve("print").visible);

    rig.open("a");
    let not_allowed = |message: &str| {
        Some(CommandRefusal { reason: RefusalReason::NotAllowed, message: message.into() })
    };
    assert_eq!(refusal("print"), not_allowed("print may not print here"));
    // `annotations:create` asks the session's annotation check.
    assert_eq!(refusal("draw"), None);
    // `allowed` saying no, under a key no language has: the general sentence.
    assert_eq!(refusal("download"), not_allowed("No download here"));
    // Hidden by default; shown disabled with the reason under `disable`.
    let print = rig.resolve("print");
    assert!(!print.visible && !print.enabled && print.disabled_reason.is_none());
    rig.commands.update_settings(&CommandsConfig {
        not_allowed: Some(NotAllowedMode::Disable),
        ..CommandsConfig::default()
    });
    let print = rig.resolve("print");
    assert!(print.visible && !print.enabled);
    assert_eq!(print.disabled_reason.as_deref(), Some("print may not print here"));

    // Turned off by id, or by a category on whole dotted parts; turned off
    // wins over not allowed, and hides whatever `notAllowed` says.
    rig.commands.disable_command("print");
    rig.commands.disable_category("annotations");
    let turned_off = |message: &str| {
        Some(CommandRefusal { reason: RefusalReason::TurnedOff, message: message.into() })
    };
    assert_eq!(refusal("print"), turned_off("print isn't available"));
    assert_eq!(refusal("mark"), turned_off("mark isn't available"));
    assert_eq!(refusal("extra"), None);
    assert!(!rig.resolve("print").visible);
    assert!(rig.commands.is_command_disabled("mark").unwrap());
    assert!(!rig.commands.is_command_disabled("extra").unwrap());
    assert!(rig.commands.is_category_disabled("annotations.highlight"));
    // An id no command has is off when the list names it.
    rig.commands.disable_command("ghost");
    assert!(rig.commands.is_command_disabled("ghost").unwrap());
    assert_eq!(refusal("ghost"), None);
    assert!(rig.reports.borrow().is_empty());
}

#[test]
fn a_label_falls_back_to_the_label_then_the_key_then_the_id() {
    let defs = || {
        vec![
            CommandDef {
                label: Some("Save".into()),
                label_key: Some("k.save".into()),
                ..def("save")
            },
            CommandDef { label_key: Some("k.open".into()), ..def("open") },
            CommandDef { label_key: Some("k.missing".into()), ..def("missing") },
            def("bare"),
            CommandDef { label: Some("Plain".into()), ..def("plain") },
        ]
    };
    // Without i18n: `label`, else `labelKey`, else the id.
    let rig = rig(config(defs()));
    let labels = |rig: &Rig| {
        ["save", "open", "missing", "bare", "plain"].map(|id| rig.resolve(id).label.clone())
    };
    assert_eq!(labels(&rig), ["Save", "k.open", "k.missing", "bare", "Plain"]);

    // With i18n: a string under the id, else under the key, else `label`,
    // else the id; a key with no label of its own warns once.
    let warnings = Rc::default();
    let rig = rig_with(
        Some(config(defs())),
        vec![i18n_with(
            json!({ "k": { "open": "Open…", "save": "Keep" }, "bare": "Bare by id" }),
            &warnings,
        )],
        json!({}),
    );
    assert_eq!(labels(&rig), ["Keep", "Open…", "missing", "Bare by id", "Plain"]);
    assert_eq!(labels(&rig), ["Keep", "Open…", "missing", "Bare by id", "Plain"]);
    assert_eq!(
        *warnings.borrow(),
        ["[i18n] missing translation \"missing\" or \"k.missing\" (locale: en)"]
    );
}

/// A counter that ticks once per turn, as `await undefined` in a loop.
fn turn_counter() -> Rc<Cell<u32>> {
    let turn = Rc::new(Cell::new(0));
    let count = turn.clone();
    rt::spawn(async move {
        for _ in 0..64 {
            rt::tick().await;
            count.set(count.get() + 1);
        }
    });
    turn
}

/// The turn `promise` settles on (its reaction's turn) and how.
fn reacts_on(promise: Promise<ExecuteResult>, turn: &Rc<Cell<u32>>) -> Rc<RefCell<String>> {
    let at: Rc<RefCell<String>> = Rc::default();
    let (seen, turn) = (at.clone(), turn.clone());
    rt::spawn(async move {
        let shown = match promise.await {
            Ok(ExecuteResult::Executed) => format!("{} executed", turn.get()),
            Ok(ExecuteResult::Rejected { reason }) => format!("{} rejected {reason:?}", turn.get()),
            Err(error) => {
                format!("rejected@{} {}", turn.get(), error.plugin_code().unwrap_or_default())
            }
        };
        *seen.borrow_mut() = shown;
    });
    at
}

/// The turn `on_executed` and `on_execution_failed` fired on.
fn heard_on(rig: &Rig, turn: &Rc<Cell<u32>>) -> Rc<RefCell<Vec<String>>> {
    let heard: Rc<RefCell<Vec<String>>> = Rc::default();
    let (log, at) = (heard.clone(), turn.clone());
    rig.commands.on_executed(move |event| {
        log.borrow_mut().push(format!("executed {}@{}", event.command_id, at.get()));
        Ok(())
    });
    let (log, at) = (heard.clone(), turn.clone());
    rig.commands.on_execution_failed(move |event| {
        log.borrow_mut().push(format!("failed {}@{}", event.command_id, at.get()));
        Ok(())
    });
    heard
}

/// Measured on the TypeScript with `npx tsx --conditions=development`
/// (`createKernel` with `commandsPlugin`, a counter of `await undefined`
/// started before the call): every `.await` here is one `await` there.
#[test]
fn execute_settles_on_the_typescripts_turns() {
    let measure = |run: Option<RunFn>, menu: bool, signal: Option<AbortSignal>, id: &str| {
        let rig =
            rig(config(vec![CommandDef { run, menu: menu.then(|| "m".to_string()), ..def("a") }]));
        let turn = turn_counter();
        let heard = heard_on(&rig, &turn);
        let at = reacts_on(
            rig.commands.execute(id, ExecuteOptions { signal, ..ExecuteOptions::default() }),
            &turn,
        );
        rt::run_until_stalled();
        (at.borrow().clone(), heard.borrow().clone())
    };
    let sync: Option<RunFn> = Some(Rc::new(|_| Ok(None)));
    let resolved: Option<RunFn> = Some(Rc::new(|_| Ok(Some(Promise::resolved(json!(1))))));
    let throws: Option<RunFn> = Some(Rc::new(|_| Err(Error::new("x"))));
    let rejects: Option<RunFn> = Some(Rc::new(|_| Ok(Some(Promise::rejected(Error::new("x"))))));
    let open = || Some(AbortSignal::new());
    let executed = |turn: u32| vec![format!("executed a@{turn}")];
    let failed = |turn: u32| vec![format!("failed a@{turn}")];

    assert_eq!(measure(sync.clone(), false, None, "a"), ("3 executed".into(), executed(2)));
    assert_eq!(measure(resolved.clone(), false, None, "a"), ("3 executed".into(), executed(2)));
    assert_eq!(measure(None, true, None, "a"), ("2 executed".into(), executed(1)));
    assert_eq!(
        measure(throws, false, None, "a"),
        ("rejected@2 operation-failed".into(), failed(1))
    );
    assert_eq!(
        measure(rejects.clone(), false, None, "a"),
        ("rejected@3 operation-failed".into(), failed(2))
    );
    assert_eq!(measure(None, false, None, "zz"), ("1 rejected NotFound".into(), vec![]));
    assert_eq!(measure(sync.clone(), false, open(), "a"), ("4 executed".into(), executed(3)));
    assert_eq!(measure(resolved, false, open(), "a"), ("4 executed".into(), executed(3)));
    assert_eq!(
        measure(rejects, false, open(), "a"),
        ("rejected@4 operation-failed".into(), failed(3))
    );
    let fired = AbortSignal::new();
    fired.abort(Error::new("stop"));
    assert_eq!(
        measure(sync, false, Some(fired), "a"),
        ("rejected@1 operation-cancelled".into(), vec![])
    );
}

/// A run whose promise the test settles later.
#[derive(Clone, Default)]
struct Later {
    resolvers: Rc<RefCell<Vec<Resolver<Value>>>>,
}

impl Later {
    fn run(&self) -> RunFn {
        let this = self.clone();
        Rc::new(move |_| {
            let (promise, resolver) = Promise::pending();
            this.resolvers.borrow_mut().push(resolver);
            Ok(Some(promise))
        })
    }
}

#[test]
fn execute_waits_for_a_run_that_completes_later() {
    let later = Later::default();
    let rig = rig(config(vec![CommandDef { run: Some(later.run()), ..def("slow") }]));
    let turn = turn_counter();
    let heard = heard_on(&rig, &turn);
    let done = rig.commands.execute("slow", ExecuteOptions::default());
    rt::run_until_stalled();
    assert!(done.peek().is_none(), "execute settled before its run");
    assert!(heard.borrow().is_empty());

    later.resolvers.borrow_mut().remove(0).resolve(json!("late"));
    rt::run_until_stalled();
    assert!(matches!(done.peek(), Some(Ok(ExecuteResult::Executed))));
    assert_eq!(heard.borrow().len(), 1);

    // Cancelling while the run is pending rejects `operation-cancelled` on
    // the next turns, with no failure event; the run's late answer changes
    // nothing.
    let rig = rig_cancel(&later);
    let turn = turn_counter();
    let heard = heard_on(&rig, &turn);
    let signal = AbortSignal::new();
    let at = reacts_on(
        rig.commands.execute(
            "slow",
            ExecuteOptions { signal: Some(signal.clone()), ..ExecuteOptions::default() },
        ),
        &turn,
    );
    signal.abort(Error::new("stop"));
    rt::run_until_stalled();
    assert_eq!(*at.borrow(), "rejected@2 operation-cancelled");
    later.resolvers.borrow_mut().remove(0).resolve(json!("late"));
    rt::run_until_stalled();
    assert!(heard.borrow().is_empty());
    assert!(rig.reports.borrow().is_empty());
}

fn rig_cancel(later: &Later) -> Rig {
    rig(config(vec![CommandDef { run: Some(later.run()), ..def("slow") }]))
}

#[test]
fn a_walk_reads_the_ids_it_started_with() {
    let rig = rig(CommandsConfig::default());
    let commands = rig.commands.downgrade();
    type Removers = Rc<RefCell<Vec<Rc<dyn Fn()>>>>;
    let removers: Removers = Rc::default();
    let visited: Rc<RefCell<Vec<String>>> = Rc::default();
    let seen = |id: &str| {
        let (log, id) = (visited.clone(), id.to_string());
        Some(Rc::new(move |_: &CommandCtx| {
            log.borrow_mut().push(id.clone());
            Ok(true)
        }) as Derivation)
    };
    // `first` removes `second`, which the walk hasn't reached, and adds
    // `late`, which it won't reach.
    let (held, take) = (commands.clone(), removers.clone());
    let first: Derivation = Rc::new(move |_| {
        let remove = take.borrow_mut().pop();
        if let Some(remove) = remove {
            remove();
            let commands = held.upgrade().expect("the commands");
            let _ = commands.register_command(
                CommandDef { visible: None, ..def("late") },
                RegisterCommandOptions::default(),
            );
        }
        Ok(true)
    });
    let register = |definition: CommandDef| {
        rig.commands.register_command(definition, RegisterCommandOptions::default()).unwrap()
    };
    register(CommandDef { visible: Some(first), ..def("first") });
    let second = register(CommandDef { visible: seen("second"), ..def("second") });
    register(CommandDef { visible: seen("third"), ..def("third") });
    removers.borrow_mut().push(second);

    let walked = rig.commands.list_commands(None).unwrap();
    let ids = |list: &[Rc<ResolvedCommand>]| list.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&walked), ["first", "third"]);
    assert_eq!(*visited.borrow(), ["third"]);
    assert_eq!(*rig.commands.list_command_ids().unwrap(), ["first", "third", "late"]);
    assert_eq!(ids(&rig.commands.list_commands(None).unwrap()), ["first", "third", "late"]);
    assert!(rig.reports.borrow().is_empty());
}

#[test]
fn reads_keep_their_identity_until_an_input_changes() {
    let answer = Rc::new(Cell::new(true));
    let enabled = answer.clone();
    let rig = rig(config(vec![
        CommandDef {
            shortcut: Some(Shortcut::One("Mod+K".into())),
            enabled: Some(Rc::new(move |_| Ok(enabled.get()))),
            ..def("a")
        },
        def("b"),
    ]));
    let (resolved, list, ids, shortcuts) = (
        rig.resolve("a"),
        rig.commands.list_commands(None).unwrap(),
        rig.commands.list_command_ids().unwrap(),
        rig.commands.list_shortcuts().unwrap(),
    );
    assert!(Rc::ptr_eq(&resolved, &rig.resolve("a")));
    assert!(Rc::ptr_eq(&list, &rig.commands.list_commands(None).unwrap()));
    assert!(Rc::ptr_eq(&ids, &rig.commands.list_command_ids().unwrap()));
    assert!(Rc::ptr_eq(&shortcuts, &rig.commands.list_shortcuts().unwrap()));
    // `""` is the active document's key, as no document.
    assert!(Rc::ptr_eq(&list, &rig.commands.list_commands(Some("")).unwrap()));

    // A derivation's answer changes that command and the list, not the ids.
    answer.set(false);
    let changed = rig.resolve("a");
    assert!(!Rc::ptr_eq(&resolved, &changed) && !changed.enabled);
    assert!(!Rc::ptr_eq(&list, &rig.commands.list_commands(None).unwrap()));
    assert!(Rc::ptr_eq(&ids, &rig.commands.list_command_ids().unwrap()));
    assert!(Rc::ptr_eq(&rig.resolve("b"), &list[1]));

    // A registration changes the ids and the shortcuts.
    let remove = rig
        .commands
        .register_command(
            CommandDef { shortcut: Some(Shortcut::One("Mod+J".into())), ..def("c") },
            RegisterCommandOptions::default(),
        )
        .unwrap();
    assert_eq!(*rig.commands.list_command_ids().unwrap(), ["a", "b", "c"]);
    assert_eq!(rig.commands.list_shortcuts().unwrap().len(), 2);
    remove();
    remove();
    assert_eq!(*rig.commands.list_command_ids().unwrap(), ["a", "b"]);
}

#[test]
fn a_family_has_a_command_for_each_name() {
    let names = Rc::new(RefCell::new(Rc::new(strings(&["pen", "ink", "pointer"]))));
    let calls = Rc::new(Cell::new(0));
    let (listed, made) = (names.clone(), calls.clone());
    let family = CommandFamily {
        prefix: "tool:".into(),
        names: Rc::new(move |_| Ok(listed.borrow().clone())),
        command: Rc::new(move |name| {
            made.set(made.get() + 1);
            CommandDef {
                id: "ignored".into(),
                label: Some(format!("Tool {name}")),
                shortcut: Some(Shortcut::One("T".into())),
                ..CommandDef::default()
            }
        }),
    };
    let rig = rig(CommandsConfig {
        commands: Some(vec![
            CommandItem::from(CommandDef { label: Some("Pointer".into()), ..def("tool:pointer") }),
            CommandItem::from(family),
        ]),
        ..CommandsConfig::default()
    });
    // The defined command keeps its id; the family lists the names no
    // defined command took.
    assert_eq!(*rig.commands.list_command_ids().unwrap(), ["tool:pointer", "tool:pen", "tool:ink"]);
    assert_eq!(rig.resolve("tool:pointer").label, "Pointer");
    let pen = rig.resolve("tool:pen");
    assert_eq!((pen.id.as_str(), pen.label.as_str()), ("tool:pen", "Tool pen"));
    // A family's command has the registry's id and no shortcut, and is made
    // once per name.
    let defined = rig.commands.get_command("tool:pen").unwrap().unwrap();
    assert_eq!((defined.id.as_str(), defined.shortcut.is_none()), ("tool:pen", true));
    assert!(Rc::ptr_eq(&defined, &rig.commands.get_command("tool:pen").unwrap().unwrap()));
    // Any id past the prefix is the family's; the prefix alone is not.
    assert!(rig.commands.has_command("tool:anything").unwrap());
    assert!(!rig.commands.has_command("tool:").unwrap());
    assert!(rig.commands.list_shortcuts().unwrap().is_empty());

    // The same names array keeps the ids; a new one rebuilds them.
    let ids = rig.commands.list_command_ids().unwrap();
    assert!(Rc::ptr_eq(&ids, &rig.commands.list_command_ids().unwrap()));
    *names.borrow_mut() = Rc::new(strings(&["ink"]));
    assert_eq!(*rig.commands.list_command_ids().unwrap(), ["tool:pointer", "tool:ink"]);
    // Made for `pen` and `anything` only, each once.
    rig.resolve("tool:pen");
    assert_eq!(calls.get(), 2);

    // A family whose names throw lists none.
    let failing = CommandFamily {
        prefix: "x:".into(),
        names: Rc::new(|_| Err(Error::new("no document"))),
        command: Rc::new(|_| CommandDef::default()),
    };
    rig.commands.update_settings(&CommandsConfig {
        commands: Some(vec![CommandItem::from(failing)]),
        ..CommandsConfig::default()
    });
    assert!(rig.commands.list_command_ids().unwrap().is_empty());
    assert!(rig.reports.borrow().is_empty());
}

fn stroke(key: &str, ctrl: bool) -> KeyStroke {
    serde_json::from_value(json!({
        "key": key, "ctrlKey": ctrl, "metaKey": false, "altKey": false, "shiftKey": false
    }))
    .unwrap()
}

#[test]
fn a_keystroke_prefers_a_command_that_may_run() {
    let keys = || Some(Shortcut::One("Ctrl+D".into()));
    let rig = rig(config(vec![
        CommandDef { shortcut: keys(), categories: Some(strings(&["off"])), ..def("refused") },
        CommandDef {
            shortcut: keys(),
            categories: Some(strings(&["off"])),
            visible: Some(derivation(false)),
            ..def("unseen")
        },
        CommandDef { shortcut: keys(), visible: Some(derivation(false)), ..def("runs") },
        CommandDef {
            shortcut: Some(Shortcut::One("Ctrl+F".into())),
            shortcut_in_text_fields: Some(true),
            ..def("find")
        },
    ]));
    rig.commands.disable_category("off");
    let pc = MatchStrokeOptions { is_mac: false, in_text_field: false };
    let matched = |options| rig.commands.match_stroke(&stroke("d", true), options).unwrap();
    let shown =
        |found: Option<ShortcutMatch>| found.map(|m| (m.command_id, m.refusal.map(|r| r.reason)));
    // One that may run wins over the refused ones ahead of it, whatever its
    // `visible` says: only a refused command asks.
    assert_eq!(shown(matched(pc)), Some(("runs".to_string(), None)));
    // Without one, the first refused one that would show.
    rig.commands.disable_command("runs");
    assert_eq!(shown(matched(pc)), Some(("refused".to_string(), Some(RefusalReason::TurnedOff))));
    // A refused one that wouldn't show leaves the keys to the page.
    rig.commands.disable_command("refused");
    rig.commands.update_settings(&CommandsConfig {
        disabled_commands: Some(strings(&["refused", "runs"])),
        ..CommandsConfig::default()
    });
    let only_unseen = config(vec![CommandDef {
        shortcut: keys(),
        categories: Some(strings(&["off"])),
        visible: Some(derivation(false)),
        ..def("unseen")
    }]);
    rig.commands.update_settings(&only_unseen);
    assert_eq!(shown(matched(pc)), None);
    // In a text field only a command that says so matches. (Reset brings
    // back the registered commands, none of them off.)
    rig.commands.reset_settings();
    let field = MatchStrokeOptions { is_mac: false, in_text_field: true };
    assert_eq!(shown(matched(pc)), Some(("refused".to_string(), None)));
    assert_eq!(shown(matched(field)), None);
    assert_eq!(
        shown(rig.commands.match_stroke(&stroke("f", true), field).unwrap()),
        Some(("find".to_string(), None))
    );
}

#[test]
fn a_panel_with_focus_asks_for_the_cursor() {
    let rig = rig_with(
        Some(config(vec![
            CommandDef {
                panel: Some(Panel::Target {
                    id: "search".into(),
                    exclusive: Some("side".into()),
                    focus: Some(true),
                }),
                ..def("find")
            },
            CommandDef { panel: Some(Panel::Id("thumbs".into())), ..def("thumbs") },
        ])),
        vec![shell_plugin()],
        json!({ "a": document(json!("*")) }),
    );
    rig.open("a");
    let execute = |id: &str| settle(&rig.commands.execute(id, ExecuteOptions::default())).unwrap();
    assert_eq!(execute("find"), ExecuteResult::Executed);
    assert_eq!(execute("thumbs"), ExecuteResult::Executed);
    let shell = rig.kernel.capability(&SHELL, Some("a")).unwrap();
    let record = |id: &str| shell.get_surface(id).and_then(|s| s.record.clone()).expect(id);
    assert_eq!(record("search").focus_request, 1.0);
    assert_eq!(record("search").exclusive.as_deref(), Some("side"));
    assert_eq!(record("thumbs").focus_request, 0.0);
    assert!(rig.resolve("find").active);
}

#[test]
fn a_document_scope_is_what_a_call_without_a_document_is_for() {
    let seen: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
    let (ran_on, derived_on) = (seen.clone(), seen.clone());
    let rig = rig_with(
        Some(config(vec![CommandDef {
            run: Some(Rc::new(move |c| {
                ran_on.borrow_mut().push(c.document_id().map(str::to_string));
                Ok(None)
            })),
            visible: Some(Rc::new(move |c| {
                derived_on.borrow_mut().push(c.document_id().map(str::to_string));
                Ok(true)
            })),
            requires: Some(strings(&["doc.print"])),
            ..def("go")
        }])),
        Vec::new(),
        json!({ "a": document(json!("*")), "b": document(json!([])) }),
    );
    rig.open("a");
    rig.open("b");
    let take = || seen.borrow_mut().drain(..).collect::<Vec<_>>();
    let on = |id: &str| Some(id.to_string());
    let scoped = rig.commands.in_scope("a");
    // Each read that leaves out the document reads the one in scope.
    assert!(scoped.resolve_command("go", None).unwrap().unwrap().visible);
    assert_eq!(scoped.list_commands(None).unwrap().len(), 1);
    assert_eq!(scoped.search_commands("go", None).unwrap().len(), 1);
    assert!(scoped.can_execute("go", None).unwrap());
    assert_eq!(scoped.get_refusal("go", None).unwrap(), None);
    assert_eq!(take(), [on("a"), on("a"), on("a"), on("a")]);
    let ran = |cap: &CommandsCapability, options| {
        settle(&cap.execute("go", options)).unwrap();
        take()
    };
    assert_eq!(ran(&scoped, ExecuteOptions::default()), [on("a"), on("a")]);
    // A document named wins; the capability itself reads the active one,
    // which refuses `go` there.
    let named = ExecuteOptions { document_id: Some("b".into()), ..ExecuteOptions::default() };
    assert_eq!(
        settle(&scoped.execute("go", named)).unwrap(),
        ExecuteResult::Rejected { reason: RejectReason::Hidden }
    );
    assert!(rig.commands.get_refusal("go", None).unwrap().is_some());
    assert!(scoped.get_refusal("go", Some("b")).unwrap().is_some());
    // A weak handle keeps the scope.
    let weak = scoped.downgrade().upgrade().unwrap();
    assert!(weak.can_execute("go", None).unwrap());
}

#[test]
fn after_the_kernel_reads_of_the_registry_survive_and_document_reads_refuse() {
    let rig = rig(config(vec![CommandDef { menu: Some("file".into()), ..def("menu") }]));
    let remove =
        rig.commands.register_command(def("live"), RegisterCommandOptions::default()).unwrap();
    settle(&rig.kernel.destroy()).unwrap();
    let destroyed = "[kernel] capability(\"documents\") on a destroyed kernel";
    assert!(rig.commands.has_command("menu").unwrap());
    assert_eq!(
        rig.commands.get_menu_target("menu").unwrap(),
        Some(MenuTarget { menu: Some("file".into()) })
    );
    assert_eq!(rig.commands.list_command_ids().unwrap_err().message, destroyed);
    assert_eq!(rig.commands.resolve_command("menu", None).unwrap_err().message, destroyed);
    assert_eq!(rig.commands.resolve_command("missing", None).unwrap(), None);
    // A remover after teardown still removes its own registration, quietly.
    remove();
    assert!(!rig.commands.has_command("live").unwrap());
    rig.commands.disable_category("x");
    let failed = settle(&rig.commands.execute("menu", ExecuteOptions::default())).unwrap_err();
    assert_eq!(failed.message, destroyed);
    assert!(rig.reports.borrow().is_empty(), "{:?}", rig.reports.borrow());
}

#[test]
fn the_kernel_hands_out_the_scoped_view() {
    let seen: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
    let log = seen.clone();
    let rig = rig_with(
        Some(config(vec![CommandDef {
            run: Some(Rc::new(move |c| {
                log.borrow_mut().push(c.document_id().map(str::to_string));
                Ok(None)
            })),
            ..def("go")
        }])),
        Vec::new(),
        json!({ "a": document(json!("*")), "b": document(json!("*")) }),
    );
    rig.open("a");
    rig.open("b");
    let ran = |cap: &CommandsCapability| {
        settle(&cap.execute("go", ExecuteOptions::default())).unwrap();
        seen.borrow_mut().pop().flatten()
    };
    // Resolved for a document, the capability is that document's view,
    // every time it is asked for; without one, the active document's.
    let scoped = rig.kernel.capability(&COMMANDS, Some("a")).unwrap();
    assert_eq!(ran(&scoped).as_deref(), Some("a"));
    assert_eq!(ran(&rig.kernel.capability(&COMMANDS, Some("a")).unwrap()).as_deref(), Some("a"));
    assert_eq!(ran(&rig.kernel.capability(&COMMANDS, None).unwrap()).as_deref(), Some("b"));
}

/// M37: the registration's table keeps only the definitions the registered
/// and current settings name, as the TypeScript store holds them; a
/// definition a change or a reset replaced is released.
#[test]
fn a_replaced_definition_is_released_from_the_command_table() {
    let rig = rig(config(vec![def("open")]));
    assert_eq!(rig.commands.retained_commands(), 1);
    for _ in 0..30 {
        rig.commands.update_settings(&config(vec![CommandDef {
            run: Some(Rc::new(|_| Ok(None))),
            ..def("save")
        }]));
    }
    assert_eq!(rig.commands.retained_commands(), 2, "the registered one and the current one");
    rig.commands.reset_settings();
    assert_eq!(rig.commands.retained_commands(), 1, "only the registered one");
    assert!(rig.commands.get_command("open").is_ok_and(|c| c.is_some()));
}

/// M37's count covers both of the table's kinds: a family the settings
/// name is held beside the definitions.
#[test]
fn the_command_table_counts_its_families_beside_its_definitions() {
    let family = CommandFamily {
        prefix: "tool:".into(),
        names: Rc::new(|_| Ok(Rc::new(strings(&["pen"])))),
        command: Rc::new(|_| CommandDef::default()),
    };
    let rig = rig(CommandsConfig {
        commands: Some(vec![CommandItem::from(def("open")), CommandItem::from(family)]),
        ..CommandsConfig::default()
    });
    assert_eq!(rig.commands.retained_commands(), 2, "one definition and one family");
}
