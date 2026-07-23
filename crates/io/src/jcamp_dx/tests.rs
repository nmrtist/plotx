use super::*;

fn header(extra: &str, body: &str) -> String {
    format!(
        "##TITLE=fixture\n\
             ##JCAMP-DX=5.01\n\
             ##DATA TYPE=NMR SPECTRUM\n\
             ##XUNITS=PPM\n\
             ##YUNITS=ARBITRARY UNITS\n\
             ##XFACTOR=1\n\
             ##YFACTOR=1\n\
             ##FIRSTX=0\n\
             ##LASTX=3\n\
             ##NPOINTS=4\n\
             ##.OBSERVE FREQUENCY=400\n\
             ##.OBSERVE NUCLEUS=^1H\n\
             {extra}\
             ##XYDATA=(X++(Y..Y))\n\
             {body}\n\
             ##END=\n"
    )
}

fn data(text: &str) -> NmrData {
    match parse_bytes(text.as_bytes(), "fixture.jdx").unwrap() {
        Acquisition::D1(data) => data,
        Acquisition::D2(_) => panic!("expected 1D data"),
        Acquisition::Electrophysiology(_) => panic!("expected NMR"),
        Acquisition::Afm(_) => panic!("expected NMR"),
    }
}

#[test]
fn decodes_affn_and_pac() {
    let spectrum = data(&header("", "0 1+2-3+4"));
    let values: Vec<f64> = spectrum.points.iter().map(|point| point.re).collect();
    assert_eq!(values, vec![1.0, 2.0, -3.0, 4.0]);
}

#[test]
fn decodes_sqz_dif_dup_and_checkpoint_continuity() {
    let fixture = "##TITLE=compressed\n\
                       ##DATA TYPE=NMR SPECTRUM\n\
                       ##XUNITS=PPM\n\
                       ##YUNITS=RELATIVE INTENSITY\n\
                       ##XFACTOR=1\n\
                       ##YFACTOR=0.5\n\
                       ##FIRSTX=0\n\
                       ##LASTX=7\n\
                       ##NPOINTS=8\n\
                       ##.OBSERVE FREQUENCY=400\n\
                       ##.OBSERVE NUCLEUS=<1H>\n\
                       ##XYDATA=(X++(Y..Y))\n\
                       0A0KU\n\
                       3A6%TjN\n\
                       7B0 $$ final DIF checkpoint\n\
                       ##END=\n";
    let spectrum = data(fixture);
    let values: Vec<f64> = spectrum.points.iter().map(|point| point.re).collect();
    assert_eq!(values, vec![5.0, 6.0, 7.0, 8.0, 8.0, 8.0, 7.5, 10.0]);
}

#[test]
fn applies_factors_and_canonicalizes_a_descending_axis() {
    let fixture = "##TITLE=reverse\n\
                       ##DATA TYPE=NMR SPECTRUM\n\
                       ##XUNITS=PPM\n\
                       ##YUNITS=ARBITRARY UNITS\n\
                       ##XFACTOR=0.5\n\
                       ##YFACTOR=0.25\n\
                       ##FIRSTX=10\n\
                       ##LASTX=7\n\
                       ##NPOINTS=4\n\
                       ##.OBSERVE FREQUENCY=400\n\
                       ##.OBSERVE NUCLEUS=1H\n\
                       ##XYDATA=(X++(Y..Y))\n\
                       20 2 4\n\
                       16 6 8\n\
                       ##END=\n";
    let spectrum = data(fixture);
    let values: Vec<f64> = spectrum.points.iter().map(|point| point.re).collect();
    assert_eq!(values, vec![2.0, 1.5, 1.0, 0.5]);
    assert!((spectrum.spectral_width_hz - 1600.0).abs() < 1.0e-12);
    assert!((spectrum.carrier_ppm - 9.0).abs() < 1.0e-12);
}

#[test]
fn rejects_compound_and_ntuples_documents() {
    let link = header("##BLOCKS=2\n##DATA TYPE=LINK\n", "0 1 2 3 4");
    assert!(matches!(
        parse_bytes(link.as_bytes(), "link.jdx"),
        Err(JcampDxError::DuplicateLabel { .. }) | Err(JcampDxError::LinkDataset)
    ));

    let ntuples = header("##NTUPLES=NMR SPECTRUM\n", "0 1 2 3 4");
    assert!(matches!(
        parse_bytes(ntuples.as_bytes(), "ntuples.jdx"),
        Err(JcampDxError::NtuplesDataset)
    ));
}

#[test]
fn rejects_missing_metadata_unsupported_units_and_bad_checkpoints() {
    let missing = header("", "0 1 2 3 4").replace("##.OBSERVE NUCLEUS=^1H\n", "");
    assert!(matches!(
        parse_bytes(missing.as_bytes(), "missing.jdx"),
        Err(JcampDxError::MissingLabel("OBSERVE NUCLEUS"))
    ));

    let unit = header("", "0 1 2 3 4").replace("##XUNITS=PPM", "##XUNITS=SECONDS");
    assert!(matches!(
        parse_bytes(unit.as_bytes(), "unit.jdx"),
        Err(JcampDxError::UnsupportedUnit { axis: "X", .. })
    ));

    let checkpoint = header("", "0A0K\n1A3KK\n3A6");
    assert!(matches!(
        parse_bytes(checkpoint.as_bytes(), "checkpoint.jdx"),
        Err(JcampDxError::Checkpoint { .. })
    ));
}
