use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn version_comparison_follows_semver_prerelease_ordering() {
    assert!(protocol_is_newer("0.2.0", "0.1.0"));
    assert!(protocol_is_newer("0.2.0-alpha.2", "0.2.0-alpha.1"));
    assert!(protocol_is_newer("0.2.0", "0.2.0-beta.3"));
    assert!(!protocol_is_newer("0.2.0-alpha.1", "0.2.0"));
    assert!(!protocol_is_newer("0.1.0", "0.1.0"));
    assert!(!protocol_is_newer("0.0.9", "0.1.0"));
    // A leading "v" from a careless manifest is tolerated.
    assert!(protocol_is_newer("v0.2.0", "0.1.0"));
}

fn protocol_is_newer(fetched: &str, installed: &str) -> bool {
    super::protocol::is_newer(fetched, installed).unwrap()
}

#[test]
fn channel_setting_resolves_and_survives_unknown_values() {
    assert_eq!(UpdateChannelSetting::Alpha.resolve(), UpdateChannel::Alpha);
    // Unknown strings fall back to Auto instead of failing the settings parse.
    let parsed: UpdateChannelSetting = serde_json::from_str("\"nightly\"").unwrap();
    assert_eq!(parsed, UpdateChannelSetting::Auto);
    let parsed: UpdateChannelSetting = serde_json::from_str("\"beta\"").unwrap();
    assert_eq!(parsed, UpdateChannelSetting::Beta);
}

/// One-shot HTTP server on an ephemeral port; answers the next request with
/// `body` and returns the base URL to point the client at.
fn serve_once(status_line: &'static str, headers: &'static str, body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
        let _ = write!(
            stream,
            "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\n{headers}\r\n",
            body.len()
        );
        let _ = stream.write_all(&body);
    });
    format!("http://{addr}")
}

#[test]
fn check_latest_uses_github_releases_semver_channels_and_checksums() {
    let hash = "ab".repeat(32);
    let base = serve_sequence(|base| {
        let filename = format!(
            "plotx-0.9.0-alpha.10-{}-{}.zip",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let releases = format!(
            r#"[
                {{"tag_name":"v0.9.0-alpha.2","draft":false,"assets":[]}},
                {{"tag_name":"v0.8.0","draft":false,"assets":[]}},
                {{"tag_name":"v0.9.0-alpha.10","draft":false,"assets":[
                    {{"name":"{filename}","browser_download_url":"{base}/download/{filename}"}},
                    {{"name":"SHA256SUMS","browser_download_url":"{base}/SHA256SUMS"}}
                ]}}
            ]"#
        );
        vec![
            MockResponse::json("/releases?per_page=100&page=1", releases),
            MockResponse::text("/SHA256SUMS", format!("{hash}  {filename}\n")),
        ]
    });
    let asset = super::protocol::check_latest(&base, UpdateChannel::Alpha)
        .unwrap()
        .unwrap();
    assert_eq!(asset.version, "0.9.0-alpha.10");
    assert_eq!(asset.sha256, hash);
    assert!(asset.url.ends_with(".zip"));

    let base = serve_sequence(|_| {
        vec![MockResponse::json(
            "/releases?per_page=100&page=1",
            r#"[{"tag_name":"v0.9.0-alpha.1","draft":false,"assets":[]}]"#,
        )]
    });
    assert!(
        super::protocol::check_latest(&base, UpdateChannel::Stable)
            .unwrap()
            .is_none()
    );
}

#[test]
fn check_latest_accepts_github_asset_digest_without_fetching_checksums() {
    let hash = "cd".repeat(32);
    let base = serve_sequence(|base| {
        let filename = format!(
            "plotx-1.0.0-{}-{}.zip",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        vec![MockResponse::json(
            "/releases?per_page=100&page=1",
            format!(
                r#"[{{"tag_name":"v1.0.0","draft":false,"assets":[{{
                    "name":"{filename}",
                    "browser_download_url":"{base}/download/{filename}",
                    "digest":"sha256:{hash}"
                }}]}}]"#
            ),
        )]
    });
    let asset = super::protocol::check_latest(&base, UpdateChannel::Stable)
        .unwrap()
        .unwrap();
    assert_eq!(asset.version, "1.0.0");
    assert_eq!(asset.sha256, hash);
}

