//! SQLite connection setup (WAL mode), schema, and repository functions
//! for feeds and items (see docs/ARCHITECTURE.md §5).

mod connection;
mod schema;

pub use connection::{init, resolve_db_path};

#[cfg(test)]
pub use connection::init_in_memory;
