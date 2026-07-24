use super::*;

pub fn dimension_from_1d(data: &NmrData) -> Dimension {
    Dimension {
        id: "f2".to_owned(),
        role: "direct".to_owned(),
        size: data.points.len(),
        storage_axis: 0,
        quantity: "time_or_frequency".to_owned(),
        display_quantity: Some("chemical_shift".to_owned()),
        unit: Some("ppm".to_owned()),
        nucleus: Some(data.nucleus.clone()),
        spectral_width_hz: Some(data.spectral_width_hz),
        observe_freq_mhz: Some(data.observe_freq_mhz),
        carrier_ppm: Some(data.carrier_ppm),
        group_delay: Some(data.group_delay),
    }
}

pub fn dimension_from_dim(
    id: &str,
    role: &str,
    storage_axis: usize,
    size: usize,
    dim: &Dim,
) -> Dimension {
    Dimension {
        id: id.to_owned(),
        role: role.to_owned(),
        size,
        storage_axis,
        quantity: "time_or_frequency".to_owned(),
        display_quantity: Some("chemical_shift".to_owned()),
        unit: Some("ppm".to_owned()),
        nucleus: Some(dim.nucleus.clone()),
        spectral_width_hz: Some(dim.spectral_width_hz),
        observe_freq_mhz: Some(dim.observe_freq_mhz),
        carrier_ppm: Some(dim.carrier_ppm),
        group_delay: Some(dim.group_delay),
    }
}

pub fn dim_from_dimension(dim: &Dimension) -> Result<Dim> {
    Ok(Dim {
        spectral_width_hz: required(dim.spectral_width_hz, "spectral_width_hz")?,
        observe_freq_mhz: required(dim.observe_freq_mhz, "observe_freq_mhz")?,
        carrier_ppm: required(dim.carrier_ppm, "carrier_ppm")?,
        nucleus: dim.nucleus.clone().unwrap_or_else(|| "X".to_owned()),
        group_delay: dim.group_delay.unwrap_or(0.0),
    })
}
