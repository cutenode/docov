//! Scans many files at once.

use std::path::Path;

use rayon::{iter::Either, prelude::*};

use crate::{FileReport, Scope, scan_file};

/// Scans each file with its scope, in parallel, or says why each one that couldn't be scanned
/// couldn't.
pub fn scan(files: &[(&Path, Scope)]) -> Result<Vec<FileReport>, Vec<String>> {
    let (reports, errors): (Vec<_>, Vec<_>) =
        files.par_iter().partition_map(|&(path, scope)| match scan_file(path, scope) {
            Ok(report) => Either::Left(report),
            Err(e) => Either::Right(e.to_string()),
        });
    if errors.is_empty() { Ok(reports) } else { Err(errors) }
}
