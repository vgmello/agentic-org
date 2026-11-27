//! `agentic-org logs`: bounded, symlink-safe, secret-redacted host and office log inspection.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::terminal::sanitize;
use serde::Serialize;

use super::paths::{normalize_lstat_path, resolve_instance, validate_plain_path, InstancePaths};
use super::redact::redact_lines;
use super::LogsArgs;
use crate::error::CliError;
use crate::selection;

pub(crate) const DEFAULT_LINES: usize = 200;
pub(crate) const MAX_LINES: usize = 2000;
const MAX_READ_BYTES_PER_FILE: u64 = 512 * 1024;
const MAX_TOTAL_READ_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4096;
const TRUNC_SUFFIX: &str = "… [truncated]";

/// Redacted log report emitted by `agentic-org logs --json` and embedded in `support`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LogReport {
    pub(crate) instance: String,
    pub(crate) lines: Vec<String>,
    pub(crate) truncated: bool,
    pub(crate) redacted_count: usize,
    pub(crate) sources: Vec<String>,
}

pub(crate) fn run(args: LogsArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    validate_lines(args.lines)?;
    if let Some(office) = args.office.as_deref() {
        validate_office_filter(office)?;
    }
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let paths = resolve_instance(target.instance())?;
    let report = collect_logs(&paths, args.office.as_deref(), args.lines)?;
    render_logs(&report, ctx)
}

pub(crate) fn validate_lines(lines: usize) -> Result<(), CliError> {
    if lines == 0 || lines > MAX_LINES {
        return Err(CliError::usage(format!(
            "--lines must be between 1 and {MAX_LINES} (got {lines})"
        )));
    }
    Ok(())
}

pub(super) fn validate_office_filter(office: &str) -> Result<(), CliError> {
    if office.is_empty()
        || !office.is_ascii()
        || office.contains(['/', '\\', '\0'])
        || office == "."
        || office == ".."
    {
        return Err(CliError::usage(format!(
            "invalid --office filter `{office}`"
        )));
    }
    Ok(())
}

/// Collects and redacts recent lines from host and office logs safely with strict bounds.
pub(crate) fn collect_logs(
    paths: &InstancePaths,
    office_filter: Option<&str>,
    max_lines: usize,
) -> Result<LogReport, CliError> {
    let files = discover_log_files(paths, office_filter)?;
    let mut all_lines = Vec::new();
    let mut sources = Vec::new();
    let mut total_bytes = 0usize;
    let mut total_redactions = 0usize;
    let mut truncated = false;
    let multi = files.len() > 1;

    for (label, path) in files {
        if total_bytes >= MAX_TOTAL_READ_BYTES {
            truncated = true;
            break;
        }
        let budget = (MAX_TOTAL_READ_BYTES - total_bytes) as u64;
        let per_file = budget.min(MAX_READ_BYTES_PER_FILE);
        let Some((raw_lines, bytes_read, file_trunc)) = read_tail_lines(&path, per_file)? else {
            continue;
        };
        total_bytes += bytes_read;
        truncated |= file_trunc;
        let (redacted, count) = redact_lines(&raw_lines);
        total_redactions += count;
        sources.push(label.clone());
        for line in redacted {
            let bounded = bound_line_utf8(&line, MAX_LINE_BYTES);
            if multi {
                all_lines.push(format!("[{label}] {bounded}"));
            } else {
                all_lines.push(bounded);
            }
        }
    }

    if all_lines.len() > max_lines {
        truncated = true;
        let drop_count = all_lines.len() - max_lines;
        all_lines.drain(..drop_count);
    }

    Ok(LogReport {
        instance: paths.name.clone(),
        lines: all_lines,
        truncated,
        redacted_count: total_redactions,
        sources,
    })
}

fn discover_log_files(
    paths: &InstancePaths,
    office_filter: Option<&str>,
) -> Result<Vec<(String, PathBuf)>, CliError> {
    let mut dirs = vec![paths.logs_dir.clone()];
    let alt = paths.instance_root.join("logs");
    if alt != paths.logs_dir {
        dirs.push(alt);
    }
    let mut seen_dirs = BTreeSet::new();
    let mut groups: Vec<(String, PathBuf)> = Vec::new();
    for dir in dirs {
        let norm = normalize_lstat_path(&dir);
        let Ok(meta) = fs::symlink_metadata(&norm) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let Ok(canon) = norm.canonicalize() else {
            continue;
        };
        if !seen_dirs.insert(canon) {
            continue;
        }
        collect_dir_logs(&norm, office_filter, &mut groups)?;
    }
    groups.sort_by(|a, b| log_sort_key(&a.0).cmp(&log_sort_key(&b.0)));
    Ok(groups)
}

