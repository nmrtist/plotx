use super::*;

fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| a + (b - a) * i as f64 / (n - 1) as f64)
        .collect()
}

fn noise(i: usize) -> f64 {
    1e-3 * (i as f64 * 12.9898).sin()
}

fn lorentz(x0: f64, h: f64, w: f64, x: f64) -> f64 {
    let hw2 = (w / 2.0) * (w / 2.0);
    h * hw2 / ((x - x0) * (x - x0) + hw2)
}

fn gauss(x0: f64, h: f64, w: f64, x: f64) -> f64 {
    h * (-4.0 * LN_2 * (x - x0) * (x - x0) / (w * w)).exp()
}

fn assert_close(got: f64, want: f64, rel: f64) {
    assert!(
        (got - want).abs() <= rel * want.abs().max(1e-12),
        "got {got}, want {want}"
    );
}

#[test]
fn recovers_three_overlapping_lorentzians() {
    let xs = linspace(0.0, 10.0, 500);
    let truth = [(4.0, 10.0, 0.5), (4.8, 6.0, 0.7), (6.0, 8.0, 0.4)];
    let offset = 0.5;
    let ys: Vec<f64> = xs
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            offset
                + noise(i)
                + truth
                    .iter()
                    .map(|&(x0, h, w)| lorentz(x0, h, w, x))
                    .sum::<f64>()
        })
        .collect();
    let seeds = seed_peaks(&xs, &ys, &[3.95, 4.85, 6.05]);
    assert_eq!(seeds.len(), 3);
    let fit = fit_lineshapes(&xs, &ys, LineShape::Lorentzian, &seeds).expect("fit");
    assert_eq!(fit.peaks.len(), 3);
    for (pk, &(x0, h, w)) in fit.peaks.iter().zip(&truth) {
        assert_close(pk.position, x0, 1e-2);
        assert_close(pk.height, h, 1e-2);
        assert_close(pk.fwhm, w, 1e-2);
        assert!(pk.eta.is_none());
        assert!(pk.fwhm > 0.0);
    }
    assert_close(fit.offset, offset, 2e-2);
    assert!(fit.r2 > 0.999);
    let res = fit.sample_residual(&xs, &ys);
    assert!(res.iter().all(|r| r.abs() < 0.05));
}

#[test]
fn recovers_three_overlapping_gaussians() {
    let xs = linspace(0.0, 10.0, 500);
    let truth = [(4.0, 10.0, 0.5), (4.8, 6.0, 0.7), (6.0, 8.0, 0.4)];
    let offset = 0.5;
    let ys: Vec<f64> = xs
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            offset
                + noise(i)
                + truth
                    .iter()
                    .map(|&(x0, h, w)| gauss(x0, h, w, x))
                    .sum::<f64>()
        })
        .collect();
    let seeds = seed_peaks(&xs, &ys, &[3.95, 4.85, 6.05]);
    let fit = fit_lineshapes(&xs, &ys, LineShape::Gaussian, &seeds).expect("fit");
    for (pk, &(x0, h, w)) in fit.peaks.iter().zip(&truth) {
        assert_close(pk.position, x0, 1e-2);
        assert_close(pk.height, h, 1e-2);
        assert_close(pk.fwhm, w, 1e-2);
    }
    assert!(fit.r2 > 0.999);
}

#[test]
fn recovers_pseudo_voigt_eta() {
    let xs = linspace(-3.0, 3.0, 400);
    let eta = 0.3;
    let ys: Vec<f64> = xs
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            0.2 + noise(i) + eta * lorentz(0.4, 5.0, 0.8, x) + (1.0 - eta) * gauss(0.4, 5.0, 0.8, x)
        })
        .collect();
    let seeds = seed_peaks(&xs, &ys, &[0.35]);
    let fit = fit_lineshapes(&xs, &ys, LineShape::PseudoVoigt, &seeds).expect("fit");
    let pk = &fit.peaks[0];
    assert_close(pk.position, 0.4, 1e-2);
    assert_close(pk.height, 5.0, 1e-2);
    assert_close(pk.fwhm, 0.8, 1e-2);
    let e = pk.eta.expect("eta");
    assert!((e - eta).abs() < 0.1, "eta {e}");
    assert!((0.0..=1.0).contains(&e));
    assert!(fit.r2 > 0.999);
}

