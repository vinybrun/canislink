//! CanisLink data stores: in-memory (tests) + SQLite durability (lab ship).

pub mod memory;
pub mod sqlite;

pub use memory::*;
