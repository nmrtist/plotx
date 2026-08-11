use super::*;
use crate::state::{
    AssetId, AssetRecord, ContentItem, ContentKind, ObjectFrame, RasterImageContent,
};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

fn png_bytes() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([12, 34, 56, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn final_v1_panel_and_shared_asset_round_trip_deterministically() {
    let mut app = super::tests::sample_app();
    let page = &mut app.doc.canvases[0];
    let plot = page.objects[0].id;
    let panel = page.create_panel("Panel a".to_owned(), page.objects[0].frame);
    page.objects[0].frame.x = 0.0;
    page.objects[0].frame.y = 0.0;
    page.panel_mut(panel).unwrap().item_order.push(plot);
    page.next_panel_label_slot = 9;

    let bytes = png_bytes();
    let asset = AssetId::new();
    app.doc.assets.insert(
        asset,
        AssetRecord {
            id: asset,
            sha256: Sha256::digest(&bytes).into(),
            format: "png".to_owned(),
            pixel_size: [2, 2],
            bytes: bytes.clone(),
        },
    );
    let image_id = app.doc.canvases[0].allocate_object_id();
    app.doc.canvases[0].objects.push(ContentItem {
        id: image_id,
        name: "microscopy".to_owned(),
        frame: ObjectFrame::new(4.0, 5.0, 20.0, 10.0),
        locked: false,
        visible: true,
        kind: ContentKind::RasterImage(RasterImageContent::new(asset)),
    });
    app.doc.canvases[0]
        .panel_mut(panel)
        .unwrap()
        .item_order
        .push(image_id);

    let view_a = canvas_to_view(&app.doc.datasets, &app.doc.canvases[0], "view").unwrap();
    let view_b = canvas_to_view(&app.doc.datasets, &app.doc.canvases[0], "view").unwrap();
    assert_eq!(
        serde_json::to_vec(&view_a).unwrap(),
        serde_json::to_vec(&view_b).unwrap()
    );

    let path = super::tests::temp_project("final-v1-panel-asset");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let loaded_page = &loaded.doc.canvases[0];
    assert_eq!(loaded_page.panels[0].item_order, vec![plot, image_id]);
    assert_eq!(loaded_page.next_panel_label_slot, 9);
    assert_eq!(
        loaded_page.content_page_frame(image_id),
        app.doc.canvases[0].content_page_frame(image_id)
    );
    assert_eq!(loaded.doc.assets[&asset].bytes, bytes);
}

#[test]
fn missing_embedded_image_degrades_without_discarding_the_v1_content_item() {
    let mut app = super::tests::sample_app();
    let bytes = png_bytes();
    let asset = AssetId::new();
    app.doc.assets.insert(
        asset,
        AssetRecord {
            id: asset,
            sha256: Sha256::digest(&bytes).into(),
            format: "png".to_owned(),
            pixel_size: [2, 2],
            bytes,
        },
    );
    let image = app.doc.canvases[0].allocate_object_id();
    app.doc.canvases[0].objects.push(ContentItem {
        id: image,
        name: "missing after archive damage".to_owned(),
        frame: ObjectFrame::new(4.0, 5.0, 20.0, 10.0),
        locked: false,
        visible: true,
        kind: ContentKind::RasterImage(RasterImageContent::new(asset)),
    });

    let valid = super::tests::temp_project("asset-degradation-source");
    let damaged = super::tests::temp_project("asset-degradation-missing");
    let _ = std::fs::remove_file(&valid);
    let _ = std::fs::remove_file(&damaged);
    save_project(&app, &valid, false).unwrap();

    let mut source = zip::ZipArchive::new(std::fs::File::open(&valid).unwrap()).unwrap();
    let output = std::fs::File::create(&damaged).unwrap();
    let mut target = zip::ZipWriter::new(output);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        if entry.name().starts_with("assets/") {
            continue;
        }
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();
        target
            .start_file(entry.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        target.write_all(&contents).unwrap();
    }
    target.finish().unwrap();

    let loaded = load_project(&damaged).expect("missing images must not block project open");
    assert!(!loaded.session.project_load_warnings.is_empty());
    assert!(!loaded.doc.assets.contains_key(&asset));
    assert!(matches!(
        &loaded.doc.canvases[0].object(image).unwrap().kind,
        ContentKind::RasterImage(content) if content.asset == asset
    ));
    let resave = super::tests::temp_project("asset-degradation-resave");
    let error = save_project(&loaded, &resave, false).unwrap_err();
    assert!(error.to_string().contains("missing referenced asset"));
    assert!(!resave.exists());

    std::fs::remove_file(valid).unwrap();
    std::fs::remove_file(damaged).unwrap();
}

#[test]
fn image_reference_absent_from_manifest_produces_a_load_warning() {
    let mut app = super::tests::sample_app();
    let bytes = png_bytes();
    let asset = AssetId::new();
    app.doc.assets.insert(
        asset,
        AssetRecord {
            id: asset,
            sha256: Sha256::digest(&bytes).into(),
            format: "png".to_owned(),
            pixel_size: [2, 2],
            bytes,
        },
    );
    let image = app.doc.canvases[0].allocate_object_id();
    app.doc.canvases[0].objects.push(ContentItem {
        id: image,
        name: "undeclared image".to_owned(),
        frame: ObjectFrame::new(4.0, 5.0, 20.0, 10.0),
        locked: false,
        visible: true,
        kind: ContentKind::RasterImage(RasterImageContent::new(asset)),
    });

    let valid = super::tests::temp_project("undeclared-asset-source");
    let damaged = super::tests::temp_project("undeclared-asset-manifest");
    let _ = std::fs::remove_file(&valid);
    let _ = std::fs::remove_file(&damaged);
    save_project(&app, &valid, false).unwrap();

    let mut source = zip::ZipArchive::new(std::fs::File::open(&valid).unwrap()).unwrap();
    let output = std::fs::File::create(&damaged).unwrap();
    let mut target = zip::ZipWriter::new(output);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();
        if entry.name() == "manifest.json" {
            let mut manifest: serde_json::Value = serde_json::from_slice(&contents).unwrap();
            manifest["assets"] = serde_json::json!([]);
            contents = serde_json::to_vec(&manifest).unwrap();
        }
        target
            .start_file(entry.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        target.write_all(&contents).unwrap();
    }
    target.finish().unwrap();

    let loaded = load_project(&damaged).expect("undeclared image must not block project open");
    assert!(loaded.session.project_load_warnings.iter().any(|warning| {
        warning.contains(&asset.to_string())
            && warning.contains("not listed in the project manifest")
    }));
    assert!(!loaded.doc.assets.contains_key(&asset));
    assert!(matches!(
        &loaded.doc.canvases[0].object(image).unwrap().kind,
        ContentKind::RasterImage(content) if content.asset == asset
    ));

    std::fs::remove_file(valid).unwrap();
    std::fs::remove_file(damaged).unwrap();
}

#[test]
fn distinct_asset_ids_can_share_one_content_addressed_blob() {
    let mut app = super::tests::sample_app();
    let bytes = png_bytes();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let first = AssetId::new();
    let second = AssetId::new();
    for id in [first, second] {
        app.doc.assets.insert(
            id,
            AssetRecord {
                id,
                sha256: digest,
                format: "png".to_owned(),
                pixel_size: [2, 2],
                bytes: bytes.clone(),
            },
        );
        let object = app.doc.canvases[0].allocate_object_id();
        app.doc.canvases[0].objects.push(ContentItem {
            id: object,
            name: format!("image {id}"),
            frame: ObjectFrame::new(0.0, 0.0, 10.0, 10.0),
            locked: false,
            visible: true,
            kind: ContentKind::RasterImage(RasterImageContent::new(id)),
        });
    }

    let path = super::tests::temp_project("shared-content-addressed-blob");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let asset_entries = (0..archive.len())
        .filter(|&index| {
            archive
                .by_index(index)
                .unwrap()
                .name()
                .starts_with("assets/")
        })
        .count();
    assert_eq!(asset_entries, 1);
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(loaded.doc.assets[&first].bytes, bytes);
    assert_eq!(loaded.doc.assets[&second].bytes, bytes);
}

#[test]
fn shared_blob_metadata_conflicts_are_rejected_before_save() {
    let mut app = super::tests::sample_app();
    let bytes = b"same-blob-conflicting-metadata".to_vec();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    for (pixel_size, x) in [([4, 3], 0.0), ([8, 6], 12.0)] {
        let id = AssetId::new();
        app.doc.assets.insert(
            id,
            AssetRecord {
                id,
                sha256: digest,
                format: "png".to_owned(),
                pixel_size,
                bytes: bytes.clone(),
            },
        );
        let object = app.doc.canvases[0].allocate_object_id();
        app.doc.canvases[0].objects.push(ContentItem {
            id: object,
            name: format!("image {id}"),
            frame: ObjectFrame::new(x, 0.0, 10.0, 10.0),
            locked: false,
            visible: true,
            kind: ContentKind::RasterImage(RasterImageContent::new(id)),
        });
    }

    let path = super::tests::temp_project("conflicting-shared-blob-metadata");
    let _ = std::fs::remove_file(&path);
    let error = save_project(&app, &path, false).unwrap_err();
    assert!(error.to_string().contains("conflicting metadata"));
    assert!(!path.exists());
}

#[test]
fn asset_record_id_must_match_its_map_key() {
    let mut app = super::tests::sample_app();
    let key = AssetId::new();
    let bytes = b"mismatched-asset-id".to_vec();
    app.doc.assets.insert(
        key,
        AssetRecord {
            id: AssetId::new(),
            sha256: Sha256::digest(&bytes).into(),
            format: "png".to_owned(),
            pixel_size: [2, 2],
            bytes,
        },
    );
    let object = app.doc.canvases[0].allocate_object_id();
    app.doc.canvases[0].objects.push(ContentItem {
        id: object,
        name: "mismatched asset".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 10.0, 10.0),
        locked: false,
        visible: true,
        kind: ContentKind::RasterImage(RasterImageContent::new(key)),
    });

    let path = super::tests::temp_project("mismatched-asset-id");
    let _ = std::fs::remove_file(&path);
    let error = save_project(&app, &path, false).unwrap_err();
    assert!(error.to_string().contains("does not match record id"));
    assert!(!path.exists());
}

#[test]
fn schema_v1_requires_explicit_hierarchy_and_parent_fields() {
    let missing_hierarchy = serde_json::json!({
        "id":"view", "role":"view", "classification":{"domain":"visualization","object":"page"},
        "name":"page", "next_object_id":1, "panel_label_style":"lower_alpha",
        "layout":{"size_mm":[100.0,100.0]}, "objects":[]
    });
    assert!(serde_json::from_value::<ViewObject>(missing_hierarchy).is_err());

    let missing_parent = serde_json::json!({
        "id":"view", "role":"view", "classification":{"domain":"visualization","object":"page"},
        "name":"page", "next_object_id":2, "next_panel_label_slot":0, "panel_label_style":"lower_alpha",
        "layout":{"size_mm":[100.0,100.0]}, "panels":[], "loose_item_order":["1"], "groups":[],
        "objects":[{"id":"1","name":"text","kind":"text","series":[],"frame":{"x":0.0,"y":0.0,"width":1.0,"height":1.0},"locked":false,"visible":true}]
    });
    assert!(serde_json::from_value::<ViewObject>(missing_parent).is_err());
}
