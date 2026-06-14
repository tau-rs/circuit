//! IO adapters behind the foundation's port traits (M2 §6). Each adapter brings
//! its own `thiserror` error so the foundation never guesses failure modes.

pub mod checkpoints;
pub mod forge;
