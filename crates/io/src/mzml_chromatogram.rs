use super::{
    ArrayKind, BinaryArray, EventBounded, Precision, attribute, decode_array, invalid,
    push_warning, read_binary_text, read_event,
};
use crate::{
    ChromatogramChannel, ChromatogramChannelId, ChromatogramKind, IoError, MassTransition, Polarity,
};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use std::io::BufRead;

const MAX_CHROMATOGRAMS: usize = 100_000;
const MAX_POINTS_PER_CHROMATOGRAM: usize = 5_000_000;
const MAX_XML_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Default)]
enum Section {
    #[default]
    Other,
    Precursor,
    PrecursorActivation,
    Product,
}

struct Draft {
    native_id: String,
    declared_len: Option<usize>,
    title: Option<String>,
    kind: ChromatogramKind,
    polarity: Polarity,
    precursor_mz: Option<f64>,
    product_mz: Option<f64>,
    collision_energy: Option<f64>,
    activation_method: Option<String>,
    time_min: Option<Vec<f64>>,
    time_unit: Option<String>,
    values: Option<Vec<f64>>,
    unit: Option<String>,
}

pub(super) fn parse<R: BufRead>(
    reader: &mut Reader<EventBounded<R>>,
    buffer: &mut Vec<u8>,
    tag: &BytesStart<'_>,
    ordinal: usize,
    warnings: &mut Vec<String>,
    total_decoded: &mut usize,
) -> Result<ChromatogramChannel, IoError> {
    if ordinal >= MAX_CHROMATOGRAMS {
        return Err(invalid(format!(
            "chromatogram count exceeds limit {MAX_CHROMATOGRAMS}"
        )));
    }
    let native_id = attribute(tag, b"id")?.unwrap_or_else(|| format!("chromatogram={ordinal}"));
    let declared_len = attribute(tag, b"defaultArrayLength")?
        .map(|value| {
            value.parse::<usize>().map_err(|_| {
                invalid(format!(
                    "chromatogram {native_id} has invalid defaultArrayLength"
                ))
            })
        })
        .transpose()?;
    if declared_len.is_some_and(|len| len > MAX_POINTS_PER_CHROMATOGRAM) {
        return Err(invalid(format!(
            "chromatogram {native_id} declares more than {MAX_POINTS_PER_CHROMATOGRAM} points"
        )));
    }
    let mut draft = Draft {
        native_id,
        declared_len,
        title: None,
        kind: ChromatogramKind::Unknown,
        polarity: Polarity::Unknown,
        precursor_mz: None,
        product_mz: None,
        collision_energy: None,
        activation_method: None,
        time_min: None,
        time_unit: None,
        values: None,
        unit: None,
    };
    let mut binary = None;
    let mut section = Section::Other;
    loop {
        match read_event(reader, buffer, MAX_XML_EVENT_BYTES)? {
            Event::Start(tag) if tag.local_name().as_ref() == b"precursor" => {
                section = Section::Precursor;
            }
            Event::Start(tag) if tag.local_name().as_ref() == b"product" => {
                section = Section::Product;
            }
            Event::Start(tag) if tag.local_name().as_ref() == b"activation" => {
                section = Section::PrecursorActivation;
            }
            Event::End(tag) if tag.local_name().as_ref() == b"activation" => {
                section = Section::Precursor;
            }
            Event::End(tag) if matches!(tag.local_name().as_ref(), b"precursor" | b"product") => {
                section = Section::Other;
            }
            Event::Start(tag) if tag.local_name().as_ref() == b"binaryDataArray" => {
                binary = Some(BinaryArray::default());
            }
            Event::Empty(tag) | Event::Start(tag) if tag.local_name().as_ref() == b"cvParam" => {
                apply_cv(&tag, &mut draft, binary.as_mut(), section)?;
            }
            Event::Start(tag) if tag.local_name().as_ref() == b"binary" => {
                let array = binary.as_mut().ok_or_else(|| {
                    invalid(format!(
                        "chromatogram {} has binary outside binaryDataArray",
                        draft.native_id
                    ))
                })?;
                read_binary_text(reader, buffer, &mut array.text, &draft.native_id)?;
            }
            Event::End(tag) if tag.local_name().as_ref() == b"binaryDataArray" => {
                let array = binary.take().ok_or_else(|| {
                    invalid(format!(
                        "chromatogram {} closes an unopened binaryDataArray",
                        draft.native_id
                    ))
                })?;
                finish_array(array, &mut draft, warnings, total_decoded)?;
            }
            Event::End(tag) if tag.local_name().as_ref() == b"chromatogram" => break,
            Event::Eof => {
                return Err(invalid(format!(
                    "input truncated inside chromatogram {}",
                    draft.native_id
                )));
            }
            _ => {}
        }
        buffer.clear();
    }
    finish(draft)
}

