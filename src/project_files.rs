use std::{
    error::Error,
    path::{Path, PathBuf},
};

pub fn collect(root: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    let walk = ignore::WalkBuilder::new(root)
        .require_git(false)
        .ignore(false)
        .follow_links(false)
        .filter_entry(|entry| {
            entry.depth() == 0
                || (!entry.file_name().to_string_lossy().starts_with('.')
                    && !(entry.file_type().is_some_and(|kind| kind.is_dir())
                        && entry.path().join(".gdignore").is_file()))
        })
        .build();
    for entry in walk {
        let entry = entry?;
        if let Some(error) = entry.error() {
            return Err(error.clone().into());
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                extensions
                    .iter()
                    .any(|wanted| extension.eq_ignore_ascii_case(wanted))
            })
        {
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}
