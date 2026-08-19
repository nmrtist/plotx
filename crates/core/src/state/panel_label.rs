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
        Self::try_from_key(key).unwrap_or_default()
    }

    pub fn try_from_key(key: &str) -> Option<Self> {
        match key {
            "lower_alpha" => Some(PanelLabelStyle::LowerAlpha),
            "upper_alpha" => Some(PanelLabelStyle::UpperAlpha),
            "lower_roman" => Some(PanelLabelStyle::LowerRoman),
            "arabic" => Some(PanelLabelStyle::Arabic),
            _ => None,
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
    /// Panel ids in publication reading order: row-major by frame top
    /// (bucketed so a near-aligned row reads left-to-right), then by left edge,
    /// with the object id as a stable final tie-break. Drives panel lettering.
    pub fn panel_reading_order(&self) -> Vec<PanelId> {
        const ROW_BUCKET_PT: f32 = 8.0;
        let mut panels: Vec<&Panel> = self.panels.iter().collect();
        panels.sort_by(|a, b| {
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
        panels.iter().map(|panel| panel.id).collect()
    }

    pub fn plot_reading_order(&self) -> Vec<ObjectId> {
        self.panel_reading_order()
            .into_iter()
            .filter_map(|id| self.panel(id))
            .flat_map(|panel| panel.item_order.iter().copied())
            .filter(|id| self.object(*id).is_some_and(|item| item.plot().is_some()))
            .collect()
    }

    /// `None` if `object_id` is not a plot on this page.
    pub fn panel_letter(&self, object_id: ObjectId) -> Option<String> {
        let panel = self.parent_panel(object_id).and_then(|id| self.panel(id))?;
        if !panel.label.visible {
            return None;
        }
        match &panel.label.mode {
            PanelLabelMode::Auto { slot } => Some(self.panel_label_style.format(*slot as usize)),
            PanelLabelMode::LockedAuto { value } | PanelLabelMode::Manual { value } => {
                Some(value.clone())
            }
        }
    }

    /// User-authored notes only. Skips empty notes and returns an empty label
    /// when the panel letter is not displayed.
    pub fn panel_note_entries(&self) -> Vec<(ObjectId, String, String)> {
        self.panels
            .iter()
            .filter_map(|panel| {
                let id = *panel.item_order.first()?;
                let note = panel.note.trim();
                let letter = if self.panel_label_is_displayed(panel.id) {
                    match &panel.label.mode {
                        PanelLabelMode::Auto { slot } => {
                            self.panel_label_style.format(*slot as usize)
                        }
                        PanelLabelMode::LockedAuto { value } | PanelLabelMode::Manual { value } => {
                            value.clone()
                        }
                    }
                } else {
                    String::new()
                };
                (!note.is_empty()).then(|| (id, letter, note.to_owned()))
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
