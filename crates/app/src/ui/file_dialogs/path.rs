pub(super) fn ensure_extension(mut path: std::path::PathBuf, ext: &str) -> std::path::PathBuf {
    let matches = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(ext));
    if !matches {
        path.set_extension(ext);
    }
    path
}

pub(super) fn ensure_plotx_extension(path: std::path::PathBuf) -> std::path::PathBuf {
    ensure_extension(path, "plotx")
}

pub(super) fn io_error_category(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::AlreadyExists => "already_exists",
        std::io::ErrorKind::InvalidInput => "invalid_input",
        std::io::ErrorKind::InvalidData => "invalid_data",
        std::io::ErrorKind::WriteZero => "write_zero",
        std::io::ErrorKind::Interrupted => "interrupted",
        std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
        _ => "other_io",
    }
}
