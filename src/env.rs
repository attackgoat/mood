use std::{env::current_exe, path::PathBuf};

/// Returns the directory of the current running executable, or an empty path if it cannot be found.
pub fn current_exe_dir() -> PathBuf {
    let res = current_exe()
        .ok()
        .map(|path| path.parent().map(|path| path.to_path_buf()))
        .flatten()
        .unwrap_or_else(PathBuf::new);

    // When running unit tests we need to fix the path in order to find the resource pak file
    #[cfg(test)]
    let res = res.parent().unwrap().to_path_buf();

    res
}
