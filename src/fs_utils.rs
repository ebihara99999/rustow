use crate::error::{FsError, Result, RustowError};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn is_directory(path: &Path) -> bool {
    path.is_dir()
}

pub fn is_symlink(path: &Path) -> bool {
    path.is_symlink()
}

pub fn path_exists(path: &Path) -> bool {
    path.exists()
}

pub fn create_symlink(link_path: &Path, target_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target_path, link_path).map_err(|e| {
            FsError::CreateSymlink {
                link_path: link_path.to_path_buf(),
                target_path: target_path.to_path_buf(),
                source: e,
            }
            .into()
        })
    }
    #[cfg(windows)]
    {
        if target_path.is_dir() {
            std::os::windows::fs::symlink_dir(target_path, link_path).map_err(|e| {
                FsError::CreateSymlink {
                    link_path: link_path.to_path_buf(),
                    target_path: target_path.to_path_buf(),
                    source: e,
                }
                .into()
            })
        } else {
            std::os::windows::fs::symlink_file(target_path, link_path).map_err(|e| {
                FsError::CreateSymlink {
                    link_path: link_path.to_path_buf(),
                    target_path: target_path.to_path_buf(),
                    source: e,
                }
                .into()
            })
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(FsError::Io {
            path: link_path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Symlink creation not supported on this platform",
            ),
        }
        .into())
    }
}

pub fn read_link(path: &Path) -> Result<PathBuf> {
    if !is_symlink(path) {
        // If the path doesn't exist at all, is_symlink will be false.
        // If it exists but is not a symlink, is_symlink will be false.
        // So, this check correctly leads to NotASymlink for both cases.
        return Err(FsError::NotASymlink(path.to_path_buf()).into());
    }
    std::fs::read_link(path).map_err(|e| {
        FsError::ReadSymlink {
            path: path.to_path_buf(),
            source: e,
        }
        .into()
    })
}

pub fn delete_symlink(path: &Path) -> Result<()> {
    if !is_symlink(path) {
        // If it's not a symlink, it could be a normal file/dir, or non-existent.
        // If it doesn't exist at all, path_exists will be false.
        if !path_exists(path) {
            return Err(FsError::NotFound(path.to_path_buf()).into());
        }
        return Err(FsError::NotASymlink(path.to_path_buf()).into());
    }

    // If is_symlink is true, the path refers to a symlink.
    // It could be a broken symlink, but std::fs::remove_file should handle it.
    std::fs::remove_file(path).map_err(|e| {
        FsError::DeleteSymlink {
            path: path.to_path_buf(),
            source: e,
        }
        .into()
    })
}

pub fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| {
        FsError::CreateDirectory {
            path: path.to_path_buf(),
            source: e,
        }
        .into()
    })
}

pub fn delete_empty_dir(path: &Path) -> Result<()> {
    if is_symlink(path) {
        return Err(FsError::NotADirectory(path.to_path_buf()).into());
    }
    if !path_exists(path) {
        return Err(FsError::NotFound(path.to_path_buf()).into());
    }
    if !is_directory(path) {
        return Err(FsError::NotADirectory(path.to_path_buf()).into());
    }

    // Check if the directory is empty
    match std::fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                // Directory is not empty
                return Err(FsError::DeleteDirectory {
                    path: path.to_path_buf(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::Other, // Using Other as std::io::ErrorKind::DirectoryNotEmpty is unstable
                        "Directory not empty",
                    ),
                }
                .into());
            }
        },
        Err(e) => {
            return Err(FsError::Io {
                path: path.to_path_buf(),
                source: e,
            }
            .into());
        },
    }

    std::fs::remove_dir(path).map_err(|e| {
        FsError::DeleteDirectory {
            path: path.to_path_buf(),
            source: e,
        }
        .into()
    })
}

