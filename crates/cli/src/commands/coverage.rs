use std::process::Command;

use clap::{Args, ValueEnum};

use super::CliError;

/// Output format for `soroban-testkit coverage`.
#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    /// Per-file coverage stats printed to the terminal.
    Text,
    /// An `lcov.info` file, for IDE integrations (e.g. Coverage Gutters).
    Lcov,
    /// A browsable HTML report.
    Html,
}

/// Parses `--fail-under` as a percentage.
///
/// The value is forwarded verbatim to the coverage tool as
/// `--fail-under-lines`, so anything outside 0..=100 is either a typo (950
/// for 95) or a misunderstanding of the flag, and either way the tool's own
/// response to it is not something a user can act on. Rejecting it at the
/// parse boundary means the error names the flag and the value.
fn parse_fail_under(raw: &str) -> Result<f64, String> {
    let value: f64 = raw
        .parse()
        .map_err(|_| format!("`{raw}` is not a percentage; expected a number between 0 and 100"))?;

    // `contains` is false for NaN, which is what we want: a NaN threshold
    // never fails a run, so accepting it would silently disable the gate.
    if !(0.0..=100.0).contains(&value) {
        return Err(format!(
            "`{raw}` is outside the valid range; --fail-under must be between 0 and 100"
        ));
    }

    Ok(value)
}

/// Arguments for `soroban-testkit coverage`.
#[derive(Args)]
pub struct CoverageArgs {
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    /// Fail (non-zero exit) if line coverage is below this percentage.
    ///
    /// Must be between 0 and 100: the value is passed to the coverage tool as
    /// `--fail-under-lines`, which has no meaning outside that range.
    #[arg(long, value_name = "PCT", value_parser = parse_fail_under)]
    fail_under: Option<f64>,
    /// Open the HTML report after generating it (implies --format html).
    #[arg(long)]
    open: bool,
    /// Only collect coverage for this workspace package. Repeatable.
    /// Mutually exclusive with `--exclude`.
    #[arg(long, value_name = "PACKAGE")]
    include: Vec<String>,
    /// Collect coverage for the whole workspace except this package.
    /// Repeatable. Mutually exclusive with `--include`.
    #[arg(long, value_name = "PACKAGE")]
    exclude: Vec<String>,
    /// Directory where coverage output files should be written.
    /// Defaults to the current directory.
    #[arg(long, value_name = "DIR")]
    output_dir: Option<std::path::PathBuf>,
}

/// Wraps `cargo llvm-cov test`, which handles Soroban's coverage needs
/// correctly out of the box — soroban_sdk's own test harness runs contract
/// logic natively (not through the WASM VM), so ordinary LLVM
/// source-based coverage instrumentation applies without any
/// Soroban-specific flags. This command's job is knowing *that*, and
/// knowing the right output-format flags, not reimplementing coverage
/// collection.
///
/// Requires `cargo-llvm-cov` to be installed
/// (`cargo install cargo-llvm-cov`).
///
/// # Package filters
///
/// `--include <PACKAGE>` (repeatable) narrows collection to the named
/// workspace package(s), via cargo's own `-p`/`--package`. `--exclude
/// <PACKAGE>` (repeatable) collects for the whole workspace except the
/// named package(s), via `--workspace --exclude`. Specifying both is
/// rejected up front with an actionable error — cargo doesn't have a
/// sensible combined meaning for "these packages, but not these other
/// packages" as a single invocation, and guessing one would be worse
/// than asking the user to run the command twice.
pub fn run(args: CoverageArgs) -> Result<(), CliError> {
    let mut cmd = build_command(&args)?;

    let status = cmd.status().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => CliError(
            "cargo-llvm-cov is not installed or not found in PATH; \
                     install it with: cargo install cargo-llvm-cov"
                .to_string(),
        ),
        _ => CliError(format!(
            "failed to run `cargo llvm-cov` ({err}); is cargo-llvm-cov installed? \
                 try `cargo install cargo-llvm-cov`"
        )),
    })?;

    if !status.success() {
        return Err(CliError(format!(
            "coverage run failed (cargo llvm-cov exited with {status})"
        )));
    }

    Ok(())
}

