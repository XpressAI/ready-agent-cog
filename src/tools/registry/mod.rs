//! Centralized tool registry that maps tool IDs to their owning modules.

mod runtime;

#[cfg(test)]
mod tests;

pub use runtime::InMemoryToolRegistry;
