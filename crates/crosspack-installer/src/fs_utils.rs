use std::fs;
use std::io;
use std::path::Path;

pub fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}
