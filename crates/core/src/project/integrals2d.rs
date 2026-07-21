use super::*;

pub(super) fn read_integrals_2d(dataset: &mut Nmr2DDataset, recipe: &RecipeObject) -> Result<()> {
    dataset.integrals = parse_integrals_2d(&recipe.extensions)?;
    dataset.reseed_integral_ids();
    dataset.renormalize_integrals();
    Ok(())
}

fn parse_integrals_2d(extensions: &serde_json::Value) -> Result<Vec<crate::Integral2D>> {
    let stored = extensions
        .get("plotx.analysis")
        .and_then(|analysis| analysis.get("integrals_2d"))
        .cloned();
    match stored {
        Some(value) => serde_json::from_value(value).map_err(|error| {
            ProjectError::Invalid(format!("plotx.analysis.integrals_2d is malformed: {error}"))
        }),
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_integrals_are_empty_but_malformed_integrals_are_an_error() {
        assert!(
            parse_integrals_2d(&serde_json::json!({}))
                .unwrap()
                .is_empty()
        );
        let error = parse_integrals_2d(&serde_json::json!({
            "plotx.analysis": { "integrals_2d": [{ "id": "not a number" }] }
        }))
        .unwrap_err();
        assert!(error.to_string().contains("integrals_2d is malformed"));
    }
}
