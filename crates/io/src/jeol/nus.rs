//! JEOL non-uniform-sampling metadata and serialized schedule extraction.

use super::Params;
use crate::NusMeta;

pub(super) fn detect_nus(bytes: &[u8], params: &Params, acquired: usize) -> Option<NusMeta> {
    let sampling = params.string_ci("sampling")?;
    if !sampling.trim().to_ascii_uppercase().starts_with("NUS") {
        return None;
    }
    // `sampling_rate` is a percentage (25 → 0.25). Fall back to a 1:1 grid.
    let rate = params
        .f64("sampling_rate")
        .or_else(|| params.si("sampling_rate"))
        .filter(|r| *r > 0.0 && *r <= 100.0)
        .map(|r| r / 100.0)
        .unwrap_or(1.0);
    let grid = params
        .f64("Y_ORIG_POINTS")
        .or_else(|| params.si("Y_ORIG_POINTS"))
        .filter(|v| v.is_finite() && *v >= acquired as f64)
        .map(|v| v.round() as usize)
        .unwrap_or_else(|| ((acquired as f64 / rate).round() as usize).max(acquired));
    let idx_base = params
        .f64("nuslist_idx_base")
        .or_else(|| params.si("nuslist_idx_base"))
        .map(|v| v as usize)
        .unwrap_or(1);
    let mode = params
        .string_ci("nus_mode")
        .or_else(|| params.string_ci("auto_nus_mode"))
        .unwrap_or_else(|| "unknown".to_string());
    let echo_antiecho = params
        .string_ci("pn_type")
        .map(|s| s.trim().eq_ignore_ascii_case("y"))
        .unwrap_or(false);
    let schedule = extract_nuslist(bytes, b"Y_NUSLIST").and_then(|raw| {
        if raw.len() != acquired {
            return None;
        }
        raw.into_iter()
            .map(|value| value.checked_sub(idx_base).filter(|value| *value < grid))
            .collect::<Option<Vec<_>>>()
            .filter(|values| {
                let mut sorted = values.clone();
                sorted.sort_unstable();
                sorted.dedup();
                sorted.len() == values.len()
            })
    });
    Some(NusMeta {
        grid,
        acquired,
        idx_base,
        mode,
        echo_antiecho,
        schedule,
    })
}

/// Find a named integer-array parameter in Delta's serialized parameter tail.
/// The fixed parameter table contains the name too, so candidates are accepted
/// only when the preceding string header and following typed array both match.
pub(super) fn extract_nuslist(bytes: &[u8], wanted: &[u8]) -> Option<Vec<usize>> {
    const STRING_TAG: u32 = 0x271d;
    const INTEGER_TAG: u32 = 0x271a;
    const CONTAINER_TAG: u32 = 0x2b2a;

    let be_u32 = |at: usize| -> Option<u32> {
        let chunk: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(u32::from_be_bytes(chunk))
    };

    for (name_at, name) in bytes.windows(wanted.len()).enumerate() {
        if !name.eq_ignore_ascii_case(wanted) || name_at < 8 {
            continue;
        }
        if be_u32(name_at - 8) != Some(STRING_TAG)
            || be_u32(name_at - 4) != u32::try_from(wanted.len()).ok()
        {
            continue;
        }

        let after_name = name_at + wanted.len();
        let container_at = (after_name..after_name.saturating_add(4))
            .take_while(|at| {
                bytes
                    .get(after_name..*at)
                    .is_some_and(|pad| pad.iter().all(|b| *b == 0))
            })
            .find(|at| be_u32(*at) == Some(CONTAINER_TAG));
        let Some(container_at) = container_at else {
            continue;
        };
        let count = be_u32(container_at + 4).and_then(|v| usize::try_from(v).ok())?;
        let array_bytes = count.checked_mul(12)?;
        let mut pos = container_at.checked_add(8)?;
        if pos
            .checked_add(array_bytes)
            .is_none_or(|end| end > bytes.len())
        {
            continue;
        }

        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            if be_u32(pos) != Some(INTEGER_TAG) || be_u32(pos + 4) != Some(1) {
                values.clear();
                break;
            }
            let value = be_u32(pos + 8).and_then(|v| usize::try_from(v).ok())?;
            values.push(value);
            pos += 12;
        }
        if values.len() == count {
            return Some(values);
        }
    }
    None
}
