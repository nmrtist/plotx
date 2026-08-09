use super::*;

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
) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
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
            return Err(ProjectError::Invalid(format!(
                "asset {id} has invalid metadata"
            )));
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
            let bytes = read_bytes(zip, &entry.path)?;
            path_bytes.insert(entry.path.clone(), bytes.clone());
            bytes
        };
        if bytes.len() as u64 != entry.byte_len {
            return Err(ProjectError::Invalid(format!(
                "asset {id} byte length mismatch"
            )));
        }
        let digest = Sha256::digest(&bytes);
        let hash = format!("{digest:x}");
        if hash != entry.sha256 || entry.path != format!("assets/{hash}.{}", entry.format) {
            return Err(ProjectError::Invalid(format!(
                "asset {id} hash or path mismatch"
            )));
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
    Ok(())
}

fn valid_format(format: &str) -> bool {
    matches!(format, "png" | "jpeg" | "tiff" | "webp" | "bmp")
}
