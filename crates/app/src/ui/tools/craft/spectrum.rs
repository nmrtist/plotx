use egui::Ui;
use plotx_core::state::CraftSpectrumChannel;

pub(super) fn channel_control(channel: &mut CraftSpectrumChannel, ui: &mut Ui) {
    egui::ComboBox::from_id_salt("craft_spectrum_channel")
        .selected_text(channel_label(*channel))
        .show_ui(ui, |ui| {
            ui.selectable_value(channel, CraftSpectrumChannel::Magnitude, "Magnitude");
            ui.selectable_value(channel, CraftSpectrumChannel::Real, "Real");
            ui.selectable_value(channel, CraftSpectrumChannel::Imaginary, "Imaginary");
        });
}

fn channel_label(channel: CraftSpectrumChannel) -> &'static str {
    match channel {
        CraftSpectrumChannel::Real => "Real",
        CraftSpectrumChannel::Magnitude => "Magnitude",
        CraftSpectrumChannel::Imaginary => "Imaginary",
    }
}
