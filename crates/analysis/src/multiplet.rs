//! Multiplet grouping and pattern classification for 1D peak lists (all math in Hz).

pub struct MultipletPeak {
    pub position_hz: f64,
    pub intensity: f64,
    pub position_sigma_hz: Option<f64>,
    pub fwhm_hz: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    Singlet,
    Doublet,
    Triplet,
    Quartet,
    DoubletOfDoublets,
    Multiplet,
}

impl Pattern {
    pub fn label(self) -> &'static str {
        match self {
            Pattern::Singlet => "s",
            Pattern::Doublet => "d",
            Pattern::Triplet => "t",
            Pattern::Quartet => "q",
            Pattern::DoubletOfDoublets => "dd",
            Pattern::Multiplet => "m",
        }
    }
}

pub struct JValue {
    pub hz: f64,
    pub sigma_hz: Option<f64>,
}

pub struct MultipletResult {
    pub center_hz: f64,
    pub pattern: Pattern,
    pub j_values: Vec<JValue>,
    pub peak_indices: Vec<usize>,
}

pub fn group_peaks(peaks: &[MultipletPeak], max_j_hz: f64) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..peaks.len()).collect();
    order.sort_by(|&a, &b| peaks[a].position_hz.total_cmp(&peaks[b].position_hz));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for idx in order {
        match groups.last_mut() {
            Some(group)
                if peaks[idx].position_hz - peaks[*group.last().unwrap()].position_hz
                    <= max_j_hz =>
            {
                group.push(idx);
            }
            _ => groups.push(vec![idx]),
        }
    }
    groups
}

pub fn analyze(peaks: &[MultipletPeak], max_j_hz: f64) -> Vec<MultipletResult> {
    group_peaks(peaks, max_j_hz)
        .iter()
        .map(|group| classify_group(peaks, group))
        .collect()
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        values[n / 2]
    } else {
        0.5 * (values[n / 2 - 1] + values[n / 2])
    }
}

fn sigma_or_zero(p: &MultipletPeak) -> f64 {
    p.position_sigma_hz.unwrap_or(0.0)
}

fn spacing_tolerance(members: &[&MultipletPeak], spacings: &[f64]) -> f64 {
    let max_pair_sigma = members
        .windows(2)
        .map(|w| (sigma_or_zero(w[0]).powi(2) + sigma_or_zero(w[1]).powi(2)).sqrt())
        .fold(0.0_f64, f64::max);
    let mut fwhms: Vec<f64> = members.iter().map(|p| p.fwhm_hz).collect();
    let mut sp: Vec<f64> = spacings.to_vec();
    (3.0 * max_pair_sigma)
        .max(0.15 * median(&mut fwhms))
        .max(0.05 * median(&mut sp))
}

fn ratios_match(members: &[&MultipletPeak], expected: &[f64]) -> bool {
    let min = members
        .iter()
        .map(|p| p.intensity)
        .fold(f64::INFINITY, f64::min);
    if min <= 0.0 {
        return false;
    }
    members.iter().zip(expected).all(|(p, &e)| {
        let r = (p.intensity / min) / e;
        (1.0 / 1.6..=1.6).contains(&r)
    })
}

fn linear_sigma(terms: &[(f64, Option<f64>)]) -> Option<f64> {
    let mut sum = 0.0;
    for &(coeff, sigma) in terms {
        sum += (coeff * sigma?).powi(2);
    }
    Some(sum.sqrt())
}

