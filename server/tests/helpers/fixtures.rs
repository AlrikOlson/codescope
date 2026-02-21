//! Fixture project scaffolding utilities for integration tests.

use std::path::Path;

/// Recursively copy a directory tree. Preserves file contents but not metadata.
pub fn copy_dir_recursive(src: &Path, dst: &Path) {
    if !dst.exists() {
        std::fs::create_dir_all(dst).expect("Failed to create dir");
    }
    for entry in std::fs::read_dir(src).expect("Failed to read dir") {
        let entry = entry.expect("Failed to read entry");
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).expect("Failed to copy file");
        }
    }
}
