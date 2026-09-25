//! Crux as the platform wrapper generator for the engine.
//!
//! Research spike. Crux is an app architecture, not a binding generator, so
//! this uses it the way it is shaped rather than against it:
//!
//! - [`app::PdfViewer`] is a Crux `App`: pure, synchronous, testable without
//!   PDFium. Its events and view model are what a shell sees, and Crux
//!   generates them as Swift, Kotlin and TypeScript types (`src/bin/codegen.rs`).
//! - [`middleware::EngineMiddleware`] resolves the app's engine effects on
//!   the PDFium owner thread, inside Rust. The shell never handles them.
//! - [`ShellEffect`] is what is left for the shell: redraw, a finished raster,
//!   and the answer to an untyped call.
//!
//! The FFI itself is three byte-array functions (update, resolve, view) plus
//! one callback, which is why Crux does not care what generates it. Two more
//! functions move bulk bytes beside Crux rather than through it
//! ([`blobs`]).

#[cfg(not(target_family = "wasm"))]
uniffi::setup_scaffolding!();

pub mod app;
pub mod blobs;
#[cfg(not(target_family = "wasm"))]
pub mod ffi;
#[cfg(not(target_family = "wasm"))]
pub mod middleware;

use crux_core::{macros::effect, render::RenderOperation};

use app::{CallReply, PageBitmap, PageText, RequestFailed};

/// The effects a shell handles. [`app::Effect`] minus the engine.
#[effect(facet_typegen)]
#[derive(Debug)]
pub enum ShellEffect {
    Render(RenderOperation),
    Bitmap(PageBitmap),
    Text(PageText),
    Reply(CallReply),
    Failed(RequestFailed),
}

impl From<app::Effect> for ShellEffect {
    fn from(effect: app::Effect) -> Self {
        match effect {
            app::Effect::Render(request) => Self::Render(request),
            app::Effect::Bitmap(request) => Self::Bitmap(request),
            app::Effect::Text(request) => Self::Text(request),
            app::Effect::Reply(request) => Self::Reply(request),
            app::Effect::Failed(request) => Self::Failed(request),
            // `HandleEffectLayer` takes every engine request before this
            // mapping runs; one arriving here means the middleware was not
            // installed, which is a construction bug and not a runtime state.
            app::Effect::Engine(_) => unreachable!("engine effects are handled in Rust"),
        }
    }
}
