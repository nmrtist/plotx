use super::*;

fn table_app(columns: &[(&str, Vec<f64>)]) -> PlotxApp {
    let rows = columns.iter().map(|(_, y)| y.len()).max().unwrap_or(0);
    let series = columns
        .iter()
        .map(|(name, values)| FloatSeries {
            name: (*name).to_owned(),
            unit: String::new(),
            values: values
                .iter()
                .copied()
                .map(Some)
                .chain(std::iter::repeat_n(None, rows - values.len()))
                .collect(),
            uncertainty: None,
            fit: None,
        })
        .collect();
    let table = materialized_float_series_table(
        (
            "row".into(),
            "".into(),
            (0..rows).map(|i| Some(i as f64)).collect(),
        ),
        series,
        "plotx.test.statistics-table.v1",
    )
    .unwrap();
    let mut app = PlotxApp::new();
    app.doc.datasets.push(Dataset::Table(Box::new(table)));
    app
}

fn stored(app: &PlotxApp) -> &[StatAnalysis] {
    &app.doc.datasets[0].as_table().unwrap().statistics
}

fn column_id(app: &PlotxApp, index: usize) -> plotx_data::ColumnId {
    app.doc.datasets[0]
        .as_table()
        .unwrap()
        .numeric_analysis_columns()[index]
        .0
}

fn draft(app: &PlotxApp) -> StatDraft {
    let columns = app.doc.datasets[0]
        .as_table()
        .unwrap()
        .numeric_analysis_columns()
        .into_iter()
        .map(|(column, _)| column)
        .collect::<Vec<_>>();
    StatDraft::new(0, &columns)
}

