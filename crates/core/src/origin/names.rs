use std::collections::{BTreeMap, BTreeSet};

use plotx_io::origin::{OriginColumn, OriginLimits};

use super::{OriginImportError, checked_add, copy_text, enforce, try_reserve};

pub(super) fn normalize(
    columns: &[OriginColumn],
    limits: &OriginLimits,
) -> Result<Vec<String>, OriginImportError> {
    let mut reserved = BTreeSet::new();
    for column in columns {
        if !column.name.trim().is_empty() {
            reserved.insert(copy_text(&column.name, "Origin column name")?);
        }
    }

    let mut names = Vec::new();
    try_reserve(&mut names, columns.len(), "Origin column names")?;
    let mut used = BTreeSet::new();
    let mut next_suffix = BTreeMap::new();
    for (index, column) in columns.iter().enumerate() {
        let source_name = !column.name.trim().is_empty();
        let base = if source_name {
            copy_text(&column.name, "Origin column name")?
        } else {
            generated_column_name(index, limits)?
        };
        enforce("string bytes", base.len(), limits.max_string_bytes)?;
        let can_use_base = !used.contains(&base) && (source_name || !reserved.contains(&base));
        let candidate = if can_use_base {
            base
        } else {
            unique_suffixed_name(&base, &reserved, &used, &mut next_suffix, limits)?
        };
        if !used.insert(copy_text(&candidate, "Origin column name")?) {
            return Err(OriginImportError::InvalidModel {
                detail: "column name normalization produced a duplicate".to_owned(),
            });
        }
        names.push(candidate);
    }
    Ok(names)
}

fn unique_suffixed_name(
    base: &str,
    reserved: &BTreeSet<String>,
    used: &BTreeSet<String>,
    next_suffix: &mut BTreeMap<String, usize>,
    limits: &OriginLimits,
) -> Result<String, OriginImportError> {
    let mut suffix = next_suffix.get(base).copied().unwrap_or(2);
    let candidate = loop {
        let candidate = suffixed_name(base, suffix, limits)?;
        suffix = checked_add(suffix, 1, "column name suffix")?;
        if !reserved.contains(&candidate) && !used.contains(&candidate) {
            break candidate;
        }
    };
    next_suffix.insert(copy_text(base, "Origin column name")?, suffix);
    Ok(candidate)
}

fn generated_column_name(index: usize, limits: &OriginLimits) -> Result<String, OriginImportError> {
    let number = checked_add(index, 1, "generated column number")?.to_string();
    joined_name("Column ", "", &number, limits)
}

fn suffixed_name(
    base: &str,
    suffix: usize,
    limits: &OriginLimits,
) -> Result<String, OriginImportError> {
    joined_name(base, " (", &format!("{suffix})"), limits)
}

fn joined_name(
    left: &str,
    separator: &str,
    right: &str,
    limits: &OriginLimits,
) -> Result<String, OriginImportError> {
    let length = checked_add(
        checked_add(left.len(), separator.len(), "column name")?,
        right.len(),
        "column name",
    )?;
    enforce("string bytes", length, limits.max_string_bytes)?;
    let mut name = String::new();
    name.try_reserve_exact(length)
        .map_err(|_| OriginImportError::AllocationFailed {
            resource: "Origin column name",
            requested: length,
        })?;
    name.push_str(left);
    name.push_str(separator);
    name.push_str(right);
    Ok(name)
}
