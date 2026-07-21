//! GDI device-context helpers for the EMF exporter: cached pens/brushes/fonts
//! and primitive drawing over a metafile DC.

use crate::Rect;
use plotx_figure::Color;
use std::collections::HashMap;
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BS_SOLID, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DeleteObject,
    Ellipse, ExtCreatePen, ExtTextOutW, FW_BOLD, FW_NORMAL, GetTextMetricsW, HDC, HGDIOBJ,
    IntersectClipRect, LOGBRUSH, LineTo, MoveToEx, NULL_BRUSH, NULL_PEN, PS_ENDCAP_ROUND,
    PS_GEOMETRIC, PS_JOIN_ROUND, PS_SOLID, PolyPolyline, Polygon as GdiPolygon, Polyline,
    Rectangle, RestoreDC, RoundRect, SaveDC, SelectObject, SetTextAlign, SetTextColor, TA_BASELINE,
    TEXTMETRICW,
};

/// Logical units per point, so hairline geometry survives integer coordinates.
const SCALE: f32 = 20.0;
const FONT_FACE: &str = "Arial";

pub(super) fn l(v: f32) -> i32 {
    (v * SCALE).round() as i32
}

fn colorref(c: Color) -> u32 {
    (c.r as u32) | ((c.g as u32) << 8) | ((c.b as u32) << 16)
}

