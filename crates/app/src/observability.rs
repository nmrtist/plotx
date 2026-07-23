use log::{LevelFilter, Log, Metadata, Record};
use std::backtrace::Backtrace;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_KEEP_COUNT: usize = 5;
const CRASH_KEEP_COUNT: usize = 10;
const LOG_TAIL_LINES: usize = 200;
const CRASH_MARKER: &str = "latest";
static LOGGER: SessionLogger = SessionLogger {
    state: Mutex::new(None),
};
static HANDLING_PANIC: AtomicBool = AtomicBool::new(false);
static MAIN_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();
static PENDING_DIALOG: Mutex<Option<PathBuf>> = Mutex::new(None);

struct LoggerState {
    file: BufWriter<File>,
    path: PathBuf,
}

struct SessionLogger {
    state: Mutex<Option<LoggerState>>,
}

impl Log for SessionLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info && metadata.target().starts_with("plotx")
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} [{}] {}\n",
            utc_timestamp(SystemTime::now()),
            record.level(),
            record.target(),
            record.args()
        );
        let mut wrote_to_file = false;
        if let Ok(mut state) = self.state.lock()
            && let Some(state) = state.as_mut()
        {
            wrote_to_file = state.file.write_all(line.as_bytes()).is_ok();
        }
        if cfg!(debug_assertions) || !wrote_to_file {
            let _ = std::io::stderr().lock().write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock()
            && let Some(state) = state.as_mut()
        {
            let _ = state.file.flush();
        }
    }
}

pub(crate) fn initialize() {
    let _ = MAIN_THREAD.set(std::thread::current().id());
    let logger_installed = log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Info));
    let Some(root) = plotx_core::settings::data_local_dir() else {
        install_panic_hook(None);
        if logger_installed.is_ok() {
            log::warn!("PlotX application data directory is unavailable; using stderr only");
        }
        return;
    };
    if let Err(error) = initialize_logger(&root)
        && logger_installed.is_ok()
    {
        log::warn!("could not initialize the session log; using stderr only: {error}");
    }
    install_panic_hook(Some(root));
    log::info!("PlotX {} session started", env!("CARGO_PKG_VERSION"));
    log::logger().flush();
}

fn initialize_logger(root: &Path) -> io::Result<()> {
    let logs = root.join("logs");
    std::fs::create_dir_all(&logs)?;
    let path = unique_file_path(&logs, "session", "log");
    let file = OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(&path)?;
    let mut state = LOGGER
        .state
        .lock()
        .map_err(|_| io::Error::other("session logger lock is poisoned"))?;
    *state = Some(LoggerState {
        file: BufWriter::new(file),
        path,
    });
    drop(state);
    if let Err(error) = rotate_files(&logs, "session-", LOG_KEEP_COUNT) {
        log::warn!("could not rotate session logs: {error}");
    }
    Ok(())
}

fn install_panic_hook(root: Option<PathBuf>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        log_panic_summary(info);
        if HANDLING_PANIC.swap(true, Ordering::SeqCst) {
            return;
        }
        let report_path = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle_panic(root.as_deref(), info)
        }))
        .ok()
        .flatten();
        HANDLING_PANIC.store(false, Ordering::SeqCst);
        if let Some(path) = report_path {
            if is_main_thread() {
                show_crash_dialog(&path);
            } else if let Ok(mut pending) = PENDING_DIALOG.lock() {
                *pending = Some(path);
            }
        }
    }));
}

fn log_panic_summary(info: &PanicHookInfo<'_>) {
    let message = panic_message(info);
    let location = info
        .location()
        .map(|location| format!("{}:{}", location.file(), location.line()))
        .unwrap_or_else(|| "unknown".to_owned());
    let thread = std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_owned();
    log::error!("panic in thread {thread} at {location}: {message}");
}

fn handle_panic(root: Option<&Path>, info: &PanicHookInfo<'_>) -> Option<PathBuf> {
    let root = root?;
    let message = panic_message(info);
    let location = info
        .location()
        .map(|location| format!("{}:{}", location.file(), location.line()))
        .unwrap_or_else(|| "unknown".to_owned());
    let thread = std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_owned();
    let backtrace = Backtrace::force_capture();
    let report = format!(
        "PlotX crash report\n\
         Version: {}\n\
         OS: {}\n\
         Architecture: {}\n\
         Timestamp (UTC): {}\n\
         Thread: {thread}\n\
         Location: {location}\n\
         Panic: {message}\n\n\
         Backtrace:\n{backtrace}\n\n\
         Session log tail (last {LOG_TAIL_LINES} lines):\n{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        utc_timestamp(SystemTime::now()),
        session_log_tail(LOG_TAIL_LINES),
    );
    let path = write_crash_report(root, &report).ok()?;
    log::error!("crash report saved to {}", path.display());
    Some(path)
}

