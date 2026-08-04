use super::*;
use crate::{LcGradientPoint, LiquidChromatographyMethod};

/// Read the human-readable inlet method embedded in a MassLynx RAW bundle.
/// A missing method is distinct from a malformed method because callers can
/// supply an explicit method for otherwise valid LC–MS data.
pub fn load(path: &Path) -> Result<Option<LiquidChromatographyMethod>, IoError> {
    let bundle = Bundle::discover(path)?;
    let Some(path) = bundle.file("_inlet.inf") else {
        return Ok(None);
    };
    parse(&std::fs::read(path)?).map(Some)
}

pub(super) fn parse(bytes: &[u8]) -> Result<LiquidChromatographyMethod, IoError> {
    let text = String::from_utf8_lossy(bytes);
    let mut name = None;
    let mut run_time_min = None;
    let mut solvent_a = None;
    let mut solvent_b = None;
    let mut gradient = Vec::new();
    let mut detector_wavelengths_nm = Vec::new();
    let mut column = None;
    let mut in_gradient = false;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if let Some(value) = value_after(line, "Inlet Method File:") {
            name = Some(clean(value));
        } else if run_time_min.is_none()
            && let Some(value) = value_after(line, "Run Time:")
        {
            run_time_min = first_number(value);
        } else if let Some(value) = value_after(line, "Solvent Name A:") {
            solvent_a = Some(clean(value));
        } else if let Some(value) = value_after(line, "Solvent Name B:") {
            solvent_b = Some(clean(value));
        } else if line.eq_ignore_ascii_case("[Gradient Table]") {
            in_gradient = true;
        } else if in_gradient && line.starts_with("Run Events:") {
            in_gradient = false;
        } else if in_gradient {
            if let Some(point) = gradient_point(line)? {
                gradient.push(point);
            }
        } else if let Some(value) = value_after(line, "Wavelength:")
            && let Some(wavelength) = first_number(value)
        {
            detector_wavelengths_nm.push(wavelength);
        } else if let Some(value) = value_after(line, "Column Type:") {
            column = Some(clean(value));
        }
    }

    detector_wavelengths_nm.sort_by(f64::total_cmp);
    detector_wavelengths_nm.dedup_by(|left, right| left.total_cmp(right).is_eq());
    let method = LiquidChromatographyMethod {
        name,
        run_time_min: run_time_min.ok_or_else(|| invalid("_INLET.INF has no pump run time"))?,
        solvent_a,
        solvent_b,
        gradient,
        detector_wavelengths_nm,
        column,
    };
    method.validate().map_err(invalid)?;
    Ok(method)
}

fn gradient_point(line: &str) -> Result<Option<LcGradientPoint>, IoError> {
    let mut tokens = line.split_whitespace();
    let Some(index) = tokens.next() else {
        return Ok(None);
    };
    if !index.ends_with('.')
        || !index[..index.len() - 1]
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return Ok(None);
    }
    let values = tokens.collect::<Vec<_>>();
    let (time, flow, percent_b) = if values
        .first()
        .is_some_and(|value| value.eq_ignore_ascii_case("Initial"))
    {
        (
            0.0,
            parse_number(values.get(1), "initial flow rate")?,
            parse_number(values.get(3), "initial %B")?,
        )
    } else {
        (
            parse_number(values.first(), "gradient time")?,
            parse_number(values.get(1), "gradient flow rate")?,
            parse_number(values.get(3), "gradient %B")?,
        )
    };
    Ok(Some(LcGradientPoint {
        time_min: time,
        flow_ml_min: flow,
        percent_b,
    }))
}

fn parse_number(value: Option<&&str>, label: &str) -> Result<f64, IoError> {
    value
        .and_then(|value| value.parse().ok())
        .filter(|value: &f64| value.is_finite())
        .ok_or_else(|| invalid(format!("_INLET.INF has invalid {label}")))
}

fn first_number(value: &str) -> Option<f64> {
    value
        .split_whitespace()
        .find_map(|token| token.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn value_after<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    line.get(..label.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(label))
        .map(|_| line[label.len()..].trim())
        .filter(|value| !value.is_empty())
}

fn clean(value: &str) -> String {
    value.replace('\u{fffd}', "").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_acquity_gradient_and_detector_details() {
        let method = parse(
            br#"Inlet Method File: d:\methods\5-95
-- PUMP --
 Run Time: 10.00 min
 Solvent Name A: Water + acid
 Solvent Name B: Acetonitrile + acid
 [Gradient Table]
  Time(min) Flow Rate %A %B Curve
 1. Initial 0.300 95.0 5.0 Initial
 2. 6.00 0.300 5.0 95.0 6
 3. 8.00 0.300 5.0 95.0 1
 4. 10.00 0.300 95.0 5.0 1
 Run Events: Yes
 Wavelength: 214 nm
 Wavelength: 254 nm
 Column Type: ACQUITY Protein BEH C4
"#,
        )
        .unwrap();
        assert_eq!(method.gradient.len(), 4);
        assert_eq!(method.percent_b_at(3.0), Some(50.0));
        assert_eq!(method.detector_wavelengths_nm, [214.0, 254.0]);
        assert_eq!(method.column.as_deref(), Some("ACQUITY Protein BEH C4"));
    }
}
