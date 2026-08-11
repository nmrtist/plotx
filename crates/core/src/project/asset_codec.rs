use super::*;

pub(super) fn append_undeclared_image_warnings(
    doc: &Document,
    manifest: &Manifest,
    warnings: &mut Vec<String>,
) {
    let declared: std::collections::BTreeSet<AssetId> = manifest
        .assets
        .iter()
        .filter_map(|entry| entry.id.parse().ok())
        .collect();
    let undeclared: std::collections::BTreeSet<AssetId> = doc
        .canvases
        .iter()
        .flat_map(|canvas| canvas.objects.iter())
        .filter_map(|item| match &item.kind {
            CanvasObjectKind::RasterImage(image)
                if !doc.assets.contains_key(&image.asset) && !declared.contains(&image.asset) =>
            {
                Some(image.asset)
            }
            _ => None,
        })
        .collect();
    warnings.extend(undeclared.into_iter().map(|asset| {
        format!("Embedded image {asset} is referenced but is not listed in the project manifest.")
    }));
}

pub(super) fn write_reachable_assets(
    doc: &Document,
    zip: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
    manifest: &mut Manifest,
) -> Result<()> {
    let reachable: std::collections::BTreeSet<_> = doc
        .canvases
        .iter()
        .flat_map(|canvas| canvas.objects.iter())
        .filter_map(|item| match &item.kind {
            CanvasObjectKind::RasterImage(image) => Some(image.asset),
            _ => None,
        })
        .collect();
    let mut path_metadata = std::collections::BTreeMap::new();
    for &asset_id in &reachable {
        let asset = doc
            .assets
            .get(&asset_id)
            .ok_or_else(|| ProjectError::Invalid(format!("missing referenced asset {asset_id}")))?;
        if asset.id != asset_id {
            return Err(ProjectError::Invalid(format!(
                "asset map key {asset_id} does not match record id {}",
                asset.id
            )));
        }
        let digest: [u8; 32] = Sha256::digest(&asset.bytes).into();
        if digest != asset.sha256 {
            return Err(ProjectError::Invalid(format!(
                "asset {asset_id} sha256 does not match its bytes"
            )));
        }
        if asset.pixel_size.contains(&0) || !valid_format(&asset.format) {
            return Err(ProjectError::Invalid(format!(
                "asset {asset_id} has invalid metadata"
            )));
        }
        let hash = format!("{:x}", Sha256::digest(&asset.bytes));
        let path = format!("assets/{hash}.{}", asset.format);
        let metadata = (
            hash,
            asset.format.clone(),
            asset.bytes.len(),
            asset.pixel_size,
        );
        if let Some(previous) = path_metadata.insert(path.clone(), metadata.clone())
            && previous != metadata
        {
            return Err(ProjectError::Invalid(format!(
                "asset path {path:?} has conflicting metadata"
            )));
        }
    }
    let mut written_paths = std::collections::BTreeSet::new();
    for asset_id in reachable {
        let asset = doc
            .assets
            .get(&asset_id)
            .ok_or_else(|| ProjectError::Invalid(format!("missing referenced asset {asset_id}")))?;
        let hash = format!("{:x}", Sha256::digest(&asset.bytes));
        let path = format!("assets/{hash}.{}", asset.format);
        if written_paths.insert(path.clone()) {
            write_bytes(zip, options, &path, &asset.bytes)?;
        }
        manifest.assets.push(AssetEntry {
            id: asset_id.to_string(),
            sha256: hash,
            path,
            format: asset.format.clone(),
            byte_len: asset.bytes.len() as u64,
            pixel_size: asset.pixel_size,
        });
    }
    Ok(())
}

pub(super) fn load_assets(
    zip: &mut ZipArchive<File>,
    manifest: &Manifest,
    app: &mut PlotxApp,
) -> Result<Vec<String>> {
    let mut ids = std::collections::BTreeSet::new();
    let mut warnings = Vec::new();
    let mut path_metadata = std::collections::BTreeMap::new();
    let mut path_bytes: std::collections::BTreeMap<String, Vec<u8>> =
        std::collections::BTreeMap::new();
    for entry in &manifest.assets {
        let id = entry
            .id
            .parse::<AssetId>()
            .map_err(|_| ProjectError::Invalid(format!("invalid asset id {}", entry.id)))?;
        if !ids.insert(id) {
            return Err(ProjectError::Invalid("duplicate asset id".to_owned()));
        }
        if entry.byte_len > usize::MAX as u64
            || entry.pixel_size.contains(&0)
            || !valid_format(&entry.format)
        {
            warnings.push(format!("Embedded image {id} has invalid metadata."));
            continue;
        }
        let metadata = (
            entry.sha256.clone(),
            entry.format.clone(),
            entry.byte_len,
            entry.pixel_size,
        );
        if let Some(previous) = path_metadata.insert(entry.path.clone(), metadata.clone())
            && previous != metadata
        {
            return Err(ProjectError::Invalid(format!(
                "asset path {:?} has conflicting metadata",
                entry.path
            )));
        }
        let bytes = if let Some(bytes) = path_bytes.get(&entry.path) {
            bytes.clone()
        } else {
            let bytes = match read_bytes(zip, &entry.path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    warnings.push(format!("Embedded image {id} could not be read: {error}"));
                    continue;
                }
            };
            path_bytes.insert(entry.path.clone(), bytes.clone());
            bytes
        };
        if bytes.len() as u64 != entry.byte_len {
            warnings.push(format!("Embedded image {id} has a byte-length mismatch."));
            continue;
        }
        let digest = Sha256::digest(&bytes);
        let hash = format!("{digest:x}");
        if hash != entry.sha256 || entry.path != format!("assets/{hash}.{}", entry.format) {
            warnings.push(format!("Embedded image {id} failed its integrity check."));
            continue;
        }
        if !format_matches_header(&entry.format, &bytes) {
            warnings.push(format!(
                "Embedded image {id} does not match its declared format."
            ));
            continue;
        }
        let probe = match plotx_io::image::probe(&bytes) {
            Ok(probe) => probe,
            Err(error) => {
                warnings.push(format!("Embedded image {id} is damaged: {error}"));
                continue;
            }
        };
        if [probe.width, probe.height] != entry.pixel_size {
            warnings.push(format!(
                "Embedded image {id} dimensions do not match its manifest."
            ));
            continue;
        }
        app.doc.assets.insert(
            id,
            AssetRecord {
                id,
                sha256: digest.into(),
                format: entry.format.clone(),
                pixel_size: entry.pixel_size,
                bytes,
            },
        );
    }
    Ok(warnings)
}

fn valid_format(format: &str) -> bool {
    matches!(format, "png" | "jpeg" | "tiff" | "webp" | "bmp")
}

fn format_matches_header(format: &str, bytes: &[u8]) -> bool {
    matches!(
        (format, plotx_io::image::sniff(bytes)),
        ("png", plotx_io::image::RasterFormat::Png)
            | ("jpeg", plotx_io::image::RasterFormat::Jpeg)
            | ("tiff", plotx_io::image::RasterFormat::Tiff)
            | ("webp", plotx_io::image::RasterFormat::WebP)
            | ("bmp", plotx_io::image::RasterFormat::Bmp)
    )
}
