use super::*;

fn invalid_stack_pipelines() -> Vec<AxisPipelineDto> {
    let params = Params2D::default_for(Preset2D::Dosy);
    let mut f1 = params.f1;
    let phase = f1
        .steps
        .iter()
        .position(|step| matches!(step.kind, StepKind::Phase(_)))
        .map(|index| f1.steps.remove(index))
        .expect("default F1 recipe has phase");
    f1.steps.insert(0, phase);
    vec![pipeline_to_dto(&params.f2), pipeline_to_dto(&f1)]
}

#[test]
fn stack_scheme_rejects_an_invalid_dormant_f1_pipeline() {
    let target = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(
        super::tests::synthetic_dosy_2d(),
    )));
    let scheme = ProcessingScheme {
        schema_version: 1,
        dimension_count: 2,
        pipelines: invalid_stack_pipelines(),
        layout: Some("stack".to_owned()),
        group_delay_correct: true,
    };

    assert!(apply_scheme(&scheme, &target).is_err());
}

#[test]
fn project_recipe_rejects_an_invalid_2d_pipeline_before_retransform() {
    let mut dataset = Nmr2DDataset::load(super::tests::synthetic_dosy_2d());
    let recipe = RecipeObject {
        id: "recipe_000000".to_owned(),
        role: "recipe".to_owned(),
        classification: Classification {
            domain: "spectroscopy".to_owned(),
            technique: Some("nmr".to_owned()),
            object: "recipe".to_owned(),
        },
        input: "data_000000".to_owned(),
        parameters: RecipeParameters {
            dimension_count: 2,
            pipelines: invalid_stack_pipelines(),
            group_delay_correct: true,
            layout: Some("stack".to_owned()),
            preset: Some("dosy".to_owned()),
        },
        extensions: serde_json::Value::Null,
    };

    assert!(matches!(
        apply_2d_recipe(&mut dataset, &recipe),
        Err(ProjectError::Invalid(message)) if message.contains("invalid F1 pipeline")
    ));
}
