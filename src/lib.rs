//! How much of a JavaScript or TypeScript project's API has JSDoc.

pub mod collect;
pub mod config;
pub mod feature;
pub mod public;
pub mod report;
pub mod rewrite;
pub mod walk;

pub use collect::{FileReport, Item, Kind, Rules, scan, scan_file};
pub use config::Config;
pub use feature::{Feature, Settings, scope::Scope, threshold::Threshold};
pub use report::{Format, Totals};
