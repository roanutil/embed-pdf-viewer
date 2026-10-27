//! `services/`: what every area is built on. The form's mirror (the field
//! tree and every widget row) is `sync::fields`; here are who may fill in
//! which field, the events, the sibling plugins, the scripting seam, the
//! texts this session's format scripts wrote, the object numbers a create
//! takes, and the widgets' pictures and looks. The one write queue is the
//! kernel's (`ctx.serial_queue("writes")`).

pub mod design;
pub mod events;
pub mod formatted;
pub mod index_cache;
pub mod looks;
pub mod object_numbers;
pub mod pictures;
pub mod rights;
pub mod scripting;
pub mod siblings;