#[test]
fn check_latest_scans_beyond_one_thousand_releases() {
    let hash = "ef".repeat(32);
    let base = serve_sequence(|base| {
        let ordinary = r#"{"tag_name":"v0.1.0-alpha.1","draft":false,"assets":[]}"#;
        let full_page = format!("[{}]", vec![ordinary; 100].join(","));
        let filename = format!(
            "plotx-2.0.0-{}-{}.zip",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let mut responses = (1..=10)
            .map(|page| {
                MockResponse::json(
                    &format!("/releases?per_page=100&page={page}"),
                    full_page.clone(),
                )
            })
            .collect::<Vec<_>>();
        responses.push(MockResponse::json(
            "/releases?per_page=100&page=11",
            format!(
                r#"[{{"tag_name":"v2.0.0","draft":false,"assets":[{{
                    "name":"{filename}",
                    "browser_download_url":"{base}/download/{filename}",
                    "digest":"sha256:{hash}"
                }}]}}]"#
            ),
        ));
        responses
    });

    let asset = super::protocol::check_latest(&base, UpdateChannel::Stable)
        .unwrap()
        .unwrap();
    assert_eq!(asset.version, "2.0.0");
    assert_eq!(asset.sha256, hash);
}

#[test]
fn github_rate_limit_preserves_retry_delay() {
    let base = serve_once(
        "429 Too Many Requests",
        "Retry-After: 120\r\n",
        br#"{"message":"rate limited"}"#.to_vec(),
    );
    let error = super::protocol::check_latest(&base, UpdateChannel::Stable).unwrap_err();
    assert!(matches!(
        error,
        UpdateError::RateLimited { retry_after }
            if retry_after == std::time::Duration::from_secs(120)
    ));
}

#[test]
fn ordinary_github_forbidden_response_is_not_misreported_as_rate_limit() {
    let base = serve_once("403 Forbidden", "", br#"{"message":"forbidden"}"#.to_vec());
    let error = super::protocol::check_latest(&base, UpdateChannel::Stable).unwrap_err();
    assert!(matches!(error, UpdateError::Http(_)));
}

struct MockResponse {
    path: String,
    content_type: &'static str,
    body: Vec<u8>,
}

impl MockResponse {
    fn json(path: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            content_type: "application/json",
            body: body.into(),
        }
    }

    fn text(path: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            content_type: "text/plain",
            body: body.into(),
        }
    }
}

fn serve_sequence(build: impl FnOnce(&str) -> Vec<MockResponse>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let responses = build(&base);
    std::thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let read = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..read]);
            assert!(
                request.starts_with(&format!("GET {} HTTP/1.1", response.path)),
                "unexpected request: {request}"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
                response.body.len(),
                response.content_type
            )
            .unwrap();
            stream.write_all(&response.body).unwrap();
        }
    });
    base
}

