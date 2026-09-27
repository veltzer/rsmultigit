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

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
