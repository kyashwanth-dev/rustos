//! Persistent storage formats and block-device persistence helpers.

pub mod persistence;
pub mod serializer;

pub use persistence::{load, save};