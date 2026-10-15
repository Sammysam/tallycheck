//! `accounting-import`: one-time import from the old PostgreSQL system.
//!
//! ```text
//! accounting-import --source postgres://user@host:5432/accounting_db
//!                   --source-tz +05:30            (the old server's time zone offset)
//!                   --target <new empty data folder>
//!                   [--files <folder with the old stored files>]
//! ```
//! The PostgreSQL password is read from `PGPASSWORD` if it is not in the URL. The source is
//! only read. Exit code 0 means the reconciliation found no unexplained discrepancy.

use anyhow::{bail, Context};
use std::path::PathBuf;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut source = arg(&args, "--source").context("--source postgres://… is required")?;
    if !source.contains('@') || source.split('@').next().is_some_and(|u| !u.contains(':')) {
        if let Ok(pw) = std::env::var("PGPASSWORD") {
            if let Some(rest) = source.strip_prefix("postgres://") {
                if let Some((user, host)) = rest.split_once('@') {
                    source = format!("postgres://{user}:{pw}@{host}");
                }
            }
        }
    }
    let tz = arg(&args, "--source-tz").context(
        "--source-tz is required: the old server's UTC offset, e.g. +05:30 for Sri Lanka or +00:00 for UTC. \
         It cannot be guessed: timestamps were stored as wall-clock times in that zone",
    )?;
    let source_offset =
        importer::convert::parse_offset(&tz).context("--source-tz must look like +05:30, -03:00 or UTC")?;
    let target_dir = PathBuf::from(arg(&args, "--target").context("--target <new data folder> is required")?);
    let files_dir = arg(&args, "--files").map(PathBuf::from);
    if app_core::config::is_network_path(&target_dir) {
        bail!("the target folder must be on this PC's own disk");
    }
    std::fs::create_dir_all(&target_dir)?;

    let report =
        importer::run(&importer::Options { source, source_offset, target_dir: target_dir.clone(), files_dir }).await?;
    eprintln!("{}", importer::render_markdown(&report));
    eprintln!("Report written to {}", target_dir.join("import-report.md").display());
    if !report.success {
        bail!("the import is NOT reconciled: {} discrepancies", report.discrepancies.len());
    }
    Ok(())
}
