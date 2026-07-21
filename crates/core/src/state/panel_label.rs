use super::*;

/// How a page numbers its plot panels; changing it re-letters every panel live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PanelLabelStyle {
    #[default]
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    Arabic,
}

impl PanelLabelStyle {
    pub const ALL: [PanelLabelStyle; 4] = [
        PanelLabelStyle::LowerAlpha,
        PanelLabelStyle::UpperAlpha,
        PanelLabelStyle::LowerRoman,
        PanelLabelStyle::Arabic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PanelLabelStyle::LowerAlpha => "a, b, c",
            PanelLabelStyle::UpperAlpha => "A, B, C",
            PanelLabelStyle::LowerRoman => "i, ii, iii",
            PanelLabelStyle::Arabic => "1, 2, 3",
        }
    }

    pub fn as_key(self) -> &'static str {
        match self {
            PanelLabelStyle::LowerAlpha => "lower_alpha",
            PanelLabelStyle::UpperAlpha => "upper_alpha",
            PanelLabelStyle::LowerRoman => "lower_roman",
            PanelLabelStyle::Arabic => "arabic",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "upper_alpha" => PanelLabelStyle::UpperAlpha,
            "lower_roman" => PanelLabelStyle::LowerRoman,
            "arabic" => PanelLabelStyle::Arabic,
            _ => PanelLabelStyle::LowerAlpha,
        }
    }

    pub fn format(self, index: usize) -> String {
        match self {
            PanelLabelStyle::LowerAlpha => alpha_label(index, false),
            PanelLabelStyle::UpperAlpha => alpha_label(index, true),
            PanelLabelStyle::LowerRoman => roman_label(index + 1),
            PanelLabelStyle::Arabic => (index + 1).to_string(),
        }
    }
}

/// Bijective base-26: 0→a, 25→z, 26→aa, … (upper-cased when `upper`).
fn alpha_label(index: usize, upper: bool) -> String {
    let base = if upper { b'A' } else { b'a' };
    let mut n = index;
    let mut out = Vec::new();
    loop {
        out.push(base + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn roman_label(mut n: usize) -> String {
    const TABLE: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, glyph) in TABLE {
        while n >= v {
            out.push_str(glyph);
            n -= v;
        }
    }
    out
}

impl CanvasDocument {
    /// Plot object ids in publication reading order: row-major by frame top
    /// (bucketed so a near-aligned row reads left-to-right), then by left edge,
    /// with the object id as a stable final tie-break. Drives panel lettering.
    pub fn plot_reading_order(&self) -> Vec<ObjectId> {
        const ROW_BUCKET_PT: f32 = 8.0;
        let mut plots: Vec<&CanvasObject> = self
            .objects
            .iter()
            .filter(|object| object.plot().is_some())
            .collect();
        plots.sort_by(|a, b| {
            let ra = (a.frame.y / ROW_BUCKET_PT).round() as i32;
            let rb = (b.frame.y / ROW_BUCKET_PT).round() as i32;
            ra.cmp(&rb)
                .then(
                    a.frame
                        .x
                        .partial_cmp(&b.frame.x)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(a.id.cmp(&b.id))
        });
        plots.iter().map(|object| object.id).collect()
    }

    /// `None` if `object_id` is not a plot on this page.
    pub fn panel_letter(&self, object_id: ObjectId) -> Option<String> {
        self.plot_reading_order()
            .iter()
            .position(|&id| id == object_id)
            .map(|i| self.panel_label_style.format(i))
    }

    /// Skips empty notes.
    pub fn panel_note_entries(&self) -> Vec<(ObjectId, String, String)> {
        self.plot_reading_order()
            .into_iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let note = self.object(id)?.plot()?.panel.user_note.trim();
                (!note.is_empty()).then(|| (id, self.panel_label_style.format(i), note.to_owned()))
            })
            .collect()
    }

    pub fn panel_notes(&self) -> Vec<(String, String)> {
        self.panel_note_entries()
            .into_iter()
            .map(|(_, letter, note)| (letter, note))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(style: PanelLabelStyle, n: usize) -> Vec<String> {
        (0..n).map(|i| style.format(i)).collect()
    }

    #[test]
    fn styles_format_reading_order_indices() {
        assert_eq!(seq(PanelLabelStyle::LowerAlpha, 3), ["a", "b", "c"]);
        assert_eq!(seq(PanelLabelStyle::UpperAlpha, 3), ["A", "B", "C"]);
        assert_eq!(
            seq(PanelLabelStyle::LowerRoman, 4),
            ["i", "ii", "iii", "iv"]
        );
        assert_eq!(seq(PanelLabelStyle::Arabic, 3), ["1", "2", "3"]);
    }

    #[test]
    fn lower_alpha_rolls_over_past_z() {
        assert_eq!(PanelLabelStyle::LowerAlpha.format(25), "z");
        assert_eq!(PanelLabelStyle::LowerAlpha.format(26), "aa");
        assert_eq!(PanelLabelStyle::LowerAlpha.format(27), "ab");
    }

    #[test]
    fn key_round_trips() {
        for style in PanelLabelStyle::ALL {
            assert_eq!(PanelLabelStyle::from_key(style.as_key()), style);
        }
        assert_eq!(
            PanelLabelStyle::from_key("nonsense"),
            PanelLabelStyle::LowerAlpha
        );
    }
}
