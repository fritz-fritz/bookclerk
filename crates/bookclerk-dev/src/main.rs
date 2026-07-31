//! [`cargo` alias dispatcher](../README.md) for the external-plugin dev workflow.
//!
//! Scripts under `scripts/` remain the source of truth for CI; this binary wraps
//! them so `.cargo/config.toml` aliases can build, stage, and run in one command.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bookclerk-dev",
    about = "Bookclerk dev workflow (cargo alias target)"
)]
struct Cli {
    /// Use release profile for builds and `cargo run`.
    #[arg(long, global = true)]
    release: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build all first-party plugin guest binaries (`scripts/build-first-party-plugins.sh`).
    BuildPlugins,
    /// Build plugins and copy binaries + manifests to `target/plugin-artifacts`.
    StagePlugins {
        /// Staging directory (default: `$BOOKCLERK_PLUGIN_ARTIFACTS` or `target/plugin-artifacts`).
        #[arg(long, env = "BOOKCLERK_PLUGIN_ARTIFACTS")]
        dest: Option<PathBuf>,
    },
    /// Build + stage plugins, then `cargo run -p bookclerkd` with `BOOKCLERK_PLUGIN_DIRS` set.
    DevDaemon {
        /// Arguments forwarded to bookclerkd (after `--`).
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Build + stage plugins, then `cargo run -p bookclerk-cli` with `BOOKCLERK_PLUGIN_DIRS` set.
    DevCli {
        /// Arguments forwarded to bookclerk (after `--`).
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Build + stage plugins, then run the staged handshake integration test.
    TestStaged,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("bookclerk-dev: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = workspace_root()?;
    match cli.command {
        Commands::BuildPlugins => build_plugins(&root, cli.release),
        Commands::StagePlugins { dest } => stage_plugins(&root, cli.release, dest.as_deref()),
        Commands::DevDaemon { args } => dev_host(&root, cli.release, Host::Daemon, &args),
        Commands::DevCli { args } => dev_host(&root, cli.release, Host::Cli, &args),
        Commands::TestStaged => test_staged(&root, cli.release),
    }
}

#[derive(Clone, Copy)]
enum Host {
    Daemon,
    Cli,
}

fn workspace_root() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("bookclerk-dev manifest has no parent")?
        .parent()
        .context("bookclerk-dev is not under workspace crates/")?
        .to_path_buf())
}

fn profile(release: bool) -> &'static str {
    if release {
        "release"
    } else {
        "debug"
    }
}

fn default_artifacts(root: &Path) -> PathBuf {
    std::env::var_os("BOOKCLERK_PLUGIN_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target").join("plugin-artifacts"))
}

fn default_files_dir() -> PathBuf {
    std::env::var_os("BOOKCLERK_FILES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/BookclerkFiles"))
}

fn run_script(root: &Path, script: &str, args: &[&str]) -> Result<()> {
    let path = root.join("scripts").join(script);
    let status = Command::new(&path)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run {}", path.display()))?;
    if status.success() {
        Ok(())
    } else {
        bail!("{} exited with {status}", path.display());
    }
}

fn build_plugins(root: &Path, release: bool) -> Result<()> {
    run_script(root, "build-first-party-plugins.sh", &[profile(release)])
}

fn stage_plugins(root: &Path, release: bool, dest: Option<&Path>) -> Result<()> {
    build_plugins(root, release)?;
    let artifacts = dest
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_artifacts(root));
    run_script(
        root,
        "stage-first-party-plugins.sh",
        &[
            profile(release),
            artifacts.to_str().context("staging path is not UTF-8")?,
        ],
    )
}

fn build_jail(root: &Path, release: bool) -> Result<()> {
    let mut cmd = cargo(root);
    if release {
        cmd.arg("--release");
    }
    let status = cmd
        .args(["build", "-p", "bookclerk-jail"])
        .status()
        .context("cargo build -p bookclerk-jail")?;
    if status.success() {
        Ok(())
    } else {
        bail!("cargo build -p bookclerk-jail exited with {status}");
    }
}

fn cargo(root: &Path) -> Command {
    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.current_dir(root);
    cmd
}

fn dev_host(root: &Path, release: bool, host: Host, host_args: &[String]) -> Result<()> {
    stage_plugins(root, release, None)?;
    build_jail(root, release)?;

    let artifacts = default_artifacts(root);
    let files_dir = default_files_dir();
    let package = match host {
        Host::Daemon => "bookclerkd",
        Host::Cli => "bookclerk-cli",
    };

    let mut cmd = cargo(root);
    if release {
        cmd.arg("--release");
    }
    cmd.args(["run", "-p", package, "--"]);
    cmd.args(host_args);
    cmd.env("BOOKCLERK_FILES_DIR", &files_dir);
    cmd.env("BOOKCLERK_PLUGIN_DIRS", &artifacts);
    cmd.env("BOOKCLERK_PLUGIN_ARTIFACTS", &artifacts);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd
        .status()
        .with_context(|| format!("cargo run -p {package}"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("cargo run -p {package} exited with {status}");
    }
}

fn test_staged(root: &Path, release: bool) -> Result<()> {
    stage_plugins(root, release, None)?;
    build_jail(root, release)?;

    let artifacts = default_artifacts(root);
    let mut cmd = cargo(root);
    if release {
        cmd.arg("--release");
    }
    cmd.args([
        "test",
        "-p",
        "bookclerk-plugin-host",
        "--test",
        "staged_plugins",
    ]);
    cmd.env("BOOKCLERK_PLUGIN_ARTIFACTS", &artifacts);
    cmd.env(
        "BOOKCLERK_SANDBOX_REQUIRE_ENFORCEMENT",
        std::env::var_os("BOOKCLERK_SANDBOX_REQUIRE_ENFORCEMENT").unwrap_or_else(|| "1".into()),
    );
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd
        .status()
        .context("cargo test -p bookclerk-plugin-host --test staged_plugins")?;
    if status.success() {
        Ok(())
    } else {
        bail!("staged plugin test exited with {status}");
    }
}