#[test]
fn recovers_negative_lorentzian() {
    let xs = linspace(0.0, 4.0, 200);
    let ys: Vec<f64> = xs
        .iter()
        .enumerate()
        .map(|(i, &x)| 1.0 + noise(i) + lorentz(2.0, -5.0, 0.3, x))
        .collect();
    let seeds = seed_peaks(&xs, &ys, &[2.02]);
    assert!(seeds[0].height < 0.0);
    let fit = fit_lineshapes(&xs, &ys, LineShape::Lorentzian, &seeds).expect("fit");
    let pk = &fit.peaks[0];
    assert_close(pk.position, 2.0, 1e-2);
    assert_close(pk.height, -5.0, 1e-2);
    assert_close(pk.fwhm, 0.3, 1e-2);
    assert!(pk.area < 0.0);
    assert_close(fit.offset, 1.0, 2e-2);
}

#[test]
fn analytic_area_matches_numeric_integral() {
    for shape in [
        LineShape::Lorentzian,
        LineShape::Gaussian,
        LineShape::PseudoVoigt,
    ] {
        let (x0, h, w, eta) = (0.0, 3.0, 0.6, 0.5);
        let xs = linspace(-5.0, 5.0, 400);
        let sample = |x: f64| match shape {
            LineShape::Lorentzian => lorentz(x0, h, w, x),
            LineShape::Gaussian => gauss(x0, h, w, x),
            LineShape::PseudoVoigt => eta * lorentz(x0, h, w, x) + (1.0 - eta) * gauss(x0, h, w, x),
        };
        let ys: Vec<f64> = xs.iter().map(|&x| sample(x)).collect();
        let seeds = seed_peaks(&xs, &ys, &[0.0]);
        let fit = fit_lineshapes(&xs, &ys, shape, &seeds).expect("fit");
        let pk = &fit.peaks[0];

        let l = 200.0 * pk.fwhm;
        let n = 80_001;
        let grid = linspace(-l, l, n);
        let step = grid[1] - grid[0];
        let mut integral = 0.0;
        for pair in grid.windows(2) {
            integral +=
                0.5 * step * (fit.eval_component(0, pair[0]) + fit.eval_component(0, pair[1]));
        }
        assert_close(integral, pk.area, 1e-2);
    }
}

#[test]
fn degenerate_inputs_return_none() {
    let seed = PeakSeed {
        position: 0.5,
        height: 1.0,
        fwhm: 0.2,
    };
    assert!(fit_lineshapes(&[], &[], LineShape::Lorentzian, &[seed]).is_none());
    let xs3 = linspace(0.0, 1.0, 3);
    assert!(fit_lineshapes(&xs3, &[0.0, 1.0, 0.0], LineShape::Lorentzian, &[seed]).is_none());
    let xs8 = linspace(0.0, 1.0, 8);
    assert!(fit_lineshapes(&xs8, &[0.0; 7], LineShape::Lorentzian, &[seed]).is_none());
    let mut nan_ys = vec![0.0; 8];
    nan_ys[3] = f64::NAN;
    assert!(fit_lineshapes(&xs8, &nan_ys, LineShape::Lorentzian, &[seed]).is_none());
    assert!(fit_lineshapes(&xs8, &[0.0; 8], LineShape::Lorentzian, &[]).is_none());
    let bad_seed = PeakSeed {
        position: f64::NAN,
        ..seed
    };
    assert!(fit_lineshapes(&xs8, &[0.0; 8], LineShape::Lorentzian, &[bad_seed]).is_none());
    assert!(seed_peaks(&[], &[], &[1.0]).is_empty());
}

