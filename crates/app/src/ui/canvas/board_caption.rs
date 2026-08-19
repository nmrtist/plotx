use super::*;

#[derive(Clone)]
pub(crate) struct FrameCaptionLine {
    pub text: String,
    pub panel_note: Option<ObjectId>,
}

/// One canonical board representation: derived scientific summary first,
/// followed by explicitly authored page and panel notes.
pub(crate) fn frame_caption_lines(app: &PlotxApp, ci: usize) -> Vec<FrameCaptionLine> {
    let Some(canvas) = app.doc.canvases.get(ci) else {
        return Vec::new();
    };
    let mut lines = app
        .canvas_scientific_summary(ci)
        .formatted_lines()
        .into_iter()
        .map(|text| FrameCaptionLine {
            text,
            panel_note: None,
        })
        .collect::<Vec<_>>();
    if !canvas.caption.trim().is_empty() {
        lines.push(FrameCaptionLine {
            text: format!("Note: {}", canvas.caption.trim()),
            panel_note: None,
        });
    }
    lines.extend(
        canvas
            .panel_note_entries()
            .into_iter()
            .map(|(id, letter, note)| FrameCaptionLine {
                text: if letter.is_empty() {
                    format!("Note: {note}")
                } else {
                    format!("{letter} note — {note}")
                },
                panel_note: Some(id),
            }),
    );
    lines
}