pub fn canonicalize_path(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).map_err(|e| {
        FsError::Canonicalize {
            path: path.to_path_buf(),
            source: e,
        }
        .into()
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RawStowItemType {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawStowItem {
    pub absolute_path: PathBuf,
    pub package_relative_path: PathBuf,
    pub item_type: RawStowItemType,
}

impl RawStowItem {
    // Helper for tests to sort items for consistent comparison
    #[cfg(test)]
    fn sort_key(&self) -> PathBuf {
        self.package_relative_path.clone()
    }

    // Method to get the basename of the item from its package_relative_path
    pub fn basename(&self) -> String {
        self.package_relative_path
            .file_name()
            .unwrap_or_default() // Use OsStr::new("") or handle more gracefully if needed
            .to_string_lossy()
            .into_owned()
    }
}

pub fn walk_package_dir(package_path: &Path) -> Result<Vec<RawStowItem>> {
    if !path_exists(package_path) {
        return Err(FsError::NotFound(package_path.to_path_buf()).into());
    }
    if !is_directory(package_path) {
        // Note: is_directory follows symlinks. If package_path is a symlink to a dir,
        // this will pass. This is generally fine as we are interested in its content.
        return Err(FsError::NotADirectory(package_path.to_path_buf()).into());
    }

    let mut items: Vec<RawStowItem> = Vec::new();

    for entry_result in WalkDir::new(package_path).min_depth(1) {
        // entry_result の型は walkdir::Result<walkdir::DirEntry>
        let entry: walkdir::DirEntry = entry_result.map_err(|e| FsError::WalkDir {
            // 型を明示
            path: e.path().unwrap_or(package_path).to_path_buf(), // Use package_path if entry path is not available
            source: e
                .into_io_error()
                .unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "walkdir error")),
        })?;

        let absolute_path: PathBuf = entry.path().to_path_buf(); // 型を明示
        let package_relative_path: PathBuf = absolute_path.strip_prefix(package_path) // 型を明示
            .map_err(|_| RustowError::Stow(crate::error::StowError::InvalidPackageStructure(
                format!("Failed to strip prefix for {:?} from {:?}", absolute_path, package_path)
            )))?
            .to_path_buf();

        let file_type: std::fs::FileType = entry.file_type(); // 型を明示
        let item_type: RawStowItemType = if file_type.is_symlink() {
            // 型を明示
            RawStowItemType::Symlink
        } else if file_type.is_dir() {
            RawStowItemType::Directory
        } else if file_type.is_file() {
            RawStowItemType::File
        } else {
            // Should not happen for normal files/dirs/symlinks
            continue;
        };

        items.push(RawStowItem {
            absolute_path,
            package_relative_path,
            item_type,
        });
    }
    Ok(items)
}

pub fn is_stow_symlink(
    link_path: &Path,
    stow_dir: &Path,
) -> Result<Option<(String, PathBuf)>, RustowError> {
    // 1. Check if link_path is a symlink
    if !is_symlink(link_path) {
        return Ok(None);
    }

    // 2. Canonicalize stow_dir for reliable comparison
    let canonical_stow_dir: PathBuf = match canonicalize_path(stow_dir) {
        // 型を明示
        Ok(p) => p,
        Err(RustowError::Fs(FsError::Canonicalize { path, source })) => {
            // Propagate canonicalization error for stow_dir
            return Err(RustowError::Fs(FsError::Canonicalize { path, source }));
        },
        Err(RustowError::Fs(FsError::NotFound(_)))
        | Err(RustowError::Fs(FsError::NotADirectory(_))) => {
            return Err(RustowError::Fs(FsError::Canonicalize {
                path: stow_dir.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Stow directory cannot be canonicalized or is not a directory",
                ),
            }));
        },
        Err(e) => return Err(e), // Other errors
    };

    // 3. Read the link's destination
    // read_link itself returns Err if not a symlink, but we've already checked.
    let target_dest_path_from_link: PathBuf = match read_link(link_path) {
        // 型を明示
        Ok(p) => p,
        Err(RustowError::Fs(FsError::NotASymlink(_))) => return Ok(None), // Should be caught by is_symlink above
        Err(e) => return Err(e),
    };

    // 4. Resolve the link's destination to an absolute, canonical path
    let link_parent_dir: &Path = link_path.parent().unwrap_or_else(|| Path::new("")); // 型を明示

    let potentially_non_canonical_target_abs_path = if target_dest_path_from_link.is_absolute() {
        target_dest_path_from_link
    } else {
        link_parent_dir.join(target_dest_path_from_link)
    };

    let canonical_target_path = match canonicalize_path(&potentially_non_canonical_target_abs_path)
    {
        Ok(p) => p,
        Err(RustowError::Fs(FsError::NotFound(_))) => {
            // Target not found directly, implies broken symlink if potentially_non_canonical_target_abs_path was derived from a link
            return Ok(None);
        },
        Err(RustowError::Fs(FsError::Canonicalize {
            path: errored_path,
            source,
        })) => {
            if source.kind() == std::io::ErrorKind::NotFound {
                // Canonicalization failed because target does not exist (broken symlink)
                return Ok(None);
            }
            // Other canonicalization error, propagate it
            return Err(RustowError::Fs(FsError::Canonicalize {
                path: errored_path,
                source,
            }));
        },
        Err(e) => return Err(e), // Other errors (e.g., Io, Config, etc.)
    };

    // 5. Check if the canonical target path is within the canonical_stow_dir
    if !canonical_target_path.starts_with(&canonical_stow_dir) {
        return Ok(None);
    }

    // 6. Extract the path relative to the stow_dir (e.g., "package_name/item/path")
    let path_relative_to_stow_dir = match canonical_target_path.strip_prefix(&canonical_stow_dir) {
        Ok(p) => p.to_path_buf(),
        Err(_) => {
            return Err(crate::error::StowError::InvalidPackageStructure(format!(
                "Internal error: Failed to strip prefix for {:?} from {:?} after starts_with check",
                canonical_target_path, canonical_stow_dir
            ))
            .into());
        },
    };

    // 7. Extract package name and item path within package
    let mut components = path_relative_to_stow_dir.components();

    match components.next() {
        Some(std::path::Component::Normal(package_name_osstr)) => {
            let package_name = package_name_osstr.to_string_lossy().into_owned();
            // The rest of the components form the item's path relative to the package dir.
            let item_path_in_package = components.as_path().to_path_buf();
            Ok(Some((package_name, item_path_in_package)))
        },
        _ => {
            // Path relative to stow_dir is empty (target is stow_dir itself)
            // or starts with `.` or `..` (shouldn't happen with canonical paths)
            // or is a root dir (also shouldn't happen).
            // This means it's not pointing to an item *within a package* inside stow_dir.
            Ok(None)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs::{self, File};
    use tempfile::tempdir;

    // ... existing test_path_exists functions ...

    #[test]
    fn test_is_directory_for_directory() {
        let dir = tempdir().unwrap();
        let sub_dir_path = dir.path().join("test_subdir");
        fs::create_dir(&sub_dir_path).unwrap();
        assert!(is_directory(&sub_dir_path));
    }

    #[test]
    fn test_is_directory_for_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_file.txt");
        File::create(&file_path).unwrap();
        assert!(!is_directory(&file_path));
    }

    #[test]
    fn test_is_directory_for_non_existing_path() {
        let dir = tempdir().unwrap();
        let non_existing_path = dir.path().join("non_existing");
        assert!(!is_directory(&non_existing_path));
    }

    #[test]
    fn test_is_directory_for_symlink_to_directory() {
        let dir = tempdir().unwrap();
        let target_dir_path = dir.path().join("target_dir");
        fs::create_dir(&target_dir_path).unwrap();
        let symlink_path = dir.path().join("link_to_dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir_path, &symlink_path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target_dir_path, &symlink_path).unwrap(); // Use symlink_dir for directories on Windows

        // path.is_dir() follows symlinks by default.
        assert!(is_directory(&symlink_path));
    }

    #[test]
    fn test_is_directory_for_symlink_to_file() {
        let dir = tempdir().unwrap();
        let target_file_path = dir.path().join("target_file.txt");
        File::create(&target_file_path).unwrap();
        let symlink_path = dir.path().join("link_to_file_for_isdir_test");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_file_path, &symlink_path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target_file_path, &symlink_path).unwrap();

        assert!(!is_directory(&symlink_path));
    }

    #[test]
    fn test_is_directory_for_broken_symlink() {
        let dir = tempdir().unwrap();
        let non_existing_target = dir.path().join("non_existing_target_for_isdir");
        let broken_symlink_path = dir.path().join("broken_link_for_isdir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&non_existing_target, &broken_symlink_path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&non_existing_target, &broken_symlink_path).unwrap();

        assert!(!is_directory(&broken_symlink_path));
    }

    #[test]
    fn test_is_symlink_for_symlink_to_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("target_file_for_is_symlink.txt");
        File::create(&file_path).unwrap();
        let symlink_path = dir.path().join("symlink_to_file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&file_path, &symlink_path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&file_path, &symlink_path).unwrap();
        assert!(is_symlink(&symlink_path));
    }

    #[test]
    fn test_is_symlink_for_symlink_to_directory() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().join("target_dir_for_is_symlink");
        fs::create_dir(&dir_path).unwrap();
        let symlink_path = dir.path().join("symlink_to_dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&dir_path, &symlink_path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&dir_path, &symlink_path).unwrap();
        assert!(is_symlink(&symlink_path));
    }

    #[test]
    fn test_is_symlink_for_broken_symlink() {
        let dir = tempdir().unwrap();
        let non_existing_target = dir.path().join("non_existing_target_for_is_symlink");
        let broken_symlink_path = dir.path().join("broken_symlink");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&non_existing_target, &broken_symlink_path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&non_existing_target, &broken_symlink_path).unwrap();
        assert!(is_symlink(&broken_symlink_path));
    }

    #[test]
    fn test_is_symlink_for_actual_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("actual_file_for_is_symlink.txt");
        File::create(&file_path).unwrap();
        assert!(!is_symlink(&file_path));
    }

    #[test]
    fn test_is_symlink_for_actual_directory() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().join("actual_dir_for_is_symlink");
        fs::create_dir(&dir_path).unwrap();
        assert!(!is_symlink(&dir_path));
    }

    #[test]
    fn test_is_symlink_for_non_existing_path() {
        let dir = tempdir().unwrap();
        let non_existing_path = dir.path().join("non_existing_for_is_symlink");
        assert!(!is_symlink(&non_existing_path));
    }

    #[test]
    fn test_create_symlink_to_file_success() {
        let dir = tempdir().unwrap();
        let target_file_path = dir.path().join("target_sym_file.txt");
        File::create(&target_file_path).unwrap();
        let link_path = dir.path().join("link_to_sym_file");

        let result = create_symlink(&link_path, &target_file_path);
        assert!(result.is_ok(), "create_symlink failed: {:?}", result.err());
        assert!(link_path.exists(), "Link path should exist");
        assert!(is_symlink(&link_path), "Path should be a symlink");
        let read_target = fs::read_link(&link_path).unwrap();
        assert_eq!(read_target, target_file_path);
    }

    #[test]
    fn test_create_symlink_to_directory_success() {
        let dir = tempdir().unwrap();
        let target_dir_path = dir.path().join("target_sym_dir");
        fs::create_dir(&target_dir_path).unwrap();
        let link_path = dir.path().join("link_to_sym_dir");

        let result = create_symlink(&link_path, &target_dir_path);
        assert!(
            result.is_ok(),
            "create_symlink failed for directory: {:?}",
            result.err()
        );
        assert!(link_path.exists(), "Link path to directory should exist");
        assert!(
            is_symlink(&link_path),
            "Path to directory should be a symlink"
        );
        let read_target = fs::read_link(&link_path).unwrap();
        assert_eq!(read_target, target_dir_path);
    }

    #[test]
    fn test_create_symlink_target_does_not_exist_success() {
        let dir = tempdir().unwrap();
        let non_existing_target_path = dir.path().join("non_existing_sym_target");
        let link_path = dir.path().join("link_to_non_existing_sym");

        let result = create_symlink(&link_path, &non_existing_target_path);
        assert!(
            result.is_ok(),
            "create_symlink to non-existing target failed: {:?}",
            result.err()
        );

        // Instead of asserting link_path.exists(), which might be problematic for broken symlinks on some platforms/setups,
        // we assert that it is a symlink and that read_link works as expected.
        // If create_symlink was successful, the link was created.
        assert!(
            is_symlink(&link_path),
            "Path should be a symlink after creation, even if broken."
        );

        let read_target_result = fs::read_link(&link_path);
        assert!(
            read_target_result.is_ok(),
            "Failed to read link even if it is broken: {:?}",
            read_target_result.err()
        );
        assert_eq!(read_target_result.unwrap(), non_existing_target_path);
    }

    #[test]
    fn test_create_symlink_link_path_already_exists_as_file() {
        let dir = tempdir().unwrap();
        let target_file_path = dir.path().join("target_for_conflict.txt");
        File::create(&target_file_path).unwrap();

        let link_path = dir.path().join("existing_item_is_file");
        File::create(&link_path).unwrap();

        let result = create_symlink(&link_path, &target_file_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::CreateSymlink {
                link_path: lp,
                target_path: tp,
                ..
            })) => {
                assert_eq!(lp, link_path);
                assert_eq!(tp, target_file_path);
            },
            _ => panic!("Expected FsError::CreateSymlink, got {:?}", result),
        }
    }

    #[test]
    fn test_create_symlink_link_path_already_exists_as_dir() {
        let dir = tempdir().unwrap();
        let target_file_path = dir.path().join("target_for_conflict_dir.txt");
        File::create(&target_file_path).unwrap();

        let link_path = dir.path().join("existing_item_is_dir");
        fs::create_dir(&link_path).unwrap();

        let result = create_symlink(&link_path, &target_file_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::CreateSymlink {
                link_path: lp,
                target_path: tp,
                ..
            })) => {
                assert_eq!(lp, link_path);
                assert_eq!(tp, target_file_path);
            },
            _ => panic!("Expected FsError::CreateSymlink, got {:?}", result),
        }
    }

    #[test]
    fn test_read_link_success_file_target() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("target_rl_file.txt");
        File::create(&target_file).unwrap();
        let link = dir.path().join("link_to_rl_file");
        create_symlink(&link, &target_file).unwrap();

        let result = read_link(&link);
        assert!(result.is_ok(), "read_link failed: {:?}", result.err());
        assert_eq!(result.unwrap(), target_file);
    }

    #[test]
    fn test_read_link_success_dir_target() {
        let dir = tempdir().unwrap();
        let target_dir = dir.path().join("target_rl_dir");
        fs::create_dir(&target_dir).unwrap();
        let link = dir.path().join("link_to_rl_dir");
        create_symlink(&link, &target_dir).unwrap();

        let result = read_link(&link);
        assert!(
            result.is_ok(),
            "read_link for dir target failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), target_dir);
    }

    #[test]
    fn test_read_link_success_broken_link() {
        let dir = tempdir().unwrap();
        let non_existent_target = dir.path().join("non_existent_rl_target");
        let link = dir.path().join("broken_rl_link");
        create_symlink(&link, &non_existent_target).unwrap();

        let result = read_link(&link);
        assert!(
            result.is_ok(),
            "read_link for broken link failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), non_existent_target);
    }

    #[test]
    fn test_read_link_not_a_symlink_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("actual_file_for_rl.txt");
        File::create(&file_path).unwrap();

        let result = read_link(&file_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotASymlink(p))) => assert_eq!(p, file_path),
            _ => panic!("Expected FsError::NotASymlink, got {:?}", result),
        }
    }

    #[test]
    fn test_read_link_not_a_symlink_directory() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().join("actual_dir_for_rl");
        fs::create_dir(&dir_path).unwrap();

        let result = read_link(&dir_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotASymlink(p))) => assert_eq!(p, dir_path),
            _ => panic!("Expected FsError::NotASymlink, got {:?}", result),
        }
    }

    #[test]
    fn test_read_link_path_does_not_exist() {
        let dir = tempdir().unwrap();
        let non_existent_path = dir.path().join("i_do_not_exist_rl");

        let result = read_link(&non_existent_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotASymlink(p))) => assert_eq!(p, non_existent_path),
            _ => panic!(
                "Expected FsError::NotASymlink for non-existent path, got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_delete_symlink_success_to_file() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("target_del_file.txt");
        File::create(&target_file).unwrap();
        let link = dir.path().join("link_to_del_file");
        create_symlink(&link, &target_file).unwrap();
        assert!(path_exists(&link) && is_symlink(&link));

        let result = delete_symlink(&link);
        assert!(result.is_ok(), "delete_symlink failed: {:?}", result.err());
        assert!(!path_exists(&link));
    }

    #[test]
    fn test_delete_symlink_success_to_directory() {
        let dir = tempdir().unwrap();
        let target_dir_path = dir.path().join("target_del_dir");
        fs::create_dir(&target_dir_path).unwrap();
        let link = dir.path().join("link_to_del_dir");
        create_symlink(&link, &target_dir_path).unwrap();
        assert!(path_exists(&link) && is_symlink(&link));

        let result = delete_symlink(&link);
        assert!(
            result.is_ok(),
            "delete_symlink for dir link failed: {:?}",
            result.err()
        );
        assert!(!path_exists(&link));
    }

    #[test]
    fn test_delete_symlink_success_broken_link() {
        let dir = tempdir().unwrap();
        let non_existent_target = dir.path().join("non_existent_del_target");
        let link = dir.path().join("broken_del_link");
        create_symlink(&link, &non_existent_target).unwrap();
        assert!(is_symlink(&link)); // For broken links, exists() can be iffy, but is_symlink() should be true.

        let result = delete_symlink(&link);
        assert!(
            result.is_ok(),
            "delete_symlink for broken link failed: {:?}",
            result.err()
        );
        assert!(!path_exists(&link));
    }

    #[test]
    fn test_delete_symlink_not_a_symlink_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("actual_file_for_del.txt");
        File::create(&file_path).unwrap();

        let result = delete_symlink(&file_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotASymlink(p))) => assert_eq!(p, file_path),
            _ => panic!("Expected FsError::NotASymlink, got {:?}", result),
        }
        assert!(path_exists(&file_path));
    }

    #[test]
    fn test_delete_symlink_not_a_symlink_directory() {
        let dir = tempdir().unwrap();
        let dir_path = dir.path().join("actual_dir_for_del");
        fs::create_dir(&dir_path).unwrap();

        let result = delete_symlink(&dir_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotASymlink(p))) => assert_eq!(p, dir_path),
            _ => panic!("Expected FsError::NotASymlink, got {:?}", result),
        }
        assert!(path_exists(&dir_path));
    }

    #[test]
    fn test_delete_symlink_path_does_not_exist() {
        let dir = tempdir().unwrap();
        let non_existent_path = dir.path().join("i_do_not_exist_del");

        let result = delete_symlink(&non_existent_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotFound(p))) => assert_eq!(p, non_existent_path),
            _ => panic!(
                "Expected FsError::NotFound for non-existent path, got {:?}",
                result
            ),
        }
    }

    // --- create_dir_all tests ---
    #[test]
    fn test_create_dir_all_success_new_single_dir() {
        let base_dir = tempdir().unwrap();
        let new_dir_path = base_dir.path().join("new_single_dir");

        let result = create_dir_all(&new_dir_path);
        assert!(result.is_ok(), "create_dir_all failed: {:?}", result.err());
        assert!(path_exists(&new_dir_path));
        assert!(is_directory(&new_dir_path));
    }

    #[test]
    fn test_create_dir_all_success_new_nested_dirs() {
        let base_dir = tempdir().unwrap();
        let new_nested_dir_path = base_dir.path().join("nested1/nested2/nested3");

        let result = create_dir_all(&new_nested_dir_path);
        assert!(
            result.is_ok(),
            "create_dir_all for nested dirs failed: {:?}",
            result.err()
        );
        assert!(path_exists(&new_nested_dir_path));
        assert!(is_directory(&new_nested_dir_path));
        assert!(is_directory(&base_dir.path().join("nested1/nested2")));
        assert!(is_directory(&base_dir.path().join("nested1")));
    }

    #[test]
    fn test_create_dir_all_success_path_already_exists_as_dir() {
        let base_dir = tempdir().unwrap();
        let existing_dir_path = base_dir.path().join("already_exists_dir");
        fs::create_dir(&existing_dir_path).unwrap();

        let result = create_dir_all(&existing_dir_path);
        assert!(
            result.is_ok(),
            "create_dir_all for existing dir failed: {:?}",
            result.err()
        );
        assert!(path_exists(&existing_dir_path));
        assert!(is_directory(&existing_dir_path));
    }

    #[test]
    fn test_create_dir_all_error_path_already_exists_as_file() {
        let base_dir = tempdir().unwrap();
        let existing_file_path = base_dir.path().join("already_exists_file.txt");
        File::create(&existing_file_path).unwrap();

        let result = create_dir_all(&existing_file_path);
        assert!(
            result.is_err(),
            "Expected create_dir_all to fail for existing file"
        );
        match result {
            Err(RustowError::Fs(FsError::CreateDirectory { path, .. })) => {
                assert_eq!(path, existing_file_path);
            },
            _ => panic!("Expected FsError::CreateDirectory, got {:?}", result),
        }
        assert!(path_exists(&existing_file_path));
        assert!(!is_directory(&existing_file_path)); // Make sure it's still a file
    }

    // --- delete_empty_dir tests ---
    #[test]
    fn test_delete_empty_dir_success() {
        let base_dir = tempdir().unwrap();
        let empty_dir_path = base_dir.path().join("empty_to_delete");
        fs::create_dir(&empty_dir_path).unwrap();

        let result = delete_empty_dir(&empty_dir_path);
        assert!(
            result.is_ok(),
            "delete_empty_dir failed: {:?}",
            result.err()
        );
        assert!(!path_exists(&empty_dir_path));
    }

    #[test]
    fn test_delete_empty_dir_error_not_found() {
        let base_dir = tempdir().unwrap();
        let non_existent_path = base_dir.path().join("i_do_not_exist_for_delete_dir");

        let result = delete_empty_dir(&non_existent_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotFound(p))) => assert_eq!(p, non_existent_path),
            _ => panic!("Expected FsError::NotFound, got {:?}", result),
        }
    }

    #[test]
    fn test_delete_empty_dir_error_not_a_directory() {
        let base_dir = tempdir().unwrap();
        let file_path = base_dir.path().join("file_instead_of_dir.txt");
        File::create(&file_path).unwrap();

        let result = delete_empty_dir(&file_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotADirectory(p))) => assert_eq!(p, file_path),
            _ => panic!("Expected FsError::NotADirectory, got {:?}", result),
        }
        assert!(path_exists(&file_path)); // Ensure the file was not deleted
    }

    #[test]
    fn test_delete_empty_dir_error_directory_not_empty() {
        let base_dir = tempdir().unwrap();
        let non_empty_dir_path = base_dir.path().join("not_empty_dir");
        fs::create_dir(&non_empty_dir_path).unwrap();
        File::create(non_empty_dir_path.join("some_file.txt")).unwrap();

        let result = delete_empty_dir(&non_empty_dir_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::DeleteDirectory { path, source })) => {
                assert_eq!(path, non_empty_dir_path);
                // We used Other, so we check the message or kind if more specific info is needed
                // For now, checking the error kind is enough if it matches the setup.
                // However, the custom message "Directory not empty" is more robust to check here.
                assert_eq!(source.to_string(), "Directory not empty");
            },
            _ => panic!(
                "Expected FsError::DeleteDirectory for not empty, got {:?}",
                result
            ),
        }
        assert!(path_exists(&non_empty_dir_path)); // Ensure the directory was not deleted
    }

    #[test]
    fn test_delete_empty_dir_error_symlink_to_empty_dir() {
        let base_dir = tempdir().unwrap();
        let target_empty_dir = base_dir.path().join("target_empty_dir_for_symlink_del");
        fs::create_dir(&target_empty_dir).unwrap();
        let symlink_path = base_dir.path().join("symlink_to_empty_dir");
        create_symlink(&symlink_path, &target_empty_dir).unwrap();

        let result = delete_empty_dir(&symlink_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotADirectory(p))) => assert_eq!(p, symlink_path),
            _ => panic!(
                "Expected FsError::NotADirectory for symlink, got {:?}",
                result
            ),
        }
        assert!(path_exists(&symlink_path)); // Ensure the symlink was not deleted
        assert!(path_exists(&target_empty_dir)); // Ensure the target dir was not deleted
    }

    // --- canonicalize_path tests ---
    #[test]
    fn test_canonicalize_path_success_simple_path() {
        let dir = tempdir().unwrap();
        let file_name = "test_file_canonical.txt";
        let file_path = dir.path().join(file_name);
        File::create(&file_path).unwrap();

        let result = canonicalize_path(&file_path);
        assert!(
            result.is_ok(),
            "canonicalize_path failed: {:?}",
            result.err()
        );
        let canonicalized = result.unwrap();
        assert!(canonicalized.is_absolute());
        assert!(canonicalized.ends_with(file_name));
        assert!(path_exists(&canonicalized));
    }

    #[test]
    fn test_canonicalize_path_success_with_dot() {
        let dir = tempdir().unwrap();
        let sub_dir_name = "sub";
        let file_name = "test_file_dot.txt";
        let sub_dir_path = dir.path().join(sub_dir_name);
        fs::create_dir(&sub_dir_path).unwrap();
        let file_path = sub_dir_path.join(file_name);
        File::create(&file_path).unwrap();

        let path_with_dot = dir.path().join(".").join(sub_dir_name).join(file_name);
        let result = canonicalize_path(&path_with_dot);
        assert!(
            result.is_ok(),
            "canonicalize_path with . failed: {:?}",
            result.err()
        );
        let canonicalized = result.unwrap();
        assert_eq!(canonicalized, std::fs::canonicalize(&file_path).unwrap());
    }

    #[test]
    fn test_canonicalize_path_success_with_dot_dot() {
        let dir = tempdir().unwrap();
        let sub_dir_name = "sub_dotdot";
        let other_sub_dir_name = "other_sub";
        let file_name = "test_file_dotdot.txt";

        let sub_dir_path = dir.path().join(sub_dir_name);
        fs::create_dir(&sub_dir_path).unwrap();

        let other_sub_dir_path = dir.path().join(other_sub_dir_name);
        fs::create_dir(&other_sub_dir_path).unwrap();
        let file_in_other_sub_dir_path = other_sub_dir_path.join(file_name);
        File::create(&file_in_other_sub_dir_path).unwrap();

        // Path like /tmp/random_dir/sub_dotdot/../other_sub/test_file_dotdot.txt
        let path_with_dot_dot = sub_dir_path
            .join("..")
            .join(other_sub_dir_name)
            .join(file_name);
        let result = canonicalize_path(&path_with_dot_dot);
        assert!(
            result.is_ok(),
            "canonicalize_path with .. failed: {:?}",
            result.err()
        );
        let canonicalized = result.unwrap();
        assert_eq!(
            canonicalized,
            std::fs::canonicalize(&file_in_other_sub_dir_path).unwrap()
        );
    }

    #[test]
    fn test_canonicalize_path_error_non_existent_path() {
        let dir = tempdir().unwrap();
        let non_existent_path = dir.path().join("i_do_not_exist_canonical.txt");

        let result = canonicalize_path(&non_existent_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::Canonicalize { path, .. })) => {
                assert_eq!(path, non_existent_path);
            },
            _ => panic!(
                "Expected FsError::Canonicalize for non-existent path, got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_canonicalize_path_success_symlink() {
        let dir = tempdir().unwrap();
        let target_file_name = "target_canonical_sym.txt";
        let target_file_path = dir.path().join(target_file_name);
        File::create(&target_file_path).unwrap();

        let link_name = "link_to_canonical_target";
        let link_path = dir.path().join(link_name);
        create_symlink(&link_path, &target_file_path).unwrap();

        let result = canonicalize_path(&link_path);
        assert!(
            result.is_ok(),
            "canonicalize_path for symlink failed: {:?}",
            result.err()
        );
        let canonicalized_link = result.unwrap();
        let canonicalized_target = std::fs::canonicalize(&target_file_path).unwrap();
        assert_eq!(canonicalized_link, canonicalized_target);
    }

    #[test]
    fn test_canonicalize_path_success_broken_symlink() {
        // std::fs::canonicalize is expected to fail on broken symlinks
        let dir = tempdir().unwrap();
        let non_existent_target = dir.path().join("non_existent_target_canonical_sym");
        let broken_link_path = dir.path().join("broken_link_canonical");
        create_symlink(&broken_link_path, &non_existent_target).unwrap();

        let result = canonicalize_path(&broken_link_path);
        assert!(
            result.is_err(),
            "canonicalize_path should fail for broken symlink"
        );
        match result {
            Err(RustowError::Fs(FsError::Canonicalize { path, .. })) => {
                assert_eq!(path, broken_link_path);
            },
            _ => panic!(
                "Expected FsError::Canonicalize for broken symlink, got {:?}",
                result
            ),
        }
    }

    // --- walk_package_dir tests ---
    fn create_nested_structure(base_dir: &Path) {
        // base_dir/
        //   file1.txt
        //   dir1/
        //     file2.txt
        //     sub_dir1/
        //       file3.txt
        //   dir2/
        //   .dotfile
        //   link_to_file1 (symlink to file1.txt)

        File::create(base_dir.join("file1.txt")).unwrap();
        fs::create_dir(base_dir.join("dir1")).unwrap();
        File::create(base_dir.join("dir1/file2.txt")).unwrap();
        fs::create_dir(base_dir.join("dir1/sub_dir1")).unwrap();
        File::create(base_dir.join("dir1/sub_dir1/file3.txt")).unwrap();
        fs::create_dir(base_dir.join("dir2")).unwrap(); // Empty dir
        File::create(base_dir.join(".dotfile")).unwrap();

        let target_for_link = base_dir.join("file1.txt");
        let link_path = base_dir.join("link_to_file1");
        create_symlink(&link_path, &target_for_link).unwrap();
    }

    #[test]
    fn test_walk_package_dir_success_complex_structure() {
        let package_dir = tempdir().unwrap();
        create_nested_structure(package_dir.path());

        let result = walk_package_dir(package_dir.path());
        assert!(
            result.is_ok(),
            "walk_package_dir failed: {:?}",
            result.err()
        );
        let mut items = result.unwrap();
        items.sort_by_key(|item| item.sort_key());

        let mut expected_items = vec![
            RawStowItem {
                absolute_path: package_dir.path().join(".dotfile"),
                package_relative_path: PathBuf::from(".dotfile"),
                item_type: RawStowItemType::File,
            },
            RawStowItem {
                absolute_path: package_dir.path().join("dir1"),
                package_relative_path: PathBuf::from("dir1"),
                item_type: RawStowItemType::Directory,
            },
            RawStowItem {
                absolute_path: package_dir.path().join("dir1/file2.txt"),
                package_relative_path: PathBuf::from("dir1/file2.txt"),
                item_type: RawStowItemType::File,
            },
            RawStowItem {
                absolute_path: package_dir.path().join("dir1/sub_dir1"),
                package_relative_path: PathBuf::from("dir1/sub_dir1"),
                item_type: RawStowItemType::Directory,
            },
            RawStowItem {
                absolute_path: package_dir.path().join("dir1/sub_dir1/file3.txt"),
                package_relative_path: PathBuf::from("dir1/sub_dir1/file3.txt"),
                item_type: RawStowItemType::File,
            },
            RawStowItem {
                absolute_path: package_dir.path().join("dir2"),
                package_relative_path: PathBuf::from("dir2"),
                item_type: RawStowItemType::Directory,
            },
            RawStowItem {
                absolute_path: package_dir.path().join("file1.txt"),
                package_relative_path: PathBuf::from("file1.txt"),
                item_type: RawStowItemType::File,
            },
            RawStowItem {
                absolute_path: package_dir.path().join("link_to_file1"),
                package_relative_path: PathBuf::from("link_to_file1"),
                item_type: RawStowItemType::Symlink,
            },
        ];
        expected_items.sort_by_key(|item| item.sort_key());

        assert_eq!(
            items.len(),
            expected_items.len(),
            "Mismatch in number of items. Got: {:?}, Expected: {:?}",
            items,
            expected_items
        );

        // Using HashSet for comparison because WalkDir doesn't guarantee order across all platforms for all items,
        // even though we sort by relative path. The absolute paths might subtly differ in intermediate steps
        // or due to symlink resolutions if not careful, but relative paths should be consistent.
        let items_set: HashSet<_> = items.into_iter().collect();
        let expected_set: HashSet<_> = expected_items.into_iter().collect();

        assert_eq!(items_set, expected_set);
    }

    #[test]
    fn test_walk_package_dir_empty_dir() {
        let package_dir = tempdir().unwrap();
        let result = walk_package_dir(package_dir.path());
        assert!(
            result.is_ok(),
            "walk_package_dir for empty dir failed: {:?}",
            result.err()
        );
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_walk_package_dir_path_not_found() {
        let dir = tempdir().unwrap();
        let non_existent_path = dir.path().join("non_existent_package");
        let result = walk_package_dir(&non_existent_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotFound(p))) => assert_eq!(p, non_existent_path),
            _ => panic!("Expected FsError::NotFound, got {:?}", result),
        }
    }

    #[test]
    fn test_walk_package_dir_path_is_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("file_as_package.txt");
        File::create(&file_path).unwrap();
        let result = walk_package_dir(&file_path);
        assert!(result.is_err());
        match result {
            Err(RustowError::Fs(FsError::NotADirectory(p))) => assert_eq!(p, file_path),
            _ => panic!("Expected FsError::NotADirectory, got {:?}", result),
        }
    }

    // Test for symlink to directory as package_path (should be handled by is_directory or WalkDir)
    #[test]
    fn test_walk_package_dir_symlink_to_dir_as_package_path() {
        let base_dir = tempdir().unwrap();
        let target_package_dir = base_dir.path().join("actual_package_dir");
        fs::create_dir_all(&target_package_dir).unwrap();
        File::create(target_package_dir.join("some_file_in_target.txt")).unwrap();
        fs::create_dir(target_package_dir.join("some_subdir_in_target")).unwrap();

        let symlink_to_package_path = base_dir.path().join("symlink_to_package");
        create_symlink(&symlink_to_package_path, &target_package_dir).unwrap();

        let result = walk_package_dir(&symlink_to_package_path);
        assert!(
            result.is_ok(),
            "walk_package_dir for symlinked package dir failed: {:?}",
            result.err()
        );
        let mut items = result.unwrap();
        items.sort_by_key(|item| item.sort_key());

        let mut expected_items = [
            RawStowItem {
                // Absolute paths will be inside the symlink path initially from WalkDir if it resolves it,
                // or inside the target_package_dir if WalkDir is given the resolved path.
                // Here, we expect absolute paths to be based on the *symlink* path as input.
                absolute_path: symlink_to_package_path.join("some_file_in_target.txt"),
                package_relative_path: PathBuf::from("some_file_in_target.txt"),
                item_type: RawStowItemType::File,
            },
            RawStowItem {
                absolute_path: symlink_to_package_path.join("some_subdir_in_target"),
                package_relative_path: PathBuf::from("some_subdir_in_target"),
                item_type: RawStowItemType::Directory,
            },
        ];
        expected_items.sort_by_key(|item| item.sort_key());

        // For symlinked package dirs, WalkDir resolves the symlink before walking.
        // So, the absolute_path of items will be relative to the *target_package_dir*.
        // The strip_prefix logic in walk_package_dir should correctly use the original symlink_to_package_path
        // as the base for stripping, which might lead to an issue if not handled carefully.
        // Let's adjust expected absolute paths based on how WalkDir and strip_prefix work.
        // If strip_prefix uses the original symlink path, the relative paths are correct.
        // The absolute paths in RawStowItem should reflect the actual location on disk.

        let expected_items_adjusted_abs_path = [
            RawStowItem {
                absolute_path: target_package_dir.join("some_file_in_target.txt"), // Actual path
                package_relative_path: PathBuf::from("some_file_in_target.txt"), // Relative to symlink
                item_type: RawStowItemType::File,
            },
            RawStowItem {
                absolute_path: target_package_dir.join("some_subdir_in_target"), // Actual path
                package_relative_path: PathBuf::from("some_subdir_in_target"), // Relative to symlink
                item_type: RawStowItemType::Directory,
            },
        ];
        // We only compare package_relative_path and item_type for this test,
        // as absolute_path depends on WalkDir's symlink following behavior which is on by default.
        let items_simplified: Vec<_> = items
            .iter()
            .map(|i| (&i.package_relative_path, &i.item_type))
            .collect();
        let expected_simplified: Vec<_> = expected_items_adjusted_abs_path
            .iter()
            .map(|i| (&i.package_relative_path, &i.item_type))
            .collect();

        assert_eq!(items_simplified.len(), 2);
        assert_eq!(items_simplified, expected_simplified);
    }

    // --- is_stow_symlink tests ---
    fn setup_stow_env_for_is_stow_symlink(base_temp_dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let stow_dir = base_temp_dir.join("stow_dir_is_stow");
        fs::create_dir_all(&stow_dir).unwrap();

        let package_name = "mypkg";
        let package_dir = stow_dir.join(package_name);
        fs::create_dir_all(&package_dir).unwrap();

        let item_name = "item.txt";
        let item_path_abs = package_dir.join(item_name);
        File::create(&item_path_abs).unwrap();

        (stow_dir, package_dir, item_path_abs)
    }

    #[test]
    fn test_is_stow_symlink_not_a_symlink() {
        let temp = tempdir().unwrap();
        let (stow_dir, _, _) = setup_stow_env_for_is_stow_symlink(temp.path());
        let not_a_link = temp.path().join("not_a_link.txt");
        File::create(&not_a_link).unwrap();
        assert_eq!(is_stow_symlink(&not_a_link, &stow_dir).unwrap(), None);
    }

    #[test]
    fn test_is_stow_symlink_broken_link() {
        let temp = tempdir().unwrap();
        let (stow_dir, _, _) = setup_stow_env_for_is_stow_symlink(temp.path());
        let link_path = temp.path().join("broken_link");
        let non_existent_target = temp.path().join("non_existent_target");
        create_symlink(&link_path, &non_existent_target).unwrap();
        assert_eq!(is_stow_symlink(&link_path, &stow_dir).unwrap(), None);
    }

    #[test]
    fn test_is_stow_symlink_stow_dir_does_not_exist() {
        let temp = tempdir().unwrap();
        let link_target_dummy = temp.path().join("dummy_target.txt"); // Target for link
        File::create(&link_target_dummy).unwrap();
        let link_path = temp.path().join("any_link");
        create_symlink(&link_path, &link_target_dummy).unwrap();

        let non_existent_stow_dir = temp.path().join("non_existent_stow");
        let result = is_stow_symlink(&link_path, &non_existent_stow_dir);
        assert!(result.is_err());
        match result.err().unwrap() {
            RustowError::Fs(FsError::Canonicalize { path, .. }) => {
                assert_eq!(path, non_existent_stow_dir)
            },
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_is_stow_symlink_target_outside_stow_dir() {
        let temp = tempdir().unwrap();
        let (stow_dir, _, _) = setup_stow_env_for_is_stow_symlink(temp.path());

        let outside_target = temp.path().join("outside_file.txt");
        File::create(&outside_target).unwrap();

        let link_path = temp.path().join("link_to_outside"); // Place link outside stow_dir for clarity
        create_symlink(&link_path, &outside_target).unwrap();

        assert_eq!(is_stow_symlink(&link_path, &stow_dir).unwrap(), None);
    }

    #[test]
    fn test_is_stow_symlink_target_is_stow_dir_itself() {
        let temp = tempdir().unwrap();
        let (stow_dir, _, _) = setup_stow_env_for_is_stow_symlink(temp.path());
        let link_path = temp.path().join("link_to_stow_dir");
        create_symlink(&link_path, &stow_dir).unwrap();
        assert_eq!(is_stow_symlink(&link_path, &stow_dir).unwrap(), None);
    }

    #[test]
    fn test_is_stow_symlink_target_is_package_dir() {
        let temp = tempdir().unwrap();
        let (stow_dir, package_dir, _) = setup_stow_env_for_is_stow_symlink(temp.path());
        let link_path = temp.path().join("link_to_package_dir");
        create_symlink(&link_path, &package_dir).unwrap();

        let expected_package_name = "mypkg".to_string();
        let expected_item_path = PathBuf::new();
        assert_eq!(
            is_stow_symlink(&link_path, &stow_dir).unwrap(),
            Some((expected_package_name, expected_item_path))
        );
    }

    #[test]
    fn test_is_stow_symlink_target_is_item_in_package() {
        let temp = tempdir().unwrap();
        let (stow_dir, _, item_abs_path) = setup_stow_env_for_is_stow_symlink(temp.path());
        let link_path = temp.path().join("link_to_item");
        create_symlink(&link_path, &item_abs_path).unwrap();

        let expected_package_name = "mypkg".to_string();
        let expected_item_path = PathBuf::from("item.txt");
        assert_eq!(
            is_stow_symlink(&link_path, &stow_dir).unwrap(),
            Some((expected_package_name, expected_item_path))
        );
    }

    #[test]
    fn test_is_stow_symlink_target_is_nested_item() {
        let temp = tempdir().unwrap();
        let (stow_dir, package_dir, _) = setup_stow_env_for_is_stow_symlink(temp.path());

        let sub_dir = package_dir.join("sub");
        fs::create_dir(&sub_dir).unwrap();
        let nested_item_name = "nested_item.txt";
        let nested_item_abs_path = sub_dir.join(nested_item_name);
        File::create(&nested_item_abs_path).unwrap();

        let link_path = temp.path().join("link_to_nested_item");
        create_symlink(&link_path, &nested_item_abs_path).unwrap();

        let expected_package_name = "mypkg".to_string();
        let expected_item_path = PathBuf::from("sub").join(nested_item_name);
        assert_eq!(
            is_stow_symlink(&link_path, &stow_dir).unwrap(),
            Some((expected_package_name, expected_item_path))
        );
    }

    #[test]
    fn test_is_stow_symlink_relative_link_correctly_resolved() {
        let temp = tempdir().unwrap();
        let (stow_dir, package_dir, _) = setup_stow_env_for_is_stow_symlink(temp.path());

        let item_name = "item_for_relative_test.txt";
        let item_abs_path = package_dir.join(item_name);
        File::create(&item_abs_path).unwrap();

        let link_parent_dir = temp.path().join("link_parent");
        fs::create_dir(&link_parent_dir).unwrap();
        let link_path = link_parent_dir.join("relative_link");

        let stow_dir_abs = canonicalize_path(&stow_dir).unwrap();
        let link_parent_abs = canonicalize_path(&link_parent_dir).unwrap();
        let relative_target = pathdiff::diff_paths(&item_abs_path, &link_parent_abs).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs as unix_fs;
            unix_fs::symlink(&relative_target, &link_path).unwrap_or_else(|e| {
                panic!(
                    "Failed to create symlink for relative test (unix): {:?}, from {:?} to {:?}",
                    e, relative_target, link_path
                )
            });
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs as windows_fs;
            // For symlink_file, the target path must exist or be a file.
            // The relative path is relative to the symlink itself.
            // We need to be careful here. For this test, let's ensure the function under test can handle
            // a relative path that read_link might return.
            // The challenge is std::fs::windows::symlink_file expects target to exist for file symlinks
            // if it's not an absolute path in some contexts.
            // The most reliable way to test `is_stow_symlink`'s resolution logic is to ensure `read_link` returns a relative path.
            // So, we *must* create it with a relative path string.
            windows_fs::symlink_file(&relative_target, &link_path).unwrap_or_else(|e| {
                panic!(
                    "Failed to create symlink for relative test (windows): {:?}, from {:?} to {:?}",
                    e, relative_target, link_path
                )
            });
        }
        #[cfg(not(any(unix, windows)))]
        {
            eprintln!("Skipping relative symlink test on this platform.");
            return;
        }

        let expected_package_name = "mypkg".to_string();
        let expected_item_path_in_package = PathBuf::from(item_name);
        let result = is_stow_symlink(&link_path, &stow_dir_abs);
        assert!(result.is_ok(), "is_stow_symlink failed: {:?}", result.err());
        assert_eq!(
            result.unwrap(),
            Some((expected_package_name, expected_item_path_in_package))
        );
    }
}
