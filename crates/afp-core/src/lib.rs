//! AFP (MO:DCA) parsing core — pure, no UI.
//!
//! Phase 1 surface: structured-field framing, SFID classification/naming,
//! triplet decoding, nesting-tree construction, document summary, resource
//! enumeration, and IOCA image extraction. See `docs/plans/`.
//!
//! Modules are added task-by-task per the Phase 1 plan.

pub mod build;
pub mod names;
pub mod resource;
pub mod sf;
pub mod summary;
pub mod tree;
pub mod triplet;

pub use resource::{Resource, ResourceKind};
pub use summary::Summary;
pub use tree::{Document, Node, Problem};
