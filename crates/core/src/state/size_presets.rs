//! The canvas-size preset catalog.
//!
//! Journal presets model what publisher artwork guidelines actually constrain:
//! the width is fixed per column count, while the height is content-driven up
//! to the publisher's maximum figure depth. Paper and presentation presets are
//! fixed rectangles. Keeping the catalog as one data table gives the size
//! popover, the command palette, and the export presets a single source of
//! truth for physical dimensions.

/// How a preset constrains the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizePresetGroup {
    /// Width fixed by the journal column, height free up to `max_height_mm`.
    Journal,
    /// Fixed rectangle; either orientation matches (portrait stored).
    Paper,
    /// Fixed rectangle for slides.
    Presentation,
}

impl SizePresetGroup {
    pub fn title(self) -> &'static str {
        match self {
            Self::Journal => "Journal figures",
            Self::Paper => "Paper",
            Self::Presentation => "Presentation",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizePreset {
    /// Stable identifier used by commands and per-canvas preset memory.
    pub id: &'static str,
    pub group: SizePresetGroup,
    pub label: &'static str,
    pub width_mm: f32,
    /// Starting height when the preset is applied. Journal heights are only a
    /// sensible default — the real constraint is `max_height_mm`.
    pub default_height_mm: f32,
    /// Publisher's maximum figure depth. `None` for fixed rectangles or when
    /// the publisher states no explicit cap.
    pub max_height_mm: Option<f32>,
}

const MATCH_TOLERANCE_MM: f32 = 0.01;

impl SizePreset {
    pub const fn size_mm(&self) -> [f32; 2] {
        [self.width_mm, self.default_height_mm]
    }

    /// Fixed-rectangle presets compare both dimensions; journal presets are
    /// defined by their width alone (height is content-driven).
    pub fn is_fixed(&self) -> bool {
        !matches!(self.group, SizePresetGroup::Journal)
    }

    pub fn matches(&self, size_mm: [f32; 2]) -> bool {
        let eq = |a: f32, b: f32| (a - b).abs() < MATCH_TOLERANCE_MM;
        if self.is_fixed() {
            let [w, h] = self.size_mm();
            (eq(size_mm[0], w) && eq(size_mm[1], h)) || (eq(size_mm[0], h) && eq(size_mm[1], w))
        } else {
            eq(size_mm[0], self.width_mm)
        }
    }
}

// Journal widths and maximum depths, from the publishers' artwork guidelines:
// - Nature: 89 mm single, 120–136 mm column-and-a-half, 183 mm double; page
//   depth 247 mm (nature.com "Guide to preparing final artwork").
// - Science family: 55 / 120 / 183 mm for 1–3 columns; max depth 9 in ≈ 228 mm
//   (science.org author figure preparation guide).
// - Cell Press: 85 / 114 / 174 mm; recommended max 8 in ≈ 203 mm
//   (cell.com figure guidelines).
// - ACS: up to 240 pt (84.7 mm) single, up to 504 pt (177.8 mm) double; max
//   depth 660 pt ≈ 233 mm (pubs.acs.org graphics preparation).
// - Elsevier: 90 / 140 / 190 mm; page allows up to 240 mm depth
//   (elsevier.com artwork sizing policy).
// - PNAS: 8.7 cm single, up to 18 cm double; max depth 22.5 cm
//   (pnas.org digital art guidelines).
// - IEEE: 3.5 in (88.9 mm) single, 7.16 in (182 mm) double; no stated depth cap
//   (IEEE author center, resolution and size).
pub const NATURE_SINGLE_COLUMN: SizePreset = SizePreset {
    id: "nature-1col",
    group: SizePresetGroup::Journal,
    label: "Nature · Single column",
    width_mm: 89.0,
    default_height_mm: 60.0,
    max_height_mm: Some(247.0),
};
pub const NATURE_ONE_HALF_COLUMN: SizePreset = SizePreset {
    id: "nature-1p5col",
    group: SizePresetGroup::Journal,
    label: "Nature · 1.5 column",
    width_mm: 136.0,
    default_height_mm: 90.0,
    max_height_mm: Some(247.0),
};
pub const NATURE_DOUBLE_COLUMN: SizePreset = SizePreset {
    id: "nature-2col",
    group: SizePresetGroup::Journal,
    label: "Nature · Double column",
    width_mm: 183.0,
    default_height_mm: 120.0,
    max_height_mm: Some(247.0),
};
pub const SCIENCE_SINGLE_COLUMN: SizePreset = SizePreset {
    id: "science-1col",
    group: SizePresetGroup::Journal,
    label: "Science · Single column",
    width_mm: 55.0,
    default_height_mm: 40.0,
    max_height_mm: Some(228.0),
};
pub const SCIENCE_DOUBLE_COLUMN: SizePreset = SizePreset {
    id: "science-2col",
    group: SizePresetGroup::Journal,
    label: "Science · Double column",
    width_mm: 120.0,
    default_height_mm: 80.0,
    max_height_mm: Some(228.0),
};
pub const SCIENCE_TRIPLE_COLUMN: SizePreset = SizePreset {
    id: "science-3col",
    group: SizePresetGroup::Journal,
    label: "Science · Full width",
    width_mm: 183.0,
    default_height_mm: 120.0,
    max_height_mm: Some(228.0),
};
pub const CELL_SINGLE_COLUMN: SizePreset = SizePreset {
    id: "cell-1col",
    group: SizePresetGroup::Journal,
    label: "Cell Press · Single column",
    width_mm: 85.0,
    default_height_mm: 60.0,
    max_height_mm: Some(203.0),
};
pub const CELL_ONE_HALF_COLUMN: SizePreset = SizePreset {
    id: "cell-1p5col",
    group: SizePresetGroup::Journal,
    label: "Cell Press · 1.5 column",
    width_mm: 114.0,
    default_height_mm: 75.0,
    max_height_mm: Some(203.0),
};
pub const CELL_FULL_WIDTH: SizePreset = SizePreset {
    id: "cell-full",
    group: SizePresetGroup::Journal,
    label: "Cell Press · Full width",
    width_mm: 174.0,
    default_height_mm: 115.0,
    max_height_mm: Some(203.0),
};
pub const ACS_SINGLE_COLUMN: SizePreset = SizePreset {
    id: "acs-1col",
    group: SizePresetGroup::Journal,
    label: "ACS · Single column",
    width_mm: 84.7,
    default_height_mm: 60.0,
    max_height_mm: Some(233.0),
};
pub const ACS_DOUBLE_COLUMN: SizePreset = SizePreset {
    id: "acs-2col",
    group: SizePresetGroup::Journal,
    label: "ACS · Double column",
    width_mm: 177.8,
    default_height_mm: 120.0,
    max_height_mm: Some(233.0),
};
pub const ELSEVIER_SINGLE_COLUMN: SizePreset = SizePreset {
    id: "elsevier-1col",
    group: SizePresetGroup::Journal,
    label: "Elsevier · Single column",
    width_mm: 90.0,
    default_height_mm: 60.0,
    max_height_mm: Some(240.0),
};
pub const ELSEVIER_ONE_HALF_COLUMN: SizePreset = SizePreset {
    id: "elsevier-1p5col",
    group: SizePresetGroup::Journal,
    label: "Elsevier · 1.5 column",
    width_mm: 140.0,
    default_height_mm: 90.0,
    max_height_mm: Some(240.0),
};
pub const ELSEVIER_DOUBLE_COLUMN: SizePreset = SizePreset {
    id: "elsevier-2col",
    group: SizePresetGroup::Journal,
    label: "Elsevier · Double column",
    width_mm: 190.0,
    default_height_mm: 125.0,
    max_height_mm: Some(240.0),
};
pub const PNAS_SINGLE_COLUMN: SizePreset = SizePreset {
    id: "pnas-1col",
    group: SizePresetGroup::Journal,
    label: "PNAS · Single column",
    width_mm: 87.0,
    default_height_mm: 60.0,
    max_height_mm: Some(225.0),
};
pub const PNAS_DOUBLE_COLUMN: SizePreset = SizePreset {
    id: "pnas-2col",
    group: SizePresetGroup::Journal,
    label: "PNAS · Double column",
    width_mm: 178.0,
    default_height_mm: 120.0,
    max_height_mm: Some(225.0),
};
pub const IEEE_SINGLE_COLUMN: SizePreset = SizePreset {
    id: "ieee-1col",
    group: SizePresetGroup::Journal,
    label: "IEEE · Single column",
    width_mm: 88.9,
    default_height_mm: 60.0,
    max_height_mm: None,
};
pub const IEEE_DOUBLE_COLUMN: SizePreset = SizePreset {
    id: "ieee-2col",
    group: SizePresetGroup::Journal,
    label: "IEEE · Double column",
    width_mm: 182.0,
    default_height_mm: 120.0,
    max_height_mm: None,
};
pub const PAPER_A5: SizePreset = SizePreset {
    id: "paper-a5",
    group: SizePresetGroup::Paper,
    label: "A5",
    width_mm: 148.0,
    default_height_mm: 210.0,
    max_height_mm: None,
};
pub const PAPER_A4: SizePreset = SizePreset {
    id: "paper-a4",
    group: SizePresetGroup::Paper,
    label: "A4",
    width_mm: 210.0,
    default_height_mm: 297.0,
    max_height_mm: None,
};
pub const PAPER_A3: SizePreset = SizePreset {
    id: "paper-a3",
    group: SizePresetGroup::Paper,
    label: "A3",
    width_mm: 297.0,
    default_height_mm: 420.0,
    max_height_mm: None,
};
pub const PAPER_LETTER: SizePreset = SizePreset {
    id: "paper-letter",
    group: SizePresetGroup::Paper,
    label: "US Letter",
    width_mm: 215.9,
    default_height_mm: 279.4,
    max_height_mm: None,
};
pub const PAPER_LEGAL: SizePreset = SizePreset {
    id: "paper-legal",
    group: SizePresetGroup::Paper,
    label: "US Legal",
    width_mm: 215.9,
    default_height_mm: 355.6,
    max_height_mm: None,
};
pub const PRESENTATION_16X9: SizePreset = SizePreset {
    id: "pres-16x9",
    group: SizePresetGroup::Presentation,
    label: "Presentation 16:9",
    width_mm: 254.0,
    default_height_mm: 142.875,
    max_height_mm: None,
};
pub const PRESENTATION_4X3: SizePreset = SizePreset {
    id: "pres-4x3",
    group: SizePresetGroup::Presentation,
    label: "Presentation 4:3",
    width_mm: 254.0,
    default_height_mm: 190.5,
    max_height_mm: None,
};

/// The full catalog in display order. Order also breaks matching ties for
/// legacy documents without a stored preset id (183 mm is both Nature double
/// and Science full width; Nature wins by listing first).
pub fn size_presets() -> &'static [SizePreset] {
    &[
        NATURE_SINGLE_COLUMN,
        NATURE_ONE_HALF_COLUMN,
        NATURE_DOUBLE_COLUMN,
        SCIENCE_SINGLE_COLUMN,
        SCIENCE_DOUBLE_COLUMN,
        SCIENCE_TRIPLE_COLUMN,
        CELL_SINGLE_COLUMN,
        CELL_ONE_HALF_COLUMN,
        CELL_FULL_WIDTH,
        ACS_SINGLE_COLUMN,
        ACS_DOUBLE_COLUMN,
        ELSEVIER_SINGLE_COLUMN,
        ELSEVIER_ONE_HALF_COLUMN,
        ELSEVIER_DOUBLE_COLUMN,
        PNAS_SINGLE_COLUMN,
        PNAS_DOUBLE_COLUMN,
        IEEE_SINGLE_COLUMN,
        IEEE_DOUBLE_COLUMN,
        PAPER_A5,
        PAPER_A4,
        PAPER_A3,
        PAPER_LETTER,
        PAPER_LEGAL,
        PRESENTATION_16X9,
        PRESENTATION_4X3,
    ]
}

pub fn preset_by_id(id: &str) -> Option<&'static SizePreset> {
    size_presets().iter().find(|preset| preset.id == id)
}

