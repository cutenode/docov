//! How much of a JavaScript or TypeScript project's API has JSDoc.

pub mod collect;
pub mod config;
pub mod report;
pub mod walk;

pub use collect::{FileReport, Item, Kind, scan, scan_file};
pub use config::{Config, Scope};
pub use report::{Format, Totals};
