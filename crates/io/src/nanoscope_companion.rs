use crate::AfmData;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn companion_candidates(path: &Path) -> Vec<PathBuf> {
    let Some(stem) = path.file_stem().map(|value| value.to_string_lossy()) else {
        return Vec::new();
    };
    let base = stem
        .strip_suffix("-PeakForceCapture")
        .or_else(|| stem.strip_suffix("_PeakForceCapture"))
        .unwrap_or(&stem);
    let exact = path.with_file_name(format!("{base}-AllImages.spm"));
    let mut candidates = Vec::new();
    if exact.exists() {
        candidates.push(exact);
    }
    let Some(parent) = path.parent() else {
        return candidates;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return candidates;
    };
    let normalized_base = normalize_companion_stem(base);
    let mut discovered: Vec<(usize, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("spm"))
        })
        .filter_map(|candidate| {
            let candidate_stem = candidate.file_stem()?.to_string_lossy();
            let lower = candidate_stem.to_ascii_lowercase();
            let marker = lower.find("allimages")?;
            let prefix = candidate_stem[..marker].trim_end_matches(['-', '_', ' ']);
            let normalized = normalize_companion_stem(prefix);
            let score = common_prefix_len(&normalized_base, &normalized);
            Some((score, candidate))
        })
        .collect();
    discovered.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    for (_, candidate) in discovered {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

pub(super) fn normalize_companion_stem(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn common_prefix_len(left: &str, right: &str) -> usize {
    left.bytes()
        .zip(right.bytes())
        .take_while(|(left, right)| left == right)
        .count()
}

pub(super) fn geometries_match(left: &AfmData, right: &AfmData) -> bool {
    let Some(force) = &left.forces else {
        return false;
    };
    right.images.iter().all(|image| {
        image.width == force.grid_width
            && image.height == force.grid_height
            && image.scan_size_x.is_finite()
            && image.scan_size_y.is_finite()
    })
}
