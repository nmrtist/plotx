use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("pr-check") => {
            let stage = match args.next() {
                None => None,
                Some(name) => Some(PrCheckStage::parse(&name).ok_or_else(|| {
                    format!("unknown pr-check stage: {name} (expected quick, lint, or test)")
                })?),
            };
            if args.next().is_some() {
                return Err("usage: cargo pr-check [quick|lint|test]".to_owned());
            }
            pr_check(stage)
        }
        Some("licenses") => {
            if args.next().is_some() {
                return Err("usage: cargo xtask licenses".to_owned());
            }
            licenses()
        }
        Some(command) => Err(format!("unknown xtask command: {command}")),
        None => Err("usage: cargo xtask <command>".to_owned()),
    }
}

/// One CI job per stage: `quick` needs no workspace build and fails fast,
/// `lint` and `test` each carry a separately cached build. A bare
/// `cargo pr-check` runs every stage and stays the local pre-submission check.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PrCheckStage {
    Quick,
    Lint,
    Test,
}

impl PrCheckStage {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "quick" => Some(Self::Quick),
            "lint" => Some(Self::Lint),
            "test" => Some(Self::Test),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Lint => "lint",
            Self::Test => "test",
        }
    }
}

enum PrCheckStep {
    Cargo {
        name: &'static str,
        args: &'static [&'static str],
    },
    RustFileSizes,
    DependencyPolicy,
}

fn pr_check(stage: Option<PrCheckStage>) -> Result<(), String> {
    use PrCheckStage::{Lint, Quick, Test};
    use PrCheckStep::{Cargo, DependencyPolicy, RustFileSizes};

    let steps: [(PrCheckStage, PrCheckStep); 7] = [
        (
            Quick,
            Cargo {
                name: "fmt",
                args: &["fmt", "--all", "--check", "--quiet"],
            },
        ),
        (Quick, RustFileSizes),
        (Quick, DependencyPolicy),
        // Cheap, and it fails long before Clippy would: the default feature
        // set of both frontends is the one contributors build, and nothing
        // else here compiles it.
        (
            Lint,
            Cargo {
                name: "check (default frontends)",
                args: &[
                    "check",
                    "-p",
                    "plotx",
                    "-p",
                    "plotx-cli",
                    "--locked",
                    "--quiet",
                ],
            },
        ),
        (
            Lint,
            Cargo {
                name: "clippy",
                args: &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--all-features",
                    "--locked",
                    "--quiet",
                    "--",
                    "-D",
                    "warnings",
                ],
            },
        ),
        (
            Test,
            Cargo {
                name: "test",
                args: &[
                    "test",
                    "--workspace",
                    "--all-features",
                    "--locked",
                    "--profile",
                    "pr-check",
                    "--quiet",
                ],
            },
        ),
        // `--all-features` always enables plotx-core/datafusion, so the
        // reference executor that default builds depend on is never exercised
        // above.
        (
            Test,
            Cargo {
                name: "test (reference backend)",
                args: &[
                    "test",
                    "-p",
                    "plotx-core",
                    "--no-default-features",
                    "--locked",
                    "--profile",
                    "pr-check",
                    "--quiet",
                ],
            },
        ),
    ];

    let repo_root = repo_root()?;
    let started_at = Instant::now();

    match stage {
        None => println!("pr-check"),
        Some(stage) => println!("pr-check ({} stage)", stage.name()),
    }

    let selected: Vec<&PrCheckStep> = steps
        .iter()
        .filter(|(step_stage, _)| stage.is_none_or(|stage| stage == *step_stage))
        .map(|(_, step)| step)
        .collect();
    let total = selected.len();
    for (index, step) in selected.into_iter().enumerate() {
        let index = index + 1;
        match step {
            Cargo { name, args } => run_cargo_step(&repo_root, index, total, name, args)?,
            RustFileSizes => assert_rust_file_sizes(&repo_root, index, total)?,
            DependencyPolicy => run_cargo_deny_step(&repo_root, index, total)?,
        }
    }

    println!(
        "ok pr-check passed ({})",
        format_duration(started_at.elapsed())
    );
    Ok(())
}

fn licenses() -> Result<(), String> {
    let repo_root = repo_root()?;
    let output_file = repo_root.join("dist").join("THIRD-PARTY-LICENSES.html");
    let started_at = Instant::now();

    print!("generating third-party license bundle ... ");
    if let Some(parent) = output_file.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    let about_args = [
        "about",
        "generate",
        "-c",
        "xtask/about.toml",
        "xtask/about.hbs",
        "-o",
    ];
    let output = Command::new("cargo")
        .args(about_args)
        .arg(&output_file)
        .current_dir(&repo_root)
        .output()
        .map_err(|error| format!("failed to run cargo about: {error}"))?;

    if !output.status.success() {
        print_command_failure("cargo", &about_args, &output);
        let hint = if String::from_utf8_lossy(&output.stderr).contains("no such command") {
            " (install it with `cargo install cargo-about`)"
        } else {
            ""
        };
        return Err(format!(
            "cargo about generate failed with {}{hint}.",
            output.status
        ));
    }

    let kib = fs::metadata(&output_file)
        .map(|meta| meta.len())
        .unwrap_or(0)
        / 1024;
    println!(
        "ok ({}, {kib} KiB, {})",
        output_file.display(),
        format_duration(started_at.elapsed())
    );
    Ok(())
}