#[test]
fn diverged_or_overflowed_fits_rejected() {
    let xs = linspace(0.0, 4.9, 50);
    let ys: Vec<f64> = xs.iter().map(|&x| lorentz(2.0, 5.0, 0.3, x)).collect();
    let far = PeakSeed {
        position: 100.0,
        height: 1.0,
        fwhm: 0.2,
    };
    assert!(fit_lineshapes(&xs, &ys, LineShape::Lorentzian, &[far]).is_none());
    let wide = PeakSeed {
        position: 2.0,
        height: 1.0,
        fwhm: 1e200,
    };
    assert!(fit_lineshapes(&xs, &ys, LineShape::Lorentzian, &[wide]).is_none());
    let huge = PeakSeed {
        position: 2.0,
        height: 1e180,
        fwhm: 1e180,
    };
    assert!(fit_lineshapes(&xs, &ys, LineShape::Lorentzian, &[huge]).is_none());
}

#[test]
fn recovers_on_descending_axis() {
    let xs = linspace(10.0, 0.0, 500);
    let truth = [(4.0, 10.0, 0.5), (6.0, 8.0, 0.4)];
    let ys: Vec<f64> = xs
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            0.5 + noise(i)
                + truth
                    .iter()
                    .map(|&(x0, h, w)| lorentz(x0, h, w, x))
                    .sum::<f64>()
        })
        .collect();
    let seeds = seed_peaks(&xs, &ys, &[6.05, 3.95]);
    assert!(seeds.iter().all(|s| s.fwhm > 0.0));
    let fit = fit_lineshapes(&xs, &ys, LineShape::Lorentzian, &seeds).expect("fit");
    assert_eq!(fit.peaks.len(), 2);
    for (pk, &(x0, h, w)) in fit.peaks.iter().zip(&truth) {
        assert_close(pk.position, x0, 1e-2);
        assert_close(pk.height, h, 1e-2);
        assert_close(pk.fwhm, w, 1e-2);
    }
    assert!(fit.r2 > 0.999);
}

#[test]
fn coincident_seeds_yield_no_degenerate_sigmas() {
    let xs = linspace(0.0, 5.0, 60);
    let ys: Vec<f64> = xs.iter().map(|&x| lorentz(2.5, 5.0, 0.3, x)).collect();
    let seeds = seed_peaks(&xs, &ys, &[2.5, 2.5]);
    let fit = fit_lineshapes(&xs, &ys, LineShape::Lorentzian, &seeds).expect("fit");
    for pk in &fit.peaks {
        assert_ne!(pk.position_sigma, Some(0.0));
        assert_ne!(pk.height_sigma, Some(0.0));
        assert_ne!(pk.fwhm_sigma, Some(0.0));
        assert!(pk.position_sigma.is_none());
    }
}

#[test]
fn pinned_eta_keeps_other_sigmas() {
    let xs = linspace(0.0, 5.0, 50);
    let ys: Vec<f64> = xs.iter().map(|&x| lorentz(2.5, 5.0, 0.3, x)).collect();
    let seeds = seed_peaks(&xs, &ys, &[2.5]);
    let fit = fit_lineshapes(&xs, &ys, LineShape::PseudoVoigt, &seeds).expect("fit");
    let pk = &fit.peaks[0];
    assert_eq!(pk.eta, Some(1.0));
    assert!(pk.eta_sigma.is_none());
    assert!(pk.position_sigma.expect("position sigma").is_finite());
    assert!(pk.height_sigma.expect("height sigma").is_finite());
    assert!(pk.fwhm_sigma.expect("fwhm sigma").is_finite());
    assert!(pk.area_sigma.expect("area sigma").is_finite());
    assert!(fit.offset_sigma.is_some());
    assert!(fit.r2 > 0.999);
}

