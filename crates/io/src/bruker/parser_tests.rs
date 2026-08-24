use super::*;

#[test]
fn parses_scalar_and_skips_arrays() {
    let text = "\
##TITLE= params
##$TD= 16384
##$NUC1= <1H>
##$SW_h= 9615.38461538464
##$GRPDLY= 76
##$XGF= (0..3)
0 0 0 0
##$O1= 2820.61
";
    let params = JcampParams::parse(text);
    assert_eq!(params.usize("TD"), Some(16384));
    assert_eq!(params.string("NUC1").as_deref(), Some("<1H>"));
    assert_eq!(params.f64("GRPDLY"), Some(76.0));
    assert_eq!(params.f64("O1"), Some(2820.61));
    assert_eq!(params.string("XGF"), None);
}

#[test]
fn group_delay_prefers_explicit_grpdly() {
    let params = JcampParams::parse("##$GRPDLY= 67.98\n##$DSPFVS= 21\n##$DECIM= 2080\n");
    assert!((group_delay(&params) - 67.98).abs() < 1e-9);
}

#[test]
fn group_delay_falls_back_to_table() {
    let params = JcampParams::parse("##$GRPDLY= -1\n##$DSPFVS= 12\n##$DECIM= 16\n");
    assert!((group_delay(&params) - 71.625).abs() < 1e-9);
}

#[test]
fn deinterleaves_complex_f64() {
    // TD = 4 real values → 2 complex points: (1+2i), (3+4i).
    let mut buffer = Vec::new();
    for value in [1.0f64, 2.0, 3.0, 4.0] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    let reader = Reader {
        bytes: &buffer,
        endian: Endian::Little,
    };
    let first = Complex64::new(
        reader.real(0, SampleFmt::F64),
        reader.real(8, SampleFmt::F64),
    );
    let second = Complex64::new(
        reader.real(16, SampleFmt::F64),
        reader.real(24, SampleFmt::F64),
    );
    assert_eq!(first, Complex64::new(1.0, 2.0));
    assert_eq!(second, Complex64::new(3.0, 4.0));
}