/// Builds the `cargo llvm-cov` invocation for `args`, without running it —
/// split out from [`run`] so the flag assembly (including the
/// include/exclude package filters) is unit-testable via
/// [`Command::get_args`] instead of only through a full subprocess.
fn build_command(args: &CoverageArgs) -> Result<Command, CliError> {
    if !args.include.is_empty() && !args.exclude.is_empty() {
        return Err(CliError(
            "--include and --exclude cannot be combined; run coverage twice if you need both \
             a narrowed and a widened view"
                .to_string(),
        ));
    }

    let mut cmd = Command::new("cargo");
    cmd.arg("llvm-cov").arg("test");

    for package in &args.include {
        cmd.arg("--package").arg(package);
    }
    if !args.exclude.is_empty() {
        cmd.arg("--workspace");
        for package in &args.exclude {
            cmd.arg("--exclude").arg(package);
        }
    }

    match args.format {
        Format::Text => {}
        Format::Lcov => {
            cmd.arg("--lcov");
            let output_path = if let Some(dir) = &args.output_dir {
                dir.join("lcov.info")
            } else {
                std::path::PathBuf::from("lcov.info")
            };
            cmd.arg("--output-path").arg(output_path);
        }
        Format::Html => {
            cmd.arg("--html");
            if let Some(dir) = &args.output_dir {
                cmd.arg("--output-dir").arg(dir);
            }
        }
    }

    if args.open {
        if !matches!(args.format, Format::Html) {
            cmd.arg("--html");
        }
        cmd.arg("--open");
    }

    if let Some(pct) = args.fail_under {
        cmd.arg("--fail-under-lines").arg(pct.to_string());
    }

    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(include: &[&str], exclude: &[&str]) -> CoverageArgs {
        CoverageArgs {
            format: Format::Text,
            fail_under: None,
            open: false,
            include: include.iter().map(|s| s.to_string()).collect(),
            exclude: exclude.iter().map(|s| s.to_string()).collect(),
            output_dir: None,
        }
    }

    fn rendered_args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn no_filters_by_default() {
        let cmd = build_command(&args(&[], &[])).unwrap();
        let rendered = rendered_args(&cmd);
        assert!(!rendered.contains(&"--package".to_string()));
        assert!(!rendered.contains(&"--exclude".to_string()));
        assert!(!rendered.contains(&"--workspace".to_string()));
    }

    #[test]
    fn include_adds_a_package_flag_per_entry() {
        let cmd = build_command(&args(&["core", "money"], &[])).unwrap();
        let rendered = rendered_args(&cmd);
        assert_eq!(
            rendered,
            vec![
                "llvm-cov",
                "test",
                "--package",
                "core",
                "--package",
                "money",
            ]
        );
    }

    #[test]
    fn exclude_adds_workspace_and_an_exclude_flag_per_entry() {
        let cmd = build_command(&args(&[], &["cli"])).unwrap();
        let rendered = rendered_args(&cmd);
        assert_eq!(
            rendered,
            vec!["llvm-cov", "test", "--workspace", "--exclude", "cli"]
        );
    }

    #[test]
    fn exclude_with_multiple_packages() {
        let cmd = build_command(&args(&[], &["cli", "audit"])).unwrap();
        let rendered = rendered_args(&cmd);
        assert_eq!(
            rendered,
            vec![
                "llvm-cov",
                "test",
                "--workspace",
                "--exclude",
                "cli",
                "--exclude",
                "audit",
            ]
        );
    }

    #[test]
    fn include_and_exclude_together_is_rejected() {
        let err = build_command(&args(&["core"], &["cli"])).unwrap_err();
        assert!(err.0.contains("cannot be combined"), "{}", err.0);
    }

    #[test]
    fn filters_compose_with_format_and_fail_under() {
        let mut a = args(&["core"], &[]);
        a.format = Format::Lcov;
        a.fail_under = Some(90.0);
        let cmd = build_command(&a).unwrap();
        let rendered = rendered_args(&cmd);
        assert_eq!(
            rendered,
            vec![
                "llvm-cov",
                "test",
                "--package",
                "core",
                "--lcov",
                "--output-path",
                "lcov.info",
                "--fail-under-lines",
                "90",
            ]
        );
    }

    #[test]
    fn fail_under_accepts_the_whole_percentage_range() {
        for raw in ["0", "100", "95.5"] {
            assert_eq!(
                parse_fail_under(raw).unwrap(),
                raw.parse::<f64>().unwrap(),
                "--fail-under {raw} should be accepted"
            );
        }
    }

    #[test]
    fn fail_under_rejects_values_outside_the_range() {
        for raw in ["-1", "100.5", "1000", "nan", "not-a-number"] {
            let err = parse_fail_under(raw).expect_err("value should be rejected");
            assert!(
                err.contains("--fail-under") || err.contains("percentage"),
                "error should name the flag or the expected unit: {err}"
            );
        }
    }

    #[test]
    fn the_cli_itself_rejects_fail_under_outside_the_range() {
        use clap::{Command, FromArgMatches};

        // Exercise the real parser rather than the helper alone: the point of
        // the fix is that the flag cannot reach the coverage tool at all.
        let command = || CoverageArgs::augment_args(Command::new("coverage"));

        // `--flag=value` rather than `--flag value`: a bare `-1` is read by
        // clap as a flag of its own, so the value parser would never see it.
        for raw in ["-1", "150", "nan"] {
            let arg = format!("--fail-under={raw}");
            let err = command()
                .try_get_matches_from(["coverage", &arg])
                .expect_err("clap should reject the value");
            assert!(
                err.to_string().contains("between 0 and 100"),
                "unexpected error for {arg}: {err}"
            );
        }

        let matches = command()
            .try_get_matches_from(["coverage", "--fail-under", "90"])
            .expect("a valid percentage parses");
        let parsed = CoverageArgs::from_arg_matches(&matches).expect("matches deserialize");
        assert_eq!(parsed.fail_under, Some(90.0));
    }

    #[test]
    fn output_dir_with_lcov_format() {
        let mut a = args(&[], &[]);
        a.format = Format::Lcov;
        a.output_dir = Some(std::path::PathBuf::from("./coverage"));
        let cmd = build_command(&a).unwrap();
        let rendered = rendered_args(&cmd);
        assert_eq!(
            rendered,
            vec![
                "llvm-cov",
                "test",
                "--lcov",
                "--output-path",
                "./coverage/lcov.info",
            ]
        );
    }

    #[test]
    fn output_dir_with_html_format() {
        let mut a = args(&[], &[]);
        a.format = Format::Html;
        a.output_dir = Some(std::path::PathBuf::from("./coverage"));
        let cmd = build_command(&a).unwrap();
        let rendered = rendered_args(&cmd);
        assert_eq!(
            rendered,
            vec!["llvm-cov", "test", "--html", "--output-dir", "./coverage",]
        );
    }

    #[test]
    fn output_dir_with_spaces_in_path_lcov() {
        let mut a = args(&[], &[]);
        a.format = Format::Lcov;
        a.output_dir = Some(std::path::PathBuf::from("./my coverage dir"));
        let cmd = build_command(&a).unwrap();
        let rendered = rendered_args(&cmd);
        assert_eq!(
            rendered,
            vec![
                "llvm-cov",
                "test",
                "--lcov",
                "--output-path",
                "./my coverage dir/lcov.info",
            ]
        );
    }

    #[test]
    fn output_dir_with_spaces_in_path_html() {
        let mut a = args(&[], &[]);
        a.format = Format::Html;
        a.output_dir = Some(std::path::PathBuf::from("./my coverage results"));
        let cmd = build_command(&a).unwrap();
        let rendered = rendered_args(&cmd);
        assert_eq!(
            rendered,
            vec![
                "llvm-cov",
                "test",
                "--html",
                "--output-dir",
                "./my coverage results",
            ]
        );
    }
}