#[test]
fn single_peak_on_flat_data_stays_finite() {
    let xs = linspace(0.0, 1.0, 32);
    let ys = vec![2.0; 32];
    let seeds = seed_peaks(&xs, &ys, &[0.5]);
    let fit = fit_lineshapes(&xs, &ys, LineShape::Gaussian, &seeds).expect("fit");
    assert!((fit.offset - 2.0).abs() < 1e-9);
    assert!(fit.peaks[0].height.abs() < 1e-9);
    assert!(fit.peaks[0].area.is_finite());
    assert!(fit.eval_total(0.5).is_finite());
    assert_eq!(fit.eval_component(7, 0.5), 0.0);
}

#[test]
fn analytic_jacobian_matches_central_differences() {
    use crate::fit::jac_step;
    for shape in [
        LineShape::Lorentzian,
        LineShape::Gaussian,
        LineShape::PseudoVoigt,
    ] {
        let k = shape.params_per_peak();
        let mut p = vec![1.0, 4.0, 0.5];
        if k == 4 {
            p.push(0.35);
        }
        p.extend([2.5, -3.0, -0.7]);
        if k == 4 {
            p.push(0.6);
        }
        p.push(0.3);
        let m = p.len();
        let mut row = vec![0.0; m];
        for &x in &[0.4, 1.15, 2.3, 2.65, 3.4] {
            fill_jacobian_row(shape, 2, &p, x, &mut row);
            for j in 0..m {
                let h = jac_step(p[j]);
                let mut pj = p.clone();
                pj[j] = p[j] + h;
                let fp = eval_model(shape, 2, &pj, x);
                pj[j] = p[j] - h;
                let fm = eval_model(shape, 2, &pj, x);
                let num = (fp - fm) / (2.0 * h);
                // Floor covers the ~1e-11 cancellation noise of the central
                // difference on partials far smaller than the model value.
                assert!(
                    (row[j] - num).abs() <= 1e-6 * num.abs().max(1e-3),
                    "{shape:?} x={x} j={j}: analytic {}, numeric {num}",
                    row[j]
                );
            }
        }
    }
}

#[test]
fn clamped_eta_yields_zero_jacobian_column() {
    for eta in [-0.2, 0.0, 1.0, 1.3] {
        let p = [1.0, 4.0, 0.5, eta, 0.3];
        let mut row = vec![0.0; 5];
        fill_jacobian_row(LineShape::PseudoVoigt, 1, &p, 1.2, &mut row);
        assert_eq!(row[3], 0.0, "eta {eta}");
        assert!(row[0] != 0.0 && row[1] != 0.0 && row[2] != 0.0);
    }
}

#[test]
fn multi_peak_fit_converges_before_iteration_ceiling() {
    let xs = linspace(0.0, 10.0, 500);
    let truth = [(4.0, 10.0, 0.5), (4.8, 6.0, 0.7), (6.0, 8.0, 0.4)];
    let ys: Vec<f64> = xs
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            0.5 + noise(i)
                + truth
                    .iter()
                    .map(|&(x0, h, w)| lorentz(x0, h, w, x))
                    .sum::<f64>()
        })
        .collect();
    let seeds = seed_peaks(&xs, &ys, &[3.95, 4.85, 6.05]);
    let mut p0 = Vec::new();
    for s in &seeds {
        p0.extend([s.position, s.height, s.fwhm]);
    }
    p0.push(edge_baseline(&ys));
    let mut problem = LineshapeProblem::new(LineShape::Lorentzian, 3, &xs, &ys);
    let (p, iters) = levenberg_marquardt_problem(&mut problem, &p0, 300).expect("fit");
    assert!(iters < 100, "took {iters} iterations");
    assert!(r_squared(&xs, &ys, |x| eval_model(LineShape::Lorentzian, 3, &p, x)) > 0.999);
}
