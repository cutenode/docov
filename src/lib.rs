//! How much of a JavaScript or TypeScript project's API has JSDoc.

pub mod batch;
pub mod cache;
pub mod check;
pub mod collect;
pub mod config;
pub mod feature;
pub mod git;
pub mod glob;
pub mod init;
pub mod jsdoc;
pub mod metric;
pub mod nested;
pub mod packages;
pub mod public;
pub mod report;
pub mod rewrite;
pub mod root;
pub mod walk;

pub use collect::{FileReport, Item, Kind, Rules, scan, scan_file};
pub use config::Config;
pub use feature::{Feature, Settings, scope::Scope, threshold::Threshold};
pub use report::{Context, Format, Output, Totals};
