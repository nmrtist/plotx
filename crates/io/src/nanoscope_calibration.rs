pub(super) fn normalize_afm_unit(unit: &str) -> String {
    match unit.trim() {
        "~m" | "um" => "µm".to_owned(),
        other => other.to_owned(),
    }
}

pub(super) fn first_number(value: &str) -> Option<f64> {
    value
        .split(|character: char| {
            !(character.is_ascii_digit() || matches!(character, '.' | '-' | '+' | 'e' | 'E'))
        })
        .find_map(|token| (!token.is_empty()).then(|| token.parse().ok()).flatten())
}

pub(super) fn physical_unit(value: &str) -> Option<&str> {
    value
        .split_whitespace()
        .find(|token| token.chars().any(char::is_alphabetic))
}

pub(super) fn sensitivity_scale(value: &str) -> Option<(f64, String)> {
    let multiplier = first_number(parenthesized(value).unwrap_or(value))?;
    let unit = value
        .split_whitespace()
        .find(|token| token.to_ascii_lowercase().contains("/v"))?;
    let numerator = unit.split('/').next()?.trim_matches(|character: char| {
        !character.is_alphanumeric() && character != 'µ' && character != '~'
    });
    (!numerator.is_empty() && multiplier.is_finite())
        .then(|| (multiplier, normalize_afm_unit(numerator)))
}

pub(super) fn calibrated_si_value(value: &str, target: &str) -> Option<f64> {
    let number = first_number(parenthesized(value).unwrap_or(value))?;
    let lower = value.to_ascii_lowercase().replace('µ', "u");
    let factor = match target {
        "m/v" if lower.contains("nm/v") => 1e-9,
        "m/v" if lower.contains("um/v") => 1e-6,
        "m/v" if lower.contains("mm/v") => 1e-3,
        _ => 1.0,
    };
    Some(number * factor)
}

pub(super) fn is_physical_deflection_unit(unit: &str) -> bool {
    let lower = unit.to_ascii_lowercase();
    ["m", "nm", "um", "µm", "pm"]
        .iter()
        .any(|physical| lower == *physical || lower.starts_with(&format!("{physical}/")))
}

pub(super) fn value_with_si(value: f64, unit: &str) -> f64 {
    let _ = unit;
    value
}

fn parenthesized(value: &str) -> Option<&str> {
    let start = value.rfind('(')? + 1;
    let end = value[start..].find(')')? + start;
    Some(&value[start..end])
}
