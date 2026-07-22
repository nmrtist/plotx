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
font-size and line-width violations against the chosen preset before you
export, so problems are fixed on the board rather than discovered by the
journal.

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
canvas — straight to the clipboard, no export needed. On Windows the figure
is published as a bitmap (PNG + DIB) and as a vector (SVG + EMF) at the same
time, and the app you paste into picks its best format automatically: chat
apps paste the bitmap, while Word, PowerPoint, and WPS paste an editable
vector.

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

A CSV or TSV exported from a data table comes with a companion
`.plotx-schema.json` file, and an XLSX export keeps the same information on a
hidden worksheet. The visible columns open normally in Excel, Origin, or Prism,
while the companion lets PlotX later reopen the table with its column types,
units, and error bars intact. Exported XLSX files hold plain values, with no
formulas to recalculate.
