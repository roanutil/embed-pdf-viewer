//! `scripting/`: the K/V/C/F transaction ([`controller`]), its overlay and
//! its result shaping. `/scripting` in the TypeScript exports
//! [`controller::create_form_scripting_controller`].

pub mod controller;
pub mod overlay;
pub mod results;
