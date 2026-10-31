//! Scans many files at once.

use std::path::Path;

use rayon::{iter::Either, prelude::*};

use crate::{FileReport, Rules, cache::Cache, scan_file};

/// Scans each file with its rules, in parallel, or says why each one that couldn't be scanned
/// couldn't. A file `cache` has unchanged isn't read, and `cache` is written with what's found.
pub fn scan(files: &[(&Path, &Rules)], cache: &Cache) -> Result<Vec<FileReport>, Vec<String>> {
    let (scanned, errors): (Vec<_>, Vec<_>) = files.par_iter().partition_map(|&(path, rules)| {
        let stamp = cache.stamp(path, rules);
        let cached = stamp.and_then(|stamp| cache.get(path, stamp));
        match cached.map_or_else(|| scan_file(path, rules), Ok) {
            Ok(report) => Either::Left((report, stamp)),
            Err(e) => Either::Right(e.to_string()),
        }
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    cache.write(&scanned).map_err(|e| vec![e])?;
    Ok(scanned.into_iter().map(|(report, _)| report).collect())
}
