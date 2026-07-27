//! Geometry controls for selected canvas objects.

use super::edits::note_inspector_edit;
use super::*;

pub(super) fn geometry_section(app: &mut PlotxApp, ci: usize, ids: &[ObjectId], ui: &mut Ui) {
    let primary = ids[0];
    let Some(o) = app.doc.canvases[ci].object(primary) else {
        return;
    };
    let enabled = !o.locked;
    let frame = o.frame;
    let mut x = frame.x / MM_TO_PT;
    let mut y = frame.y / MM_TO_PT;
    let mut w = frame.width / MM_TO_PT;
    let mut h = frame.height / MM_TO_PT;

    egui::Grid::new("object_geometry")
        .num_columns(4)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            ui.label("X");
            let rx = ui.add_enabled(enabled, mm_drag(&mut x));
            ui.label("Y");
            let ry = ui.add_enabled(enabled, mm_drag(&mut y));
            ui.end_row();
            ui.label("W");
            let rw = ui.add_enabled(enabled, mm_drag(&mut w));
            ui.label("H");
            let rh = ui.add_enabled(enabled, mm_drag(&mut h));
            ui.end_row();

            if rx.changed() || ry.changed() || rw.changed() || rh.changed() {
                note_inspector_edit(app, ci, ids);
                let new = ObjectFrame::new(x * MM_TO_PT, y * MM_TO_PT, w * MM_TO_PT, h * MM_TO_PT);
                app.set_object_frame(ci, primary, new);
            }
        });

    if !enabled {
        ui.weak("Locked — unlock to edit geometry.");
    } else if ids.len() > 1 {
        ui.weak("Geometry edits the primary selection.");
    }
}

fn mm_drag(value: &mut f32) -> DragValue<'_> {
    DragValue::new(value)
        .speed(0.5)
        .max_decimals(1)
        .suffix(" mm")
}
