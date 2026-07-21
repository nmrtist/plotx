---
title: 2D volume integration
description: Measure rectangular peak volumes in true 2D NMR spectra.
---

True 2D contour spectra such as COSY, HSQC, HMBC, NOESY, ROESY, and TOCSY
can be integrated with rectangular footprints. These integrals are separate from
the **Regions** used for pseudo-2D series and do not change the 1D integration
workflow.

## Create and edit an integral

1. Open a true 2D spectrum and press `I` to select **Integrate**.
2. Drag a rectangle around a cross-peak. Very small drags (about three screen
   pixels or less in either direction) are ignored.
3. Drag inside the rectangle to move it. Drag an edge or corner to resize it;
   all eight handles remain inside the spectrum limits.
4. Release the pointer to calculate the volume. Geometry follows the pointer
   while dragging, but the volume is recalculated only when the edit is
   committed.

Press `Esc` to cancel an in-progress edit. Press `Delete` or `Backspace` to
remove the selected integral. The context menu also provides **Set as
reference** and **Delete**. Creating, moving, resizing, renaming, changing the
reference, and deleting can all be undone and redone.

The Integrals table lists the F2 and F1 bounds, raw and normalized volumes, the
display mode used for the calculation, and the baseline setting. You can rename
entries, choose the reference, and set its numeric reference value. Use
**Export Data…** → **Integral table** to save it as CSV/TSV or copy it as TSV.

## What the volume means

PlotX integrates the surface as it is displayed when the integral is
calculated:

- **Real** keeps the signed real surface of a phase-sensitive spectrum. Negative
  lobes therefore produce negative volumes.
- **Magnitude** integrates a non-negative absolute-value surface, as commonly
  displayed for magnitude-mode spectra such as HMBC.

The chosen mode is stored with each integral. Changing the display later does
not reinterpret an existing value; processing changes, including phase changes,
recalculate it in its stored mode.

The raw volume is a two-dimensional trapezoidal integral on the actual F2 and
F1 ppm axes, with units of intensity·ppm². It accounts for partial cells at the
rectangle boundary and works with ascending, descending, or non-uniform axes.
On a uniform grid this is the digital sum multiplied by the F2 and F1 cell
widths, so ratios between integrals from the same spectrum are the same as
ratios of digital sums.

## Baseline correction

Baseline correction is **off by default**. In that mode the rectangle integrates
the displayed surface directly. For quantitative work, each integral can opt in
to one of these local corrections:

- **Constant** subtracts the median of the grid points on the rectangle
  perimeter.
- **Plane** fits a tilted plane to the perimeter and subtracts it throughout the
  rectangle.

The selected baseline method is stored and included in data export. Baseline
correction changes the computed raw volume; normalization never does.

## Reference normalization

The first integral becomes the reference. For each integral, PlotX reports:

`normalized volume = raw volume / reference raw volume × reference value`

The reference value defaults to `1`. Set it to a quantitative weight when
appropriate—for example, `2` when an HSQC reference represents a CH₂ group.
Signed real-mode volumes can yield negative normalized values when a peak has
the opposite sign from the reference.

If the reference volume is effectively zero relative to the other integrals,
normalization is unavailable. The table and plot label show `—` instead of an
unstable ratio. Deleting the reference promotes the first remaining integral.

## Limits and placement advice

- Only rectangles are supported. Ellipses, polygons, contour lassos, automatic
  peak footprints, and deconvolution are not available.
- Overlapping rectangles each integrate their complete area, so their overlap
  is counted in both values.
- Footprints are placed manually; there is no automatic peak detection for
  2D integrals.
- t1-noise ridges, the COSY diagonal, and other artifacts are integrated as-is.
  Place rectangles clear of them when possible.
- Integration does not separate overlapping peaks within one rectangle.

For a stacked series with one-dimensional traces, use
[Pseudo-2D analysis](/guides/pseudo-2d/) and its **Regions** workflow instead.
