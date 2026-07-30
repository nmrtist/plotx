use super::*;

pub(super) fn read_peaks_2d(dataset: &mut Nmr2DDataset, recipe: &RecipeObject) -> Result<()> {
    dataset.peaks = parse_peaks_2d(&recipe.extensions)?;
    dataset
        .peaks
        .validate()
        .map_err(|error| ProjectError::Invalid(format!("plotx.analysis.peaks_2d: {error}")))?;
    dataset.peaks.reseed();
    Ok(())
}

fn parse_peaks_2d(extensions: &serde_json::Value) -> Result<crate::state::Peak2DSet> {
    let stored = extensions
        .get("plotx.analysis")
        .and_then(|analysis| analysis.get("peaks_2d"))
        .cloned();
    match stored {
        Some(value) => serde_json::from_value(value).map_err(|error| {
            ProjectError::Invalid(format!("plotx.analysis.peaks_2d is malformed: {error}"))
        }),
        None => Ok(crate::state::Peak2DSet::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_peaks_are_empty_but_malformed_peaks_are_an_error() {
        assert!(
            parse_peaks_2d(&serde_json::json!({}))
                .unwrap()
                .marks
                .is_empty()
        );
        let error = parse_peaks_2d(&serde_json::json!({
            "plotx.analysis": {
                "peaks_2d": {
                    "marks": [{ "id": "not a number" }],
                    "next_id": 1
                }
            }
        }))
        .unwrap_err();
        assert!(error.to_string().contains("peaks_2d is malformed"));
    }
}