/// GDI has no per-primitive alpha; translucent fills are pre-blended opaque.
pub(super) fn blend(fg: Color, alpha: f32, bg: Color) -> Color {
    let mix = |f: u8, b: u8| (f as f32 * alpha + b as f32 * (1.0 - alpha)).round() as u8;
    Color::rgb(mix(fg.r, bg.r), mix(fg.g, bg.g), mix(fg.b, bg.b))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[derive(Clone, Copy)]
pub(super) struct TextStyle {
    pub size: f32,
    pub color: Color,
    pub align: u32,
    pub bold: bool,
    pub middle: bool,
    pub rotated: bool,
}

impl TextStyle {
    pub fn new(size: f32, color: Color, align: u32) -> Self {
        Self {
            size,
            color,
            align,
            bold: false,
            middle: false,
            rotated: false,
        }
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn middle(mut self) -> Self {
        self.middle = true;
        self
    }

    pub fn rotated(mut self) -> Self {
        self.rotated = true;
        self
    }
}

pub(super) struct Dc {
    pub hdc: HDC,
    pub pens: HashMap<(u32, i32), HGDIOBJ>,
    pub brushes: HashMap<u32, HGDIOBJ>,
    pub fonts: HashMap<(i32, bool, i32), HGDIOBJ>,
}

impl Dc {
    fn pen(&mut self, color: Color, width: f32) -> HGDIOBJ {
        let key = (colorref(color), l(width.max(0.5)));
        *self.pens.entry(key).or_insert_with(|| unsafe {
            let brush = LOGBRUSH {
                lbStyle: BS_SOLID,
                lbColor: key.0,
                lbHatch: 0,
            };
            ExtCreatePen(
                (PS_GEOMETRIC | PS_SOLID | PS_ENDCAP_ROUND | PS_JOIN_ROUND) as u32,
                key.1.max(1) as u32,
                &brush,
                0,
                std::ptr::null(),
            ) as HGDIOBJ
        })
    }

    fn brush(&mut self, color: Color) -> HGDIOBJ {
        let key = colorref(color);
        *self
            .brushes
            .entry(key)
            .or_insert_with(|| unsafe { CreateSolidBrush(key) as HGDIOBJ })
    }

    fn font(&mut self, size: f32, bold: bool, escapement_deci_deg: i32) -> HGDIOBJ {
        let key = (l(size), bold, escapement_deci_deg);
        *self.fonts.entry(key).or_insert_with(|| unsafe {
            let mut face = [0u16; 32];
            for (i, u) in FONT_FACE.encode_utf16().take(31).enumerate() {
                face[i] = u;
            }
            CreateFontW(
                -key.0,
                0,
                escapement_deci_deg,
                escapement_deci_deg,
                if bold {
                    FW_BOLD as i32
                } else {
                    FW_NORMAL as i32
                },
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                ANTIALIASED_QUALITY as u32,
                0,
                face.as_ptr(),
            ) as HGDIOBJ
        })
    }

    fn stroke_fill(&mut self, stroke: Option<(Color, f32)>, fill: Option<Color>) {
        unsafe {
            let pen = match stroke {
                Some((c, w)) => self.pen(c, w),
                None => windows_sys::Win32::Graphics::Gdi::GetStockObject(NULL_PEN),
            };
            let brush = match fill {
                Some(c) => self.brush(c),
                None => windows_sys::Win32::Graphics::Gdi::GetStockObject(NULL_BRUSH),
            };
            SelectObject(self.hdc, pen);
            SelectObject(self.hdc, brush);
        }
    }

    pub fn rect(&mut self, r: Rect, fill: Option<Color>, stroke: Option<(Color, f32)>) {
        self.stroke_fill(stroke, fill);
        unsafe {
            Rectangle(self.hdc, l(r.left), l(r.top), l(r.right()), l(r.bottom()));
        }
    }

    pub fn round_rect(
        &mut self,
        r: Rect,
        corner: f32,
        fill: Option<Color>,
        stroke: Option<(Color, f32)>,
    ) {
        self.stroke_fill(stroke, fill);
        unsafe {
            RoundRect(
                self.hdc,
                l(r.left),
                l(r.top),
                l(r.right()),
                l(r.bottom()),
                l(corner * 2.0),
                l(corner * 2.0),
            );
        }
    }

    pub fn ellipse(&mut self, r: Rect, fill: Option<Color>, stroke: Option<(Color, f32)>) {
        self.stroke_fill(stroke, fill);
        unsafe {
            Ellipse(self.hdc, l(r.left), l(r.top), l(r.right()), l(r.bottom()));
        }
    }

    pub fn polygon(
        &mut self,
        pts: &[(f32, f32)],
        fill: Option<Color>,
        stroke: Option<(Color, f32)>,
    ) {
        if pts.len() < 3 {
            return;
        }
        self.stroke_fill(stroke, fill);
        let points: Vec<POINT> = pts
            .iter()
            .map(|&(x, y)| POINT { x: l(x), y: l(y) })
            .collect();
        unsafe {
            GdiPolygon(self.hdc, points.as_ptr(), points.len() as i32);
        }
    }

    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), color: Color, width: f32) {
        let pen = self.pen(color, width);
        unsafe {
            SelectObject(self.hdc, pen);
            MoveToEx(self.hdc, l(a.0), l(a.1), std::ptr::null_mut());
            LineTo(self.hdc, l(b.0), l(b.1));
        }
    }

    pub fn polyline(&mut self, pts: &[(f32, f32)], color: Color, width: f32) {
        if pts.len() < 2 {
            return;
        }
        let pen = self.pen(color, width);
        let points: Vec<POINT> = pts
            .iter()
            .map(|&(x, y)| POINT { x: l(x), y: l(y) })
            .collect();
        unsafe {
            SelectObject(self.hdc, pen);
            Polyline(self.hdc, points.as_ptr(), points.len() as i32);
        }
    }

    pub fn segments(&mut self, segs: &[[(f32, f32); 2]], color: Color, width: f32) {
        if segs.is_empty() {
            return;
        }
        let pen = self.pen(color, width);
        let mut points = Vec::with_capacity(segs.len() * 2);
        let counts = vec![2u32; segs.len()];
        for seg in segs {
            points.push(POINT {
                x: l(seg[0].0),
                y: l(seg[0].1),
            });
            points.push(POINT {
                x: l(seg[1].0),
                y: l(seg[1].1),
            });
        }
        unsafe {
            SelectObject(self.hdc, pen);
            PolyPolyline(
                self.hdc,
                points.as_ptr(),
                counts.as_ptr(),
                counts.len() as u32,
            );
        }
    }

    /// Baseline-anchored text matching SVG semantics: `align` maps text-anchor,
    /// `middle` emulates `dominant-baseline="middle"` via font metrics.
    pub fn text(&mut self, text: &str, pos: (f32, f32), style: TextStyle) {
        let TextStyle {
            size,
            color,
            align,
            bold,
            middle,
            rotated,
        } = style;
        if text.trim().is_empty() {
            return;
        }
        let font = self.font(size, bold, if rotated { 900 } else { 0 });
        unsafe {
            SelectObject(self.hdc, font);
            SetTextColor(self.hdc, colorref(color));
            SetTextAlign(self.hdc, TA_BASELINE | align);
            let mut y = l(pos.1);
            if middle {
                let mut tm: TEXTMETRICW = std::mem::zeroed();
                if GetTextMetricsW(self.hdc, &mut tm) != 0 {
                    y += tm.tmAscent - tm.tmHeight / 2;
                }
            }
            let utf16 = wide(text);
            ExtTextOutW(
                self.hdc,
                l(pos.0),
                y,
                0,
                std::ptr::null(),
                utf16.as_ptr(),
                utf16.len() as u32,
                std::ptr::null(),
            );
        }
    }

    pub fn clipped(&mut self, clip: Rect, draw: impl FnOnce(&mut Self)) {
        unsafe {
            SaveDC(self.hdc);
            IntersectClipRect(
                self.hdc,
                l(clip.left),
                l(clip.top),
                l(clip.right()),
                l(clip.bottom()),
            );
        }
        draw(self);
        unsafe {
            RestoreDC(self.hdc, -1);
        }
    }
}

impl Drop for Dc {
    fn drop(&mut self) {
        use windows_sys::Win32::Graphics::Gdi::{GetStockObject, SYSTEM_FONT};
        unsafe {
            // Deselect our objects first — GDI refuses to delete (and would
            // leak) an object still selected into the DC.
            SelectObject(self.hdc, GetStockObject(NULL_PEN));
            SelectObject(self.hdc, GetStockObject(NULL_BRUSH));
            SelectObject(self.hdc, GetStockObject(SYSTEM_FONT));
            for obj in self
                .pens
                .values()
                .chain(self.brushes.values())
                .chain(self.fonts.values())
            {
                DeleteObject(*obj);
            }
        }
    }
}