fn collect_dir_logs(
    dir: &Path,
    office_filter: Option<&str>,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), CliError> {
    let entries = fs::read_dir(dir).map_err(|e| {
        CliError::failed(
            "logs::io",
            format!("listing log directory `{}`: {e}", dir.display()),
        )
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        validate_plain_path(&path)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_log_filename(&name) || !matches_office_filter(&name, office_filter) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(normalize_lstat_path(&path)) else {
            continue;
        };
        // Refuse symlinks and non-regular files (FIFOs, devices, sockets, dirs).
        if !meta.is_file() {
            continue;
        }
        out.push((name, path));
    }
    Ok(())
}

fn is_log_filename(name: &str) -> bool {
    name.ends_with(".log") || name.ends_with(".log.1")
}

pub(super) fn matches_office_filter(filename: &str, office_filter: Option<&str>) -> bool {
    let Some(filter) = office_filter else {
        return true;
    };
    let stem = filename
        .strip_suffix(".log.1")
        .or_else(|| filename.strip_suffix(".log"))
        .unwrap_or(filename);
    if stem == filter {
        return true;
    }
    if !filter.is_ascii() {
        return false;
    }
    let handle = if filter.len() >= 12 {
        &filter[filter.len() - 12..]
    } else {
        filter
    };
    stem.eq_ignore_ascii_case(handle)
}

/// Orders rotated `.log.1` before its active `.log` sibling, and sorts stems alphabetically.
fn log_sort_key(name: &str) -> (&str, u8) {
    if let Some(stem) = name.strip_suffix(".log.1") {
        (stem, 0)
    } else if let Some(stem) = name.strip_suffix(".log") {
        (stem, 1)
    } else {
        (name, 2)
    }
}

fn read_tail_lines(
    path: &Path,
    max_bytes: u64,
) -> Result<Option<(Vec<String>, usize, bool)>, CliError> {
    let norm = normalize_lstat_path(path);
    let mut file = match org_platform::fs::open_nofollow_ro(&norm) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(org_platform::fs::ELOOP) => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => return Ok(None),
        Err(e) => {
            return Err(CliError::failed(
                "logs::io",
                format!("opening log `{}`: {e}", path.display()),
            ))
        }
    };
    let meta = file.metadata().map_err(|e| {
        CliError::failed(
            "logs::io",
            format!("reading metadata `{}`: {e}", path.display()),
        )
    })?;
    if !meta.is_file() {
        return Ok(None);
    }
    let mut truncated = false;
    if meta.len() > max_bytes {
        truncated = true;
        let offset = meta.len() - max_bytes;
        file.seek(SeekFrom::Start(offset)).map_err(|e| {
            CliError::failed("logs::io", format!("seeking log `{}`: {e}", path.display()))
        })?;
    }
    let mut buf = Vec::new();
    file.take(max_bytes).read_to_end(&mut buf).map_err(|e| {
        CliError::failed("logs::io", format!("reading log `{}`: {e}", path.display()))
    })?;
    let bytes_read = buf.len();
    let slice = if truncated {
        match buf.iter().position(|&b| b == b'\n') {
            Some(pos) => &buf[pos + 1..],
            None => &buf[..],
        }
    } else {
        &buf[..]
    };
    let text = String::from_utf8_lossy(slice);
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.trim_end_matches('\r').to_owned())
        .collect();
    Ok(Some((lines, bytes_read, truncated)))
}

fn bound_line_utf8(line: &str, budget: usize) -> String {
    if line.len() <= budget {
        return line.to_owned();
    }
    if budget <= TRUNC_SUFFIX.len() {
        let cut = TRUNC_SUFFIX.floor_char_boundary(budget);
        return TRUNC_SUFFIX[..cut].to_owned();
    }
    let keep = line.floor_char_boundary(budget - TRUNC_SUFFIX.len());
    format!("{}{TRUNC_SUFFIX}", &line[..keep])
}

fn render_logs(report: &LogReport, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    if ctx.modes().json {
        return Ok(ctx.write_json(report)?);
    }
    if report.lines.is_empty() {
        if !ctx.modes().quiet {
            ctx.note(&format!(
                "No log lines found for instance `{}`.",
                report.instance
            ));
        }
        return Ok(());
    }
    let mut out = String::new();
    for line in &report.lines {
        out.push_str(&sanitize(line));
        out.push('\n');
    }
    ctx.out(&out)?;
    if report.truncated && !ctx.modes().quiet {
        ctx.note("Log output was truncated to fit line/byte bounds.");
    }
    Ok(())
}