#[test]
fn descriptive_excludes_non_finite_cells_and_records_the_used_count() {
    let mut app = table_app(&[("control", vec![1.0, 2.0, f64::NAN, 4.0])]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::Summarize;
    draft.columns = vec![column_id(&app, 0)];

    let preflight = app.statistics_preflight(&draft);
    assert!(preflight.role_error.is_none());
    assert!(preflight.needs_confirmation(), "a NaN cell must be flagged");

    app.run_statistics(&draft).unwrap();
    let analysis = &stored(&app)[0];
    assert!(analysis.data_note.is_some());
    let selection = &analysis.selection.selections[0];
    let table = app.doc.datasets[0].as_table().unwrap();
    assert_eq!(
        analysis.selection.source_revision,
        table.typed_state.envelope.revision.id
    );
    assert_eq!(
        selection.columns,
        vec![table.series_bindings[0].value_column]
    );
    assert_eq!(selection.included_rows.len(), 3);
    assert_eq!(selection.excluded_rows.len(), 1);
    assert_eq!(
        selection.excluded_rows[0].row.to_string(),
        table.typed_rows(4, &[]).unwrap().row_ids[2].to_string()
    );
    assert_eq!(
        selection.excluded_rows[0].cells[0].reason,
        StatExclusionReason::NonFinite
    );
    let StatOutcome::Descriptive(records) = &analysis.outcome else {
        panic!("descriptive outcome");
    };
    assert_eq!(records[0].count, 3, "the NaN row is excluded from n");
    assert!((records[0].mean - 7.0 / 3.0).abs() < 1e-9);
}

#[test]
fn independent_welch_reports_signed_left_minus_right_difference() {
    let mut app = table_app(&[
        ("treated", vec![5.1, 5.4, 5.0, 5.3]),
        ("control", vec![4.2, 4.5, 4.1, 4.4]),
    ]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::CompareTwoGroups;
    draft.column_a = column_id(&app, 0);
    draft.column_b = column_id(&app, 1);
    draft.variance = VarianceModel::Welch;

    app.run_statistics(&draft).unwrap();
    let StatOutcome::TTest(result) = &stored(&app)[0].outcome else {
        panic!("t-test outcome");
    };
    assert!(matches!(
        result.kind,
        TTestKind::Independent(VarianceModel::Welch)
    ));
    assert_eq!(result.left_label, "treated");
    assert_eq!(result.right_label.as_deref(), Some("control"));
    assert!(result.estimate > 0.0, "treated mean exceeds control");
    assert_eq!(result.count_left, 4);
    assert_eq!(result.count_right, Some(4));
}

#[test]
fn paired_test_uses_only_complete_row_pairs() {
    let mut app = table_app(&[
        ("before", vec![1.0, 2.0, 3.0, 4.0, f64::NAN]),
        ("after", vec![1.5, 2.4, 3.3, f64::NAN, 5.0]),
    ]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::ComparePaired;
    draft.column_a = column_id(&app, 0);
    draft.column_b = column_id(&app, 1);

    let preflight = app.statistics_preflight(&draft);
    assert!(preflight.needs_confirmation(), "partial rows are dropped");

    app.run_statistics(&draft).unwrap();
    let StatOutcome::TTest(result) = &stored(&app)[0].outcome else {
        panic!("t-test outcome");
    };
    assert!(matches!(result.kind, TTestKind::Paired));
    assert_eq!(
        result.count_left, 3,
        "only the first three rows are complete pairs"
    );
    assert!(result.estimate < 0.0, "before minus after is negative");
}

#[test]
fn one_way_anova_names_groups_and_runs_tukey_regardless_of_omnibus() {
    let mut app = table_app(&[
        ("low", vec![1.0, 1.2, 0.9, 1.1]),
        ("mid", vec![2.0, 2.1, 1.9, 2.2]),
        ("high", vec![3.0, 3.1, 2.9, 3.2]),
    ]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::CompareManyGroups;
    draft.group_columns = (0..3).map(|index| column_id(&app, index)).collect();
    draft.run_tukey = true;

    app.run_statistics(&draft).unwrap();
    let StatOutcome::OneWay(result) = &stored(&app)[0].outcome else {
        panic!("one-way outcome");
    };
    assert_eq!(result.groups.len(), 3);
    assert_eq!(result.groups[2].label, "high");
    let tukey = result.tukey.as_ref().expect("tukey ran");
    assert_eq!(tukey.comparisons.len(), 3, "three pairwise comparisons");
    assert_eq!(tukey.comparisons[0].group_a, "low");
}

#[test]
fn two_way_detects_numeric_levels_and_flags_missing_replication() {
    // A 2x2 design with one observation per cell: interaction is not estimable.
    let mut app = table_app(&[
        ("yield", vec![10.0, 12.0, 20.0, 24.0]),
        ("dose", vec![0.0, 0.0, 1.0, 1.0]),
        ("time", vec![0.0, 1.0, 0.0, 1.0]),
    ]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::TwoFactors;
    draft.value_column = column_id(&app, 0);
    draft.factor_a_column = column_id(&app, 1);
    draft.factor_b_column = column_id(&app, 2);

    let (levels_a, levels_b) = app.factor_levels_preview(&draft).unwrap();
    assert_eq!(levels_a, vec!["dose = 0", "dose = 1"]);
    assert_eq!(levels_b, vec!["time = 0", "time = 1"]);

    app.run_statistics(&draft).unwrap();
    let StatOutcome::TwoWay(result) = &stored(&app)[0].outcome else {
        panic!("two-way outcome");
    };
    assert_eq!(result.replication, TwoWayReplication::Without);
    assert!(
        result.interaction.is_none(),
        "no replication, no interaction"
    );
}

#[test]
fn preflight_blocks_two_identical_columns() {
    let app = table_app(&[("a", vec![1.0, 2.0, 3.0]), ("b", vec![4.0, 5.0, 6.0])]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::CompareTwoGroups;
    draft.column_a = column_id(&app, 0);
    draft.column_b = column_id(&app, 0);
    let preflight = app.statistics_preflight(&draft);
    assert_eq!(
        preflight.role_error.as_deref(),
        Some("Choose two different columns.")
    );
}

#[test]
fn constant_column_yields_an_actionable_error_not_a_panic() {
    let mut app = table_app(&[("flat", vec![2.0, 2.0, 2.0, 2.0])]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::CompareToValue;
    draft.column_a = column_id(&app, 0);
    draft.reference_value = 0.0;
    let error = app.run_statistics(&draft).unwrap_err();
    assert!(error.contains("no variation"), "got: {error}");
    assert!(stored(&app).is_empty(), "a failed run stores nothing");
}

#[test]
fn running_and_undoing_a_result_is_one_reversible_step() {
    let mut app = table_app(&[("x", vec![1.0, 2.0, 3.0, 4.0])]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::Summarize;
    draft.columns = vec![column_id(&app, 0)];

    app.run_statistics(&draft).unwrap();
    assert_eq!(stored(&app).len(), 1);
    app.undo();
    assert!(stored(&app).is_empty());
    app.redo();
    assert_eq!(stored(&app).len(), 1);
}

#[test]
fn add_to_board_materializes_a_derived_table_with_lineage() {
    let mut app = table_app(&[("g1", vec![1.0, 2.0, 3.0]), ("g2", vec![2.0, 3.0, 4.0])]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::CompareManyGroups;
    draft.group_columns = vec![column_id(&app, 0), column_id(&app, 1)];
    draft.run_tukey = false;
    app.run_statistics(&draft).unwrap();
    let id = stored(&app)[0].id;

    app.add_statistics_result_to_board(0, id).unwrap();
    let derived = app.doc.datasets.last().unwrap();
    assert_eq!(
        derived.lineage(),
        Some(&DatasetLineage::new(
            DerivationKind::StatisticsTable,
            [app.doc.datasets[0].resource_id()]
        ))
    );
    // One column per named group keeps the source column identity.
    let table = derived.as_table().unwrap();
    let headers: Vec<&str> = table
        .typed_state
        .envelope
        .revision
        .snapshot
        .schema
        .columns
        .iter()
        .skip(1)
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(headers, vec!["g1", "g2"]);
}

#[test]
fn scalar_results_are_copy_only_not_board_tables() {
    let mut app = table_app(&[("a", vec![1.0, 2.0, 3.0]), ("b", vec![2.0, 1.0, 3.0])]);
    let mut draft = draft(&app);
    draft.question = StatQuestion::Relationship;
    draft.column_a = column_id(&app, 0);
    draft.column_b = column_id(&app, 1);
    draft.correlation = CorrelationKind::Pearson;
    app.run_statistics(&draft).unwrap();
    let analysis = &stored(&app)[0];
    assert!(!analysis.outcome.supports_table());
    // The labelled report is still available for the clipboard.
    assert!(report_text(analysis).contains("correlation"));
}
