---
title: Curve fitting
description: Fit x-y data with built-in or custom models in PlotX.
---

PlotX fits x-y data in a table to a mathematical model, using the same set of
tools for its built-in models and for models you write yourself. A fit and its
full results are saved with the table, so a fitted plot stays reproducible even
after you later edit or delete the model it came from.

## Fit a table

Open the **Analyze** tab and choose **Fit Curves** in the **Curve Fit** group.
A task card opens over the canvas. In it:

1. **Model** — pick a built-in or custom model. PlotX preselects a sensible
   default: Stejskal–Tanner for a DOSY table, inversion recovery for a
   relaxation-delay series, and mono-exponential otherwise.
2. **Data** — fit every curve in the table (**All curves**) or a single
   **Selected curve**.
3. **Parameters** — when fitting all curves, tick **Share all free parameters
   across curves** to fit one shared parameter set (a global fit), or leave it
   off to fit each curve independently.
4. **Fit** — choose a solver, weighting, optional robust loss, and the number
   of starts (see below). **Preview initial curve** overlays the model at its
   starting values so you can check the guess before fitting.
5. Choose **Run Fit**.

Fitted curves and results stay attached to the table. Editing the table's data
clears the affected fits, and that clearing can be undone in one step along with
the edit.

### Solver, weighting, and robustness

- **Solver** — **Bounded trust region** (the default) honors the parameter
  bounds a model declares. **Levenberg–Marquardt** is faster but unbounded; it
  refuses to run on a model that sets bounds, so switch back to the trust-region
  solver for those.
- **Weights** — how much each point counts. **Equal** treats all points alike.
  **Measurement σ** weights by an error column. **Relative** suits noise that
  grows with signal. **Poisson** suits counting data. **Auto** uses
  inverse-variance weighting when every included point has a valid positive
  error, and otherwise falls back to equal weights and notes that choice in the
  results.
- **Robust loss** — **Huber**, **Soft-L1**, or **Cauchy** progressively reduce
  the influence of outliers; leave it at **None** for clean data.
- **Starts** — fitting from several starting points helps avoid a poor local
  minimum on models with many parameters. The search is deterministic, so a
  repeated fit gives the same result.

PlotX never silently drops data. If an included row holds a non-finite value
(NaN or ∞), the fit reports it instead of quietly skipping the point.

## Read the results

The Results section lists, for each fitted curve, its parameter estimates with
standard errors and its R². Below them, per-fit diagnostics report χ² and
reduced χ², degrees of freedom, AICc, BIC, and the iteration count. Any
warnings the solver raised appear alongside them.

## Built-in models

PlotX ships these models, grouped by kind:

- **Relaxation** — Mono-exponential, Inversion recovery, Saturation recovery,
  Bi-exponential, Stretched exponential.
- **Diffusion** — Stejskal–Tanner. On a DOSY table it reads the gradient and
  diffusion parameters (γ, δ, Δ, and the recovery delay and shape factor) from
  the table's metadata, so you don't enter them by hand. It needs those
  parameters and reports if the table lacks them.
- **General** — Linear.

## Custom models

When no built-in model matches your experiment, clone one and edit it as a
`.plotxfit` definition — explicit expressions, implicit equations, or ODE
systems, with bounds, constraints, and data-derived initial values. See
[Custom fit models](/guides/custom-models/).

## Export

Choose **Export Data… → Curve-fit parameters** to write a long-form table of
parameter estimates and standard errors, per-response diagnostics, covariance
entries, and every observed, predicted, and residual point.
