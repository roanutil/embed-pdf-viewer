//! Generates the shell-side types: `codegen <swift|kotlin|typescript> <out-dir>`.
//!
//! Registers the shell's effect type rather than calling `register_app`,
//! because `register_app` would register the app's own `Effect`, and that
//! includes the engine request the middleware keeps out of every shell.

use std::path::PathBuf;

use crux_core::type_generation::facet::{Config, Export as _, TypeGenError, TypeRegistry};
use epdf_crux::ShellEffect;
use epdf_crux::app::{Event, ViewModel};

fn main() -> Result<(), TypeGenError> {
    let mut args = std::env::args().skip(1);
    let language = args.next().expect("usage: codegen <swift|kotlin|typescript> <out-dir>");
    let out_dir = PathBuf::from(args.next().expect("an output directory"));

    let mut registry = TypeRegistry::new();
    ShellEffect::register_types(&mut registry)?;
    let generator = registry.register_type::<Event>()?.register_type::<ViewModel>()?.build()?;

    match language.as_str() {
        "swift" => generator.swift(&Config::builder("EmbedPdfCrux", &out_dir).build()),
        "kotlin" => generator.kotlin(&Config::builder("com.embedpdf.crux", &out_dir).build()),
        "typescript" => generator.typescript(&Config::builder("embedpdf_crux", &out_dir).build()),
        other => panic!("no generator for '{other}'"),
    }
}
