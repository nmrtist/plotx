use super::{attribute, invalid, push_warning};
use crate::{IoError, Precursor};
use quick_xml::events::BytesStart;

#[derive(Clone, Copy, Default)]
enum Section {
    #[default]
    Other,
    IsolationWindow,
    SelectedIon(usize),
    Activation,
}

#[derive(Default)]
pub(super) struct Draft {
    precursor_index: Option<usize>,
    precursor_count: usize,
    selected_ion_count: usize,
    section: Section,
    source_spectrum_native_id: Option<String>,
    selected_mz: Option<f64>,
    selected_intensity: Option<f64>,
    charge: Option<i32>,
    isolation_window_target_mz: Option<f64>,
    isolation_window_lower_offset: Option<f64>,
    isolation_window_upper_offset: Option<f64>,
    collision_energy: Option<f64>,
    activation_methods: Vec<String>,
}

impl Draft {
    pub(super) fn start(&mut self, tag: &BytesStart<'_>) -> Result<(), IoError> {
        match tag.local_name().as_ref() {
            b"precursor" => {
                self.precursor_count += 1;
                self.precursor_index = Some(self.precursor_count);
                self.section = Section::Other;
                if self.precursor_count == 1 {
                    self.source_spectrum_native_id = attribute(tag, b"spectrumRef")?;
                }
            }
            b"isolationWindow" if self.precursor_index.is_some() => {
                self.section = Section::IsolationWindow;
            }
            b"selectedIon" if self.precursor_index == Some(1) => {
                self.selected_ion_count += 1;
                self.section = Section::SelectedIon(self.selected_ion_count);
            }
            b"activation" if self.precursor_index.is_some() => {
                self.section = Section::Activation;
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn end(&mut self, name: &[u8]) {
        match name {
            b"precursor" => {
                self.precursor_index = None;
                self.section = Section::Other;
            }
            b"isolationWindow" | b"selectedIon" | b"activation" => {
                self.section = Section::Other;
            }
            _ => {}
        }
    }

    pub(super) fn apply_cv(&mut self, tag: &BytesStart<'_>) -> Result<(), IoError> {
        if self.precursor_index != Some(1) {
            return Ok(());
        }
        let accession = attribute(tag, b"accession")?.unwrap_or_default();
        match (self.section, accession.as_str()) {
            (Section::IsolationWindow, "MS:1000827") => set_f64(
                &mut self.isolation_window_target_mz,
                tag,
                "isolation window target m/z",
            ),
            (Section::IsolationWindow, "MS:1000828") => set_f64(
                &mut self.isolation_window_lower_offset,
                tag,
                "isolation window lower offset",
            ),
            (Section::IsolationWindow, "MS:1000829") => set_f64(
                &mut self.isolation_window_upper_offset,
                tag,
                "isolation window upper offset",
            ),
            (Section::SelectedIon(1), "MS:1000744" | "MS:1000040") => {
                set_f64(&mut self.selected_mz, tag, "selected ion m/z")
            }
            (Section::SelectedIon(1), "MS:1000042") => {
                set_f64(&mut self.selected_intensity, tag, "selected ion intensity")
            }
            (Section::SelectedIon(1), "MS:1000041") => {
                let value = required_value(tag, "charge state")?;
                set_once(
                    &mut self.charge,
                    value
                        .parse::<i32>()
                        .map_err(|_| invalid("invalid precursor charge state"))?,
                    "precursor charge state",
                )
            }
            (Section::Activation, "MS:1000045") => {
                set_f64(&mut self.collision_energy, tag, "collision energy")
            }
            (Section::Activation, accession) if !is_activation_energy(accession) => {
                if let Some(name) = attribute(tag, b"name")?.filter(|name| !name.is_empty())
                    && !self.activation_methods.contains(&name)
                {
                    self.activation_methods.push(name);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn finish(self, spectrum: &str, warnings: &mut Vec<String>) -> Option<Precursor> {
        if self.precursor_count > 1 {
            push_warning(
                warnings,
                format!(
                    "Spectrum {spectrum} has {} precursors; only the first was imported.",
                    self.precursor_count
                ),
            );
        }
        if self.selected_ion_count > 1 {
            push_warning(
                warnings,
                format!(
                    "Spectrum {spectrum} has {} selected ions in its first precursor; only the first was imported.",
                    self.selected_ion_count
                ),
            );
        }
        if self.precursor_count == 0 {
            return None;
        }
        let activation_method =
            (!self.activation_methods.is_empty()).then(|| self.activation_methods.join(" + "));
        let precursor = Precursor {
            source_spectrum_native_id: self.source_spectrum_native_id,
            selected_mz: self.selected_mz,
            selected_intensity: self.selected_intensity,
            charge: self.charge,
            isolation_window_target_mz: self.isolation_window_target_mz,
            isolation_window_lower_offset: self.isolation_window_lower_offset,
            isolation_window_upper_offset: self.isolation_window_upper_offset,
            collision_energy: self.collision_energy,
            activation_method,
        };
        if precursor.source_spectrum_native_id.is_none()
            && precursor.selected_mz.is_none()
            && precursor.selected_intensity.is_none()
            && precursor.charge.is_none()
            && precursor.isolation_window_target_mz.is_none()
            && precursor.isolation_window_lower_offset.is_none()
            && precursor.isolation_window_upper_offset.is_none()
            && precursor.collision_energy.is_none()
            && precursor.activation_method.is_none()
        {
            push_warning(
                warnings,
                format!(
                    "Spectrum {spectrum} has a precursor with no supported metadata; it was skipped."
                ),
            );
            None
        } else {
            Some(precursor)
        }
    }
}

fn required_value(tag: &BytesStart<'_>, field: &str) -> Result<String, IoError> {
    attribute(tag, b"value")?.ok_or_else(|| invalid(format!("{field} has no value")))
}

fn set_f64(target: &mut Option<f64>, tag: &BytesStart<'_>, field: &str) -> Result<(), IoError> {
    let value = required_value(tag, field)?
        .parse::<f64>()
        .map_err(|_| invalid(format!("invalid {field}")))?;
    set_once(target, value, field)
}

fn set_once<T>(target: &mut Option<T>, value: T, field: &str) -> Result<(), IoError> {
    if target.replace(value).is_some() {
        return Err(invalid(format!("precursor repeats {field}")));
    }
    Ok(())
}

fn is_activation_energy(accession: &str) -> bool {
    matches!(
        accession,
        "MS:1000045"
            | "MS:1000138"
            | "MS:1000509"
            | "MS:1002013"
            | "MS:1002014"
            | "MS:1002218"
            | "MS:1002219"
            | "MS:1002680"
            | "MS:1003410"
    )
}
