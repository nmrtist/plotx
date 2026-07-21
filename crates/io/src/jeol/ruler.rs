//! Decoding of JEOL scaler bytes, display units, and the embedded arrayed-axis
//! (pseudo-2D ruler) text.

use crate::{AxisSource, PseudoKind};

/// Decode a JEOL scaler byte to the base-10 exponent it applies to a stored
/// value. The high nibble is a signed SI-prefix index: 0→10⁰, 1→milli, 2→micro,
/// 3→nano, …, and 0xF→kilo, 0xE→mega for the positive prefixes.
pub(super) fn prefix_exponent(scaler: u8) -> i32 {
    let n = (scaler >> 4) as i32;
    if n < 8 { -3 * n } else { -3 * (n - 16) }
}

/// Convert a `value[unit]` display unit to an SI multiplier, e.g. `ms → 1e-3`,
/// `mT/m → 1e-3`, `G/cm → 1e-2`. Unrecognised units map to 1.0.
fn unit_to_si(unit: &str) -> f64 {
    match unit.trim() {
        "s" => 1.0,
        "ms" => 1e-3,
        "us" | "µs" => 1e-6,
        "ns" => 1e-9,
        "T/m" => 1.0,
        "mT/m" => 1e-3,
        "G/cm" => 1e-2, // 1 gauss/cm = 1e-4 T / 1e-2 m = 1e-2 T/m
        "G/mm" => 0.1,
        _ => 1.0,
    }
}

pub(super) fn kind_for_unit(unit: &str) -> PseudoKind {
    match unit.trim() {
        "s" | "ms" | "us" | "µs" | "ns" => PseudoKind::Delay,
        "T/m" | "mT/m" | "G/cm" | "G/mm" => PseudoKind::Gradient,
        _ => PseudoKind::Generic,
    }
}

/// Parse a single `123.4[unit]` token into `(value, unit)`; the value is left in
/// its display unit (the caller applies `unit_to_si`).
fn parse_quantity_token(tok: &str) -> Option<(f64, String)> {
    let tok = tok.trim();
    let open = tok.find('[')?;
    let close = tok.find(']')?;
    if close < open {
        return None;
    }
    let value: f64 = tok[..open].trim().parse().ok()?;
    let unit = tok[open + 1..close].trim().to_string();
    Some((value, unit))
}

/// Scan the embedded experiment text for the arrayed indirect axis. JEOL writes
/// it as `name  => y_acq {v1[u], v2[u], …}` (explicit list) or
/// `name  => y_acq start[u]..stop[u] : step[u]` (linear ramp). Returns the SI
/// values, the (display) unit, and which form was found.
pub(super) fn scan_embedded_axis(bytes: &[u8]) -> Option<(String, Vec<f64>, String, AxisSource)> {
    // Work over a lossy-ASCII view; the experiment text is plain ASCII.
    let text = String::from_utf8_lossy(bytes);
    let marker = "y_acq";
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find(marker) {
        let at = search_from + rel;
        search_from = at + marker.len();

        // Recover the parameter name: the identifier just before "=>"/"=?".
        let name = text[..at]
            .rfind(['>', '?'])
            .map(|arrow| text[..arrow].trim_end_matches(['=', ' ']).to_string())
            .and_then(|s| s.rsplit([' ', '\n', '\t', ';']).next().map(str::to_string))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "increment".to_string());

        let rest = text[at + marker.len()..].trim_start();

        // Explicit list form: { … }.
        if let Some(stripped) = rest.strip_prefix('{')
            && let Some(end) = stripped.find('}')
        {
            let mut unit = String::new();
            let values: Vec<f64> = stripped[..end]
                .split(',')
                .filter_map(|tok| {
                    let (v, u) = parse_quantity_token(tok)?;
                    if unit.is_empty() {
                        unit = u.clone();
                    }
                    Some(v * unit_to_si(&u))
                })
                .collect();
            if values.len() >= 2 {
                return Some((name, values, unit, AxisSource::EmbeddedList));
            }
        }

        // Ramp form: start[u]..stop[u] : step[u]. Each token carries its own
        // unit (start may be mT/m while stop is T/m), so convert independently.
        let ramp = rest.split(['\n', ',']).next().unwrap_or(rest);
        if let Some((lo_s, hi_step)) = ramp.split_once("..") {
            let (hi_s, step_s) = hi_step.split_once(':').unwrap_or((hi_step, ""));
            if let (Some((lo, lu)), Some((hi, hu)), Some((step, su))) = (
                parse_quantity_token(lo_s),
                parse_quantity_token(hi_s),
                parse_quantity_token(step_s),
            ) {
                let lo_si = lo * unit_to_si(&lu);
                let hi_si = hi * unit_to_si(&hu);
                let step_si = (step * unit_to_si(&su)).abs();
                if step_si > 0.0 && hi_si.is_finite() && lo_si.is_finite() {
                    let mut values = Vec::new();
                    let n = ((hi_si - lo_si) / step_si).round() as i64;
                    for i in 0..=n.max(0) {
                        values.push(lo_si + step_si * i as f64);
                    }
                    if values.len() >= 2 {
                        return Some((name, values, lu, AxisSource::EmbeddedRamp));
                    }
                }
            }
        }
    }
    None
}

pub(super) fn ascii_trim(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim().to_string()
}
