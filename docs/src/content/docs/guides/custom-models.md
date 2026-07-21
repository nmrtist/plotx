---
title: Custom fit models
description: Write, edit, and share your own fit models with the .plotxfit model language.
---

When the [built-in models](/guides/curve-fitting/#built-in-models) don't match
your experiment, write your own. A custom model behaves exactly like a
built-in one: it appears in the model list, fits with the same solvers and
weighting, and its results export the same way.

## Create and manage models

Choose **Clone** beside any model to make an editable copy, then edit it as a
`.plotxfit` definition and **Save to model library**. Custom models appear in
the model list next to the built-ins; use **Edit custom model** to save a new
revision or **Delete** to remove one. Built-in models are read-only until you
clone them.

Custom models are saved in your PlotX configuration directory and can be shared
as ordinary files — a model file is plain data and never runs code.

## Model language

The editor validates the definition as you type and reports any unclassified
symbol; only a valid model can be saved. A model comes in one of three forms.

Explicit models assign an expression to each response:

```text
let rate = 1 / T
y1 = a1 * exp(-rate*x)
y2 = a2 * exp(-rate*x)
```

Implicit models give one algebraic equality per response:

```text
y^2 = a*x + b
```

ODE systems use derivative equations, with initial conditions declared
separately:

```text
d(A)/d(t) = -k1*A
d(B)/d(t) = k1*A - k2*B
```

Identifiers are case-sensitive ASCII names; display names and units may use
Unicode. Expressions support `+ - * / ^ **`, comparisons, `&&`, `||`, `!`,
`if(condition, yes, no)`, the trigonometric, inverse-trigonometric, and
hyperbolic functions, `exp`, `ln`, `log10`, `sqrt`, `abs`, `erf`, `gamma`,
`min`, `max`, and the constants `pi` and `e`. Bare `log` is rejected because
its base is ambiguous across fields — write `ln` or `log10`.

## Parameters and constants

Free parameters may take lower and upper bounds. Fixed parameters stay out of
the optimization. Constrained parameters are expressions of other parameters;
circular constraints are rejected. An initial value may be a number or a rule
computed from the data: `min`, `max`, `mean`, `median`, `quantile`, `span`,
start/middle/end slope, and interpolated `x_at_y`.

A model may also declare per-dataset constants; a constant with no default is a
required input. ODE systems can integrate with an automatic, adaptive non-stiff,
or BDF stiff method, and accept observation times on either side of the initial
time.