#[test]
fn swap_binary_replaces_target_and_rolls_back_on_failure() {
    let dir = std::env::temp_dir().join("plotx-swap-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("plotx.exe");
    let new = dir.join("incoming.bin");
    std::fs::write(&target, b"old build").unwrap();
    std::fs::write(&new, b"new build").unwrap();

    super::install::swap_binary(&new, &target).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"new build");
    assert_eq!(
        std::fs::read(dir.join("plotx.exe.old")).unwrap(),
        b"old build"
    );

    // A missing source rolls the rename back so the target keeps existing.
    let err = super::install::swap_binary(&dir.join("missing.bin"), &target);
    assert!(err.is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"new build");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn helper_swap_cleans_staging_and_keeps_rollback_binary() {
    let dir = std::env::temp_dir().join("plotx-helper-swap-test");
    let _ = std::fs::remove_dir_all(&dir);
    let staging = dir.join("plotx-update-test-operation");
    std::fs::create_dir_all(&staging).unwrap();
    let target = dir.join("plotx.exe");
    let new = staging.join("plotx.extracted");
    std::fs::write(&target, b"old build").unwrap();
    std::fs::write(&new, b"new build").unwrap();
    std::fs::write(staging.join("download.zip"), b"archive").unwrap();

    super::install::apply_staged_update(&new, &target, &staging).unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"new build");
    assert_eq!(
        std::fs::read(dir.join("plotx.exe.old")).unwrap(),
        b"old build"
    );
    assert!(!staging.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn startup_cleanup_preserves_active_staging_directories() {
    let root = std::env::temp_dir().join(format!("plotx-cleanup-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let completed = root.join("plotx-update-completed");
    let downloading = root.join("plotx-update-downloading");
    let prepared = root.join("plotx-update-prepared");
    std::fs::create_dir_all(&completed).unwrap();
    std::fs::create_dir_all(&downloading).unwrap();
    std::fs::create_dir_all(&prepared).unwrap();
    std::fs::write(completed.join("plotx-update-helper.exe"), b"helper").unwrap();
    std::fs::write(downloading.join("download.zip"), b"partial").unwrap();
    std::fs::write(prepared.join("plotx-update-helper.exe"), b"helper").unwrap();
    std::fs::write(prepared.join("update.extracted"), b"ready").unwrap();

    super::install::cleanup_completed_staging(&root);

    assert!(!completed.exists());
    assert!(downloading.exists());
    assert!(prepared.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn extract_binary_pulls_matching_entry_from_zip_and_passes_raw_files_through() {
    let dir = std::env::temp_dir().join("plotx-extract-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let installed = std::path::Path::new("C:/somewhere/plotx.exe");

    let raw = dir.join("plotx-raw.bin");
    std::fs::write(&raw, b"raw binary").unwrap();
    assert_eq!(
        super::install::extract_binary(&raw, installed).unwrap(),
        raw
    );

    let archive_path = dir.join("plotx-0.2.0.zip");
    let file = std::fs::File::create(&archive_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    zip.start_file("README.txt", options).unwrap();
    zip.write_all(b"readme").unwrap();
    zip.start_file("plotx.exe", options).unwrap();
    zip.write_all(b"zipped binary").unwrap();
    zip.finish().unwrap();

    let extracted = super::install::extract_binary(&archive_path, installed).unwrap();
    assert_eq!(std::fs::read(&extracted).unwrap(), b"zipped binary");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn download_verifies_sha256_and_rejects_tampering() {
    let payload = b"new plotx build".to_vec();
    let good_hash = {
        use sha2::{Digest, Sha256};
        super::protocol::hex_string(&Sha256::digest(&payload))
    };

    let base = serve_once(
        "200 OK",
        "Content-Type: application/octet-stream\r\n",
        payload.clone(),
    );
    let asset = ReleaseAsset {
        version: "9.9.9".into(),
        url: format!("{base}/plotx-9.9.9.zip"),
        sha256: good_hash.clone(),
    };
    let mut saw_progress = false;
    let path = super::protocol::download(&asset, |_| saw_progress = true).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), payload);
    assert!(saw_progress);
    let first_staging = path.parent().unwrap().to_owned();

    let base = serve_once("200 OK", "", payload);
    let tampered = ReleaseAsset {
        sha256: "0".repeat(64),
        url: format!("{base}/plotx-9.9.9.zip"),
        version: "9.9.9".into(),
    };
    let err = super::protocol::download(&tampered, |_| {}).unwrap_err();
    assert!(matches!(err, UpdateError::Integrity), "got {err:?}");
    assert_ne!(first_staging, super::protocol::download_dir());
}