fn repo_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "could not resolve repository root".to_owned())
}

fn run_cargo_step(
    repo_root: &Path,
    index: usize,
    total: usize,
    name: &str,
    args: &[&str],
) -> Result<(), String> {
    let started_at = Instant::now();
    print_step(index, total, name)?;
    let output = cargo_output(repo_root, args)?;

    if !output.status.success() {
        print_command_failure("cargo", args, &output);
        return Err(format!("{name} failed with {}.", output.status));
    }

    println!("ok ({})", format_duration(started_at.elapsed()));
    Ok(())
}

fn print_step(index: usize, total: usize, name: &str) -> Result<(), String> {
    print!("[{index}/{total}] {name} ... ");
    io::stdout()
        .flush()
        .map_err(|error| format!("failed to flush verification output: {error}"))
}

fn cargo_output(repo_root: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("cargo")
        .args(args)
        .current_dir(repo_root)
        .env("CARGO_TERM_COLOR", "always")
        .env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
        .output()
        .map_err(|error| format!("failed to run cargo {}: {error}", args.join(" ")))
}

fn run_cargo_deny_step(repo_root: &Path, index: usize, total: usize) -> Result<(), String> {
    let args = ["deny", "--locked", "check"];
    let started_at = Instant::now();
    print_step(index, total, "dependency policy")?;
    let output = cargo_output(repo_root, &args)?;

    if output.status.success() {
        println!("ok ({})", format_duration(started_at.elapsed()));
        return Ok(());
    }

    print_command_failure("cargo", &args, &output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("no such command: `deny`") {
        return Err(
            "cargo-deny is required; install it with `cargo install --locked cargo-deny`."
                .to_owned(),
        );
    }
    Err(format!("dependency policy failed with {}.", output.status))
}

fn assert_rust_file_sizes(repo_root: &Path, index: usize, total: usize) -> Result<(), String> {
    let started_at = Instant::now();
    print_step(index, total, "rust file size")?;

    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "*.rs",
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|error| format!("failed to list tracked Rust files: {error}"))?;

    if !output.status.success() {
        print_command_failure(
            "git",
            &[
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "--",
                "*.rs",
            ],
            &output,
        );
        return Err(format!("failed to list Rust files with {}.", output.status));
    }

    let files = String::from_utf8(output.stdout)
        .map_err(|error| format!("git produced non-UTF-8 file output: {error}"))?;
    let rust_files: Vec<_> = files.lines().collect();
    let oversized_files = oversized_rust_files(repo_root, rust_files.iter().copied())?;

    if oversized_files.is_empty() {
        println!(
            "ok ({} files, {})",
            rust_files.len(),
            format_duration(started_at.elapsed())
        );
        Ok(())
    } else {
        println!("failed");
        for (path, line_count) in oversized_files {
            eprintln!("{path} has {line_count} physical lines (limit 800).");
        }
        Err(
            "Split the files above; keep Rust sources under 800 physical lines (soft target ~500)."
                .to_owned(),
        )
    }
}

fn oversized_rust_files<'a>(
    repo_root: &Path,
    files: impl Iterator<Item = &'a str>,
) -> Result<BTreeSet<(String, usize)>, String> {
    let mut oversized_files = BTreeSet::new();
    for file in files {
        let path = repo_root.join(file);
        if !path.exists() {
            continue;
        }
        let contents = fs::read(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let line_count = physical_line_count(&contents);
        if line_count > 800 {
            oversized_files.insert((file.to_owned(), line_count));
        }
    }
    Ok(oversized_files)
}

fn physical_line_count(contents: &[u8]) -> usize {
    let newline_count = contents.iter().filter(|byte| **byte == b'\n').count();
    if contents.last().is_some_and(|byte| *byte == b'\n') {
        newline_count
    } else {
        newline_count + usize::from(!contents.is_empty())
    }
}

fn print_command_failure(program: &str, args: &[&str], output: &Output) {
    println!("failed");
    eprintln!("command: {program} {}", args.join(" "));
    print_stream("stdout", &output.stdout);
    print_stream("stderr", &output.stderr);
}

fn print_stream(name: &str, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    if text.trim().is_empty() {
        return;
    }

    eprintln!("--- {name} ---");
    eprint!("{text}");
    if !text.ends_with('\n') {
        eprintln!();
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs_f64();
    if seconds < 10.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{seconds:.0}s")
    }
}

#[cfg(test)]
mod tests {
    const ABOUT_TEMPLATE: &str = include_str!("../about.hbs");
    const OPENOPJ_LICENSE: &str =
        include_str!("../../crates/io/tests/fixtures/origin/OPENOPJ-LICENSE.txt");

    #[test]
    fn license_template_includes_the_complete_openopj_notice() {
        assert!(ABOUT_TEMPLATE.contains("OpenOPJ"));
        assert!(ABOUT_TEMPLATE.contains(
            "Copyright (c) 2012 Juliusz Gonera, Minor Laboratory, University of Virginia"
        ));
        assert!(ABOUT_TEMPLATE.contains(OPENOPJ_LICENSE.trim()));
    }

    #[test]
    fn license_template_describes_cargo_and_non_cargo_projects() {
        assert!(ABOUT_TEMPLATE.contains("Cargo dependencies"));
        assert!(ABOUT_TEMPLATE.contains("other third-party projects"));
    }
}
