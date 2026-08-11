---
title: Exporting
description: Export publication-quality graphics and the numbers behind them.
---

## Export a figure

Export via the toolbar's export menu — the scope is the current page, all
pages, or a page range.

| Format | Use |
| --- | --- |
| SVG | Vector, editable in illustration software |
| PDF | Vector, for manuscripts |
| PNG / TIFF / JPEG | Raster, DPI adjustable from 72 to 1200 (default 300) |

Presets cover common journal figure sizes — for example
*Single column · 89 mm · 600 dpi · TIFF* — and an export precheck flags
font-size and line-width violations against the chosen preset. For pages with
images, it also reports the lowest effective PPI at the exported size, missing
or damaged resources, and sources with ICC profiles or samples above 8 bits.
Image resolution passes at 300 PPI, warns from 150 to 299 PPI, and fails below
150 PPI. Raster output is 8-bit RGB/RGBA and does not embed the source ICC
profile, so colour-critical work should be verified in its publication
workflow.

Every figure format includes embedded images and applies their crop, rotation,
opacity, fit, interpolation, Panel clipping, and z-order. SVG embeds image
pixels rather than linking to a local path; PDF and bitmap output matches the
same page. If a source is missing or damaged, export stops by default. Enable
**Export with missing-image placeholders** only when a labelled review copy is
preferable to no output.

### Trim page whitespace

Enable **Trim page to visible content** in the Export dialog to remove page
whitespace around the final rendered content. PNG, JPEG, TIFF, SVG, and PDF
support this option, and PlotX remembers the choice for later exports.

Trimming happens after the target-width preset establishes the page's physical
scale. It changes only the page boundary and does not enlarge or fit the content
again. A journal or column preset can therefore produce a final physical page
width smaller than the preset width. Every supported format retains a 1-point
physical safety edge; bitmap exports round that edge up to a whole output pixel.
Empty pages keep their original dimensions.

## Copy figure

*Copy figure* (`Ctrl` + `C`, also in the export menu, the command palette,
and a frame's right-click menu) copies the selected frame — or the active
canvas — straight to the clipboard, no export needed. Images are included. On
Windows the figure is published as PNG, DIB, SVG, and EMF at the same time, and
the receiving app chooses a format it supports.

## Export numerical data

With a dataset selected, choose **Export Data…** from the File menu, the Data
Ribbon tab, or the command palette. The dialog shows only content that exists
for that dataset and offers **Save CSV…**, **Save TSV…**, **Save XLSX…**, and
**Copy TSV**.

Processed NMR data can export Real, Imaginary, or Magnitude intensity. For true
2D and pseudo-2D data, **Matrix** puts F2/ppm across the first row and F1/ppm or
the series axis down the first column. **Long** writes one observation per
row: `f1_ppm,f2_ppm,intensity` for true 2D, or the named series axis with its
unit, `ppm`, and `intensity` for pseudo-2D. Large exports are generated in the
background.

For XPS, **Processed data** includes native, binding-energy, processed, and fit
axes; raw and processed CPS; the selected background model, fit window and
anchors; background-subtracted intensity; envelope, residual, and every fit
component. **Curve-fit parameters** adds standard errors, approximate 95%
intervals, maximum correlation, RMSE, and optional Bootstrap quantiles. CasaXPS
rows remain labelled `Imported (CasaXPS)` rather than being presented as PlotX
fits.

A CSV or TSV exported from a data table comes with a companion
`.plotx-schema.json` file, and an XLSX export keeps the same information on a
hidden worksheet. The visible columns open normally in Excel, Origin, or Prism,
while the companion lets PlotX later reopen the table with its column types,
units, and error bars intact. Exported XLSX files hold plain values, with no
formulas to recalculate.