pub fn classify_group(peaks: &[MultipletPeak], indices: &[usize]) -> MultipletResult {
    if indices.is_empty() {
        return MultipletResult {
            center_hz: 0.0,
            pattern: Pattern::Multiplet,
            j_values: vec![],
            peak_indices: vec![],
        };
    }
    let mut idx: Vec<usize> = indices.to_vec();
    idx.sort_by(|&a, &b| peaks[a].position_hz.total_cmp(&peaks[b].position_hz));
    let members: Vec<&MultipletPeak> = idx.iter().map(|&i| &peaks[i]).collect();

    let total: f64 = members.iter().map(|p| p.intensity).sum();
    let center_hz = if total != 0.0 {
        members
            .iter()
            .map(|p| p.position_hz * p.intensity)
            .sum::<f64>()
            / total
    } else {
        members.iter().map(|p| p.position_hz).sum::<f64>() / members.len() as f64
    };

    let pos = |k: usize| members[k].position_hz;
    let sig = |k: usize| members[k].position_sigma_hz;
    let spacings: Vec<f64> = members
        .windows(2)
        .map(|w| w[1].position_hz - w[0].position_hz)
        .collect();
    let tol = spacing_tolerance(&members, &spacings);
    let eq = |a: f64, b: f64| (a - b).abs() <= tol;

    let (pattern, j_values) = match members.len() {
        1 => (Pattern::Singlet, vec![]),
        2 if spacings[0] > tol && ratios_match(&members, &[1.0, 1.0]) => (
            Pattern::Doublet,
            vec![JValue {
                hz: spacings[0],
                sigma_hz: linear_sigma(&[(-1.0, sig(0)), (1.0, sig(1))]),
            }],
        ),
        3 if spacings.iter().all(|&s| s > tol)
            && eq(spacings[0], spacings[1])
            && ratios_match(&members, &[1.0, 2.0, 1.0]) =>
        {
            (
                Pattern::Triplet,
                vec![JValue {
                    hz: 0.5 * (spacings[0] + spacings[1]),
                    sigma_hz: linear_sigma(&[(-0.5, sig(0)), (0.5, sig(2))]),
                }],
            )
        }
        4 if spacings.iter().all(|&s| s > tol)
            && eq(spacings[0], spacings[1])
            && eq(spacings[1], spacings[2])
            && eq(spacings[0], spacings[2])
            && ratios_match(&members, &[1.0, 3.0, 3.0, 1.0]) =>
        {
            (
                Pattern::Quartet,
                vec![JValue {
                    hz: (spacings[0] + spacings[1] + spacings[2]) / 3.0,
                    sigma_hz: linear_sigma(&[(-1.0 / 3.0, sig(0)), (1.0 / 3.0, sig(3))]),
                }],
            )
        }
        4 if eq(spacings[0], spacings[2]) && spacings[1] > tol && {
            let min = members
                .iter()
                .map(|p| p.intensity)
                .fold(f64::INFINITY, f64::min);
            min > 0.0 && members.iter().all(|p| p.intensity / min <= 1.6)
        } =>
        {
            let j_large = 0.5 * ((pos(3) - pos(0)) + (pos(2) - pos(1)));
            let j_small = 0.5 * ((pos(1) - pos(0)) + (pos(3) - pos(2)));
            let half = &[(-0.5, sig(0)), (-0.5, sig(1)), (0.5, sig(2)), (0.5, sig(3))];
            let sigma_large = linear_sigma(half);
            let sigma_small =
                linear_sigma(&[(-0.5, sig(0)), (0.5, sig(1)), (-0.5, sig(2)), (0.5, sig(3))]);
            (
                Pattern::DoubletOfDoublets,
                vec![
                    JValue {
                        hz: j_large,
                        sigma_hz: sigma_large,
                    },
                    JValue {
                        hz: j_small,
                        sigma_hz: sigma_small,
                    },
                ],
            )
        }
        _ => (Pattern::Multiplet, vec![]),
    };

    let mut j_values = j_values;
    j_values.sort_by(|a, b| b.hz.total_cmp(&a.hz));

    MultipletResult {
        center_hz,
        pattern,
        j_values,
        peak_indices: idx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(position_hz: f64, intensity: f64) -> MultipletPeak {
        MultipletPeak {
            position_hz,
            intensity,
            position_sigma_hz: None,
            fwhm_hz: 1.0,
        }
    }

    fn peak_sigma(position_hz: f64, intensity: f64, sigma: f64) -> MultipletPeak {
        MultipletPeak {
            position_sigma_hz: Some(sigma),
            ..peak(position_hz, intensity)
        }
    }

    #[test]
    fn perfect_doublet() {
        let peaks = vec![peak(100.0, 1.0), peak(107.5, 1.0)];
        let r = classify_group(&peaks, &[0, 1]);
        assert_eq!(r.pattern, Pattern::Doublet);
        assert!((r.j_values[0].hz - 7.5).abs() < 1e-12);
        assert!((r.center_hz - 103.75).abs() < 1e-12);
    }

    #[test]
    fn perfect_triplet() {
        let peaks = vec![peak(100.0, 1.0), peak(106.0, 2.0), peak(112.0, 1.0)];
        let r = classify_group(&peaks, &[0, 1, 2]);
        assert_eq!(r.pattern, Pattern::Triplet);
        assert!((r.j_values[0].hz - 6.0).abs() < 1e-12);
    }

    #[test]
    fn perfect_quartet() {
        let peaks = vec![
            peak(100.0, 1.0),
            peak(104.0, 3.0),
            peak(108.0, 3.0),
            peak(112.0, 1.0),
        ];
        let r = classify_group(&peaks, &[0, 1, 2, 3]);
        assert_eq!(r.pattern, Pattern::Quartet);
        assert!((r.j_values[0].hz - 4.0).abs() < 1e-12);
    }

    #[test]
    fn doublet_of_doublets_recovers_both_j() {
        let peaks = vec![
            peak(-8.0, 1.0),
            peak(-4.0, 1.0),
            peak(4.0, 1.0),
            peak(8.0, 1.0),
        ];
        let r = classify_group(&peaks, &[0, 1, 2, 3]);
        assert_eq!(r.pattern, Pattern::DoubletOfDoublets);
        assert!((r.j_values[0].hz - 12.0).abs() < 1e-12);
        assert!((r.j_values[1].hz - 4.0).abs() < 1e-12);
    }

    #[test]
    fn even_spacing_wrong_ratios_is_multiplet() {
        let peaks = vec![peak(100.0, 1.0), peak(106.0, 1.0), peak(112.0, 1.0)];
        let r = classify_group(&peaks, &[0, 1, 2]);
        assert_eq!(r.pattern, Pattern::Multiplet);
        assert!(r.j_values.is_empty());
    }

    #[test]
    fn perturbed_triplet_is_multiplet() {
        let peaks = vec![peak(100.0, 1.0), peak(106.0, 2.0), peak(115.0, 1.0)];
        let r = classify_group(&peaks, &[0, 1, 2]);
        assert_eq!(r.pattern, Pattern::Multiplet);
    }

    #[test]
    fn grouping_splits_distant_multiplets() {
        let peaks = vec![
            peak(0.0, 1.0),
            peak(7.0, 1.0),
            peak(107.0, 1.0),
            peak(114.0, 1.0),
        ];
        let groups = group_peaks(&peaks, 20.0);
        assert_eq!(groups, vec![vec![0, 1], vec![2, 3]]);
        let results = analyze(&peaks, 20.0);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].pattern, Pattern::Doublet);
        assert_eq!(results[1].pattern, Pattern::Doublet);
    }

    #[test]
    fn doublet_sigma_propagation() {
        let peaks = vec![peak_sigma(100.0, 1.0, 0.1), peak_sigma(108.0, 1.0, 0.1)];
        let r = classify_group(&peaks, &[0, 1]);
        let s = r.j_values[0].sigma_hz.unwrap();
        assert!((s - 0.02_f64.sqrt()).abs() < 1e-12);
        assert!((s - 0.1414).abs() < 1e-3);
    }

    #[test]
    fn missing_sigma_gives_none() {
        let peaks = vec![peak_sigma(100.0, 1.0, 0.1), peak(108.0, 1.0)];
        let r = classify_group(&peaks, &[0, 1]);
        assert_eq!(r.pattern, Pattern::Doublet);
        assert!(r.j_values[0].sigma_hz.is_none());
    }

    #[test]
    fn spacing_difference_at_tol_boundary_is_equal() {
        let d2 = 6.15 / 0.975;
        let peaks = vec![peak(100.0, 1.0), peak(106.0, 2.0), peak(106.0 + d2, 1.0)];
        let tol = 0.05 * 0.5 * (6.0 + d2);
        assert!((d2 - 6.0 - tol).abs() < 1e-12);
        let r = classify_group(&peaks, &[0, 1, 2]);
        assert_eq!(r.pattern, Pattern::Triplet);
    }

    #[test]
    fn coincident_peaks_are_not_a_zero_j_doublet() {
        let peaks = vec![peak(100.0, 1.0), peak(100.0, 1.0)];
        let r = classify_group(&peaks, &[0, 1]);
        assert_eq!(r.pattern, Pattern::Multiplet);
        assert!(r.j_values.is_empty());
    }

    #[test]
    fn dd_sigma_propagation() {
        let peaks = vec![
            peak_sigma(-8.0, 1.0, 0.1),
            peak_sigma(-4.0, 1.0, 0.2),
            peak_sigma(4.0, 1.0, 0.3),
            peak_sigma(8.0, 1.0, 0.4),
        ];
        let r = classify_group(&peaks, &[0, 1, 2, 3]);
        assert_eq!(r.pattern, Pattern::DoubletOfDoublets);
        let expected = 0.5 * (0.01_f64 + 0.04 + 0.09 + 0.16).sqrt();
        assert!((r.j_values[0].sigma_hz.unwrap() - expected).abs() < 1e-12);
        assert!((r.j_values[1].sigma_hz.unwrap() - expected).abs() < 1e-12);

        let peaks = vec![
            peak_sigma(-8.0, 1.0, 0.1),
            peak_sigma(-4.0, 1.0, 0.1),
            peak_sigma(4.0, 1.0, 0.5),
            peak_sigma(8.0, 1.0, 0.5),
        ];
        let r = classify_group(&peaks, &[0, 1, 2, 3]);
        let expected = 0.5 * (0.01_f64 + 0.01 + 0.25 + 0.25).sqrt();
        assert!((r.j_values[0].sigma_hz.unwrap() - expected).abs() < 1e-12);
        assert!((r.j_values[1].sigma_hz.unwrap() - expected).abs() < 1e-12);
    }

    #[test]
    fn five_peaks_no_pattern_is_multiplet() {
        let peaks = vec![
            peak(100.0, 1.0),
            peak(103.0, 2.5),
            peak(105.0, 3.0),
            peak(108.5, 2.0),
            peak(110.0, 1.5),
        ];
        let r = classify_group(&peaks, &[0, 1, 2, 3, 4]);
        assert_eq!(r.pattern, Pattern::Multiplet);
        assert!(r.j_values.is_empty());
        let total = 1.0 + 2.5 + 3.0 + 2.0 + 1.5;
        let expected = (100.0 + 103.0 * 2.5 + 105.0 * 3.0 + 108.5 * 2.0 + 110.0 * 1.5) / total;
        assert!((r.center_hz - expected).abs() < 1e-9);
    }
}