fn is_main_thread() -> bool {
    MAIN_THREAD
        .get()
        .is_some_and(|main| *main == std::thread::current().id())
}

fn show_crash_dialog(path: &Path) {
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("PlotX internal error")
        .set_description(format!(
            "PlotX encountered an internal error.\n\nA crash report was saved to:\n{}\n\nPlease attach this file to an issue at https://github.com/nmrtist/plotx/issues.",
            path.display()
        ))
        .show();
}

pub(crate) fn show_pending_crash_dialog() {
    let path = PENDING_DIALOG
        .lock()
        .ok()
        .and_then(|mut pending| pending.take());
    if let Some(path) = path {
        let _ = std::panic::catch_unwind(|| show_crash_dialog(&path));
    }
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    info.payload()
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_owned())
}

fn write_crash_report(root: &Path, report: &str) -> io::Result<PathBuf> {
    let crashes = root.join("crashes");
    std::fs::create_dir_all(&crashes)?;
    let path = unique_file_path(&crashes, "crash", "txt");
    std::fs::write(&path, report)?;
    let _ = std::fs::write(
        crashes.join(CRASH_MARKER),
        path.as_os_str().as_encoded_bytes(),
    );
    let _ = rotate_files(&crashes, "crash-", CRASH_KEEP_COUNT);
    Ok(path)
}

pub(crate) fn pending_crash_report() -> Option<PathBuf> {
    let root = plotx_core::settings::data_local_dir()?;
    read_marker(&root)
}

pub(crate) fn acknowledge_crash_report() {
    let Some(root) = plotx_core::settings::data_local_dir() else {
        return;
    };
    if let Err(error) = clear_marker(&root) {
        log::warn!("failed to clear acknowledged crash report marker: {error}");
    }
}

fn read_marker(root: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(root.join("crashes").join(CRASH_MARKER)).ok()?;
    let path = PathBuf::from(String::from_utf8_lossy(&bytes).trim());
    path.is_file().then_some(path)
}

fn clear_marker(root: &Path) -> io::Result<()> {
    match std::fs::remove_file(root.join("crashes").join(CRASH_MARKER)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn session_log_tail(line_count: usize) -> String {
    let path = LOGGER.state.lock().ok().and_then(|mut state| {
        state.as_mut().map(|state| {
            let _ = state.file.flush();
            state.path.clone()
        })
    });
    path.and_then(|path| read_tail(&path, line_count).ok())
        .unwrap_or_else(|| "<session log unavailable>".to_owned())
}

fn read_tail(path: &Path, line_count: usize) -> io::Result<String> {
    let mut text = String::new();
    File::open(path)?.read_to_string(&mut text)?;
    let lines = text.lines().collect::<Vec<_>>();
    Ok(lines[lines.len().saturating_sub(line_count)..].join("\n"))
}

fn rotate_files(dir: &Path, prefix: &str, keep: usize) -> io::Result<()> {
    let mut files = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry.file_name().to_string_lossy().starts_with(prefix)
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| {
        entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH)
    });
    let remove_count = files.len().saturating_sub(keep);
    for entry in files.into_iter().take(remove_count) {
        std::fs::remove_file(entry.path())?;
    }
    Ok(())
}

fn unique_file_path(dir: &Path, prefix: &str, extension: &str) -> PathBuf {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    for sequence in 0_u32.. {
        let path = dir.join(format!(
            "{prefix}-{seconds}-{}-{sequence}.{extension}",
            std::process::id()
        ));
        if !path.exists() {
            return path;
        }
    }
    unreachable!("the filename sequence is unbounded")
}

fn utc_timestamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_date(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3_600,
        day_seconds % 3_600 / 60,
        day_seconds % 60
    )
}

fn civil_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_piece = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_piece + 2) / 5 + 1;
    let month = month_piece + if month_piece < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
#[path = "observability_tests.rs"]
mod tests;