/// The preset a canvas size corresponds to, preferring the canvas's remembered
/// preset id (which disambiguates equal widths) over a catalog scan.
pub fn matching_preset(size_mm: [f32; 2], preset_id: Option<&str>) -> Option<&'static SizePreset> {
    if let Some(preset) = preset_id.and_then(preset_by_id)
        && preset.matches(size_mm)
    {
        return Some(preset);
    }
    size_presets().iter().find(|preset| preset.matches(size_mm))
}

/// Display name for a canvas size: the preset label, with the orientation
/// spelled out for rotated fixed rectangles. `None` for custom sizes.
pub fn size_display_name(size_mm: [f32; 2], preset_id: Option<&str>) -> Option<String> {
    let preset = matching_preset(size_mm, preset_id)?;
    if preset.is_fixed() {
        let rotated = (size_mm[0] - preset.default_height_mm).abs() < MATCH_TOLERANCE_MM
            && (size_mm[1] - preset.width_mm).abs() < MATCH_TOLERANCE_MM
            && (preset.width_mm - preset.default_height_mm).abs() >= MATCH_TOLERANCE_MM;
        if matches!(preset.group, SizePresetGroup::Paper) {
            let orientation = if rotated { "Landscape" } else { "Portrait" };
            return Some(format!("{} · {orientation}", preset.label));
        }
        return Some(preset.label.to_string());
    }
    Some(preset.label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique() {
        let mut ids: Vec<_> = size_presets().iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), size_presets().len());
    }

    #[test]
    fn journal_presets_match_on_width_alone() {
        assert_eq!(
            matching_preset([89.0, 123.4], None),
            Some(&NATURE_SINGLE_COLUMN)
        );
        assert_eq!(
            matching_preset([183.0, 60.0], None),
            Some(&NATURE_DOUBLE_COLUMN)
        );
    }

    #[test]
    fn preset_id_disambiguates_shared_widths() {
        assert_eq!(
            matching_preset([183.0, 100.0], Some("science-3col")),
            Some(&SCIENCE_TRIPLE_COLUMN)
        );
        // A stale id that no longer matches falls back to the catalog scan.
        assert_eq!(
            matching_preset([89.0, 60.0], Some("science-3col")),
            Some(&NATURE_SINGLE_COLUMN)
        );
    }

    #[test]
    fn fixed_presets_match_either_orientation() {
        assert_eq!(matching_preset([210.0, 297.0], None), Some(&PAPER_A4));
        assert_eq!(matching_preset([297.0, 210.0], None), Some(&PAPER_A4));
        assert_eq!(
            size_display_name([297.0, 210.0], None).as_deref(),
            Some("A4 · Landscape")
        );
        assert_eq!(
            size_display_name([210.0, 297.0], None).as_deref(),
            Some("A4 · Portrait")
        );
    }

    #[test]
    fn custom_sizes_have_no_display_name() {
        assert_eq!(size_display_name([100.0, 100.0], None), None);
    }

    #[test]
    fn near_miss_widths_stay_distinct() {
        // IEEE single (88.9) must not swallow Nature single (89.0).
        assert_eq!(
            matching_preset([88.9, 60.0], None),
            Some(&IEEE_SINGLE_COLUMN)
        );
        assert_eq!(
            matching_preset([89.0, 60.0], None),
            Some(&NATURE_SINGLE_COLUMN)
        );
    }
}
