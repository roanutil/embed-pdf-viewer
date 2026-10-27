//! A definition's closures live in the registry the capability owns. One
//! that holds the capability through `WeakCommandsCapability`, and the
//! remover its own registration returned, lets the capability drop with
//! the kernel.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_commands::*;
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

type Remover = Rc<dyn Fn()>;

#[test]
fn a_definition_holding_a_weak_capability_drops_with_the_kernel() {
    rt::reset();
    let fake = FakeEngine::new(json!({ "documents": {} }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![commands_plugin(CommandsConfig::default())],
        KernelOptions {
            report: None,
            on_dispatch: None,
            release_closed_sessions: false,
            printer: None,
        },
    )
    .unwrap();
    let _ = kernel.start();
    rt::run_until_stalled();

    let commands = kernel.capability(&COMMANDS, None).unwrap();
    let handle = commands.downgrade();
    let marker = Rc::new(());
    let held: Weak<()> = Rc::downgrade(&marker);
    let remover: Rc<RefCell<Option<Remover>>> = Rc::default();
    let mut def = CommandDef::new("once", "once.label");
    let (weak, own, marker) = (commands.downgrade(), remover.clone(), marker);
    def.run = Some(Rc::new(move |_| {
        let _ = (&weak, &own, &marker);
        Ok(None)
    }));
    *remover.borrow_mut() =
        Some(commands.register_command(def, RegisterCommandOptions::default()).unwrap());
    drop(remover);
    assert!(handle.upgrade().is_some());
    drop(commands);

    let _ = kernel.destroy();
    rt::run_until_stalled();
    drop(kernel);
    rt::run_until_stalled();
    assert!(handle.upgrade().is_none(), "the capability leaked");
    assert!(held.upgrade().is_none(), "the definition leaked");
}