fn apply_cv(
    tag: &BytesStart<'_>,
    draft: &mut Draft,
    binary: Option<&mut BinaryArray>,
    section: Section,
) -> Result<(), IoError> {
    let accession = attribute(tag, b"accession")?.unwrap_or_default();
    if let Some(array) = binary {
        match accession.as_str() {
            "MS:1000595" => array.kind = Some(ArrayKind::Time),
            "MS:1000515" => array.kind = Some(ArrayKind::Intensity),
            "MS:1000521" => array.precision = Some(Precision::F32),
            "MS:1000523" => array.precision = Some(Precision::F64),
            "MS:1000574" => array.zlib = true,
            "MS:1000576" => {}
            "MS:1002312" | "MS:1002313" | "MS:1002314" => {
                array.unsupported = Some("MS-Numpress encoding".to_owned());
            }
            "MS:1000140" => array.unsupported = Some("big-endian binary data".to_owned()),
            "MS:1000786" => array.optional_auxiliary = true,
            _ => {}
        }
        if matches!(array.kind, Some(ArrayKind::Time)) {
            draft.time_unit = attribute(tag, b"unitAccession")?.or_else(|| draft.time_unit.take());
        } else if matches!(array.kind, Some(ArrayKind::Intensity)) {
            draft.unit = attribute(tag, b"unitName")?.or_else(|| draft.unit.take());
        }
        return Ok(());
    }
    match accession.as_str() {
        "MS:1000235" => {
            draft.kind = ChromatogramKind::TotalIonCurrent;
            draft.title = attribute(tag, b"name")?;
        }
        "MS:1000628" => {
            draft.kind = ChromatogramKind::BasePeak;
            draft.title = attribute(tag, b"name")?;
        }
        "MS:1001472" => draft.kind = ChromatogramKind::SelectedIonMonitoring,
        "MS:1001473" => draft.kind = ChromatogramKind::SelectedReactionMonitoring,
        "MS:1000130" => draft.polarity = Polarity::Positive,
        "MS:1000129" => draft.polarity = Polarity::Negative,
        "MS:1000827" if matches!(section, Section::Precursor | Section::Product) => {
            let value = attribute(tag, b"value")?
                .ok_or_else(|| invalid("transition target m/z has no value"))?
                .parse::<f64>()
                .map_err(|_| invalid("invalid transition target m/z"))?;
            match section {
                Section::Precursor => draft.precursor_mz = Some(value),
                Section::Product => draft.product_mz = Some(value),
                _ => {}
            }
        }
        "MS:1000045" if matches!(section, Section::PrecursorActivation) => {
            draft.collision_energy = attribute(tag, b"value")?
                .map(|value| {
                    value
                        .parse::<f64>()
                        .map_err(|_| invalid("invalid collision energy"))
                })
                .transpose()?;
        }
        _ if matches!(section, Section::PrecursorActivation)
            && draft.activation_method.is_none() =>
        {
            draft.activation_method = attribute(tag, b"name")?;
        }
        _ => {}
    }
    Ok(())
}

fn finish_array(
    array: BinaryArray,
    draft: &mut Draft,
    warnings: &mut Vec<String>,
    total_decoded: &mut usize,
) -> Result<(), IoError> {
    let Some(_) = array.kind else {
        if array.optional_auxiliary {
            return Ok(());
        }
        push_warning(
            warnings,
            format!(
                "Chromatogram {} contains an unsupported auxiliary binary array; it was skipped.",
                draft.native_id
            ),
        );
        return Ok(());
    };
    let (kind, values) = decode_array(array, draft.declared_len, &draft.native_id, total_decoded)?;
    let target = match kind {
        ArrayKind::Time => &mut draft.time_min,
        ArrayKind::Intensity => &mut draft.values,
        ArrayKind::Mz => return Err(invalid("chromatogram contains an unexpected m/z array")),
    };
    if target.replace(values).is_some() {
        return Err(invalid(format!(
            "chromatogram {} repeats a required binary array",
            draft.native_id
        )));
    }
    Ok(())
}

fn finish(mut draft: Draft) -> Result<ChromatogramChannel, IoError> {
    let mut time_min = draft.time_min.take().ok_or_else(|| {
        invalid(format!(
            "chromatogram {} is missing the time array",
            draft.native_id
        ))
    })?;
    let values = draft.values.take().ok_or_else(|| {
        invalid(format!(
            "chromatogram {} is missing the intensity array",
            draft.native_id
        ))
    })?;
    if time_min.len() != values.len() {
        return Err(invalid(format!(
            "chromatogram {} has mismatched time and intensity arrays",
            draft.native_id
        )));
    }
    if let Some(declared) = draft.declared_len
        && declared != time_min.len()
    {
        return Err(invalid(format!(
            "chromatogram {} declares {declared} points but decodes {}",
            draft.native_id,
            time_min.len()
        )));
    }
    match draft.time_unit.as_deref() {
        Some("UO:0000010") => time_min.iter_mut().for_each(|value| *value /= 60.0),
        Some("UO:0000031") | Some("MS:1000038") | None => {}
        Some(unit) => {
            return Err(invalid(format!(
                "unsupported chromatogram time unit {unit}"
            )));
        }
    }
    let description = if matches!(
        draft.kind,
        ChromatogramKind::SelectedIonMonitoring | ChromatogramKind::SelectedReactionMonitoring
    ) {
        draft.native_id.clone()
    } else {
        draft.title.unwrap_or_else(|| draft.native_id.clone())
    };
    let transition = (draft.precursor_mz.is_some()
        || draft.product_mz.is_some()
        || draft.collision_energy.is_some()
        || draft.activation_method.is_some())
    .then_some(MassTransition {
        precursor_mz: draft.precursor_mz,
        product_mz: draft.product_mz,
        collision_energy: draft.collision_energy,
        activation_method: draft.activation_method,
    });
    Ok(ChromatogramChannel {
        id: ChromatogramChannelId(draft.native_id),
        kind: draft.kind,
        polarity: draft.polarity,
        transition,
        source_stream: None,
        coordinate: None,
        description,
        unit: draft.unit.unwrap_or_else(|| "intensity".to_owned()),
        time_min,
        values,
    })
}
