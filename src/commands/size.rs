use camino::Utf8Path;

use anyhow::Result;

/// Show the size of the .git directory.
pub fn do_size(project: &Utf8Path) -> Result<Option<String>> {
    let git_dir = project.join(".git");
    if !git_dir.is_dir() {
        return Ok(None);
    }
    let size = dir_size(git_dir.as_std_path())?;
    Ok(Some(format_size(size)))
}

/// Recursively sum file sizes under `path`, skipping symlinks so we don't
/// follow links out of the tree or into cycles. Works on plain `Path`s: only
/// sizes are read, so a non-UTF-8 entry name inside `.git` is no reason to
/// fail.
fn dir_size(path: &std::path::Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            total += dir_size(&entry.path())?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}

/// `bytes / unit` with one decimal (round half to even), by integer
/// arithmetic so no precision-losing `u64` -> `f64` cast is needed.
fn tenths(bytes: u64, unit: u64) -> String {
    let num = u128::from(bytes) * 10;
    let den = u128::from(unit);
    let mut q = num / den;
    let r = num % den;
    if r * 2 > den || (r * 2 == den && q % 2 == 1) {
        q += 1;
    }
    format!("{}.{}", q / 10, q % 10)
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{} GB", tenths(bytes, GB))
    } else if bytes >= MB {
        format!("{} MB", tenths(bytes, MB))
    } else if bytes >= KB {
        format!("{} KB", tenths(bytes, KB))
    } else {
        format!("{bytes} B")
    }
}
