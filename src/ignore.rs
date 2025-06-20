// src/ignore.rs

use regex;
use regex::Regex;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct IgnorePatterns {
    patterns: Vec<Regex>,
}

#[derive(Debug)]
pub enum IgnoreError {
    FileIoError {
        path: PathBuf,
        source: io::Error,
    },
    InvalidPattern {
        pattern: String,
        source: regex::Error,
    },
}

// item_package_relative_path is expected to start with "/" (e.g., "/file.txt", "/dir/item.conf")
// item_basename is the file or directory name (e.g., "file.txt", "item.conf")
pub fn is_ignored(
    item_package_relative_path: &Path,
    item_basename: &str,
    ignore_patterns: &IgnorePatterns,
) -> bool {
    let relative_path_str: &str = item_package_relative_path.to_str().unwrap_or("");

    for regex_pattern in &ignore_patterns.patterns {
        let pattern_str: &str = regex_pattern.as_str();
        if pattern_str.contains('/') {
            if regex_pattern.is_match(relative_path_str) {
                return true;
            }
        } else {
            // Check current item's basename directly
            if regex_pattern.is_match(item_basename) {
                return true;
            }
            // Check if any parent directory component in the path matches the basename pattern
            let mut path_accumulator: PathBuf = PathBuf::new();
            for component in item_package_relative_path.components() {
                match component {
                    std::path::Component::RootDir => {
                        path_accumulator.push(component.as_os_str());
                    },
                    std::path::Component::Normal(name_os_str) => {
                        path_accumulator.push(name_os_str);
                        let name_str_cow: std::borrow::Cow<str> = name_os_str.to_string_lossy();
                        let name_str: &str = name_str_cow.as_ref(); // Convert Cow to &str

                        if regex_pattern.is_match(name_str) {
                            // If this component (name_str) is the item_basename itself,
                            // and the item is a top-level item (e.g. item_package_relative_path is "/.git" and name_str is ".git"),
                            // then it was already caught by the direct item_basename check above. So we don't return true here for that case.
                            // We want to return true if a *parent* directory component matches.

                            // Check if the current component `name_str` is a genuine parent part of the path,
                            // not just the item itself if it's at the root of the relative path.
                            // Example: item_package_relative_path = "/.git", item_basename = ".git", name_str = ".git"
                            // Here, `name_str == item_basename` is true.
                            // `item_package_relative_path.strip_prefix("/").unwrap_or_default() == Path::new(name_str)` would be `Path::new(".git") == Path::new(".git")`, true.
                            // So, this would NOT return true, which is correct (it was caught by the item_basename check).

                            // Example: item_package_relative_path = "/.git/config", item_basename = "config", name_str = ".git"
                            // Here, `name_str == item_basename` is false.
                            // So, it returns true, which is correct (parent .git matched).

                            // Example: item_package_relative_path = "/foo/.git/config", item_basename = "config", name_str = ".git"
                            // Here, `name_str == item_basename` is false.
                            // So, it returns true, correct.

                            let is_top_level_item_match: bool = item_package_relative_path
                                .strip_prefix("/")
                                .is_ok_and(|p| p == Path::new(name_str));

                            if !(name_str == item_basename && is_top_level_item_match) {
                                return true;
                            }
                        }
                    },
                    _ => {},
                }
            }
        }
    }
    false
}

// Helper function to read patterns from a file, skipping comments and empty lines
fn read_patterns_from_file(file_path: &Path) -> Result<Vec<Regex>, IgnoreError> {
    let file: File = File::open(file_path).map_err(|e| IgnoreError::FileIoError {
        path: file_path.to_path_buf(),
        source: e,
    })?;
    let reader: BufReader<File> = BufReader::new(file);
    let mut patterns: Vec<Regex> = Vec::new();

    for line_result in reader.lines() {
        let line: String = line_result.map_err(|e| IgnoreError::FileIoError {
            path: file_path.to_path_buf(),
            source: e,
        })?;
        let trimmed_line: &str = line.trim();

        if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
            continue;
        }
        patterns.push(
            Regex::new(trimmed_line).map_err(|e| IgnoreError::InvalidPattern {
                pattern: trimmed_line.to_string(),
                source: e,
            })?,
        );
    }
    Ok(patterns)
}

// Default ignore patterns based on specification.md
// Section "D. Examples of default ignore patterns", the table.
const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    // Basename patterns (those without explicit path separators in the pattern example column)
    r"\.git",
    r"CVS",
    r"\.svn",
    r"RCS",
    r"_darcs",
    r".*~",   // Example: file.txt~
    r"#.*#",  // Example: #file.txt#
    r"\.#.+", // From spec table for Emacs lock files etc. Example: .#file.txt
    r".+,v",  // Corrected: From spec table for RCS/CVS version files. Example: file.c,v
    r"\.stow-local-ignore",
    r"\.gitignore",
    r"\.cvsignore",
    // Full path patterns (must start with ^/ as per spec examples)
    r"^/README.*",
    r"^/LICENSE.*",
    r"^/COPYING$", // Note: no wildcard *, ensure exact match
];

fn get_default_ignore_patterns() -> Result<Vec<Regex>, IgnoreError> {
    DEFAULT_IGNORE_PATTERNS
        .iter()
        .map(|s| {
            Regex::new(s).map_err(|e| IgnoreError::InvalidPattern {
                pattern: (*s).to_string(), // Dereference &&str to &str, then to_string()
                source: e,
            })
        })
        .collect()
}

impl IgnorePatterns {
    // Helper for tests
    #[cfg(test)]
    fn new_for_test(regex_strings: Vec<&str>) -> Self {
        IgnorePatterns {
            patterns: regex_strings
                .into_iter()
                .map(|s| Regex::new(s).unwrap())
                .collect(),
        }
    }

    // Public method to iterate over the compiled regex patterns
    pub fn iter_patterns(&self) -> impl Iterator<Item = &Regex> {
        self.patterns.iter()
    }

    pub fn load(
        stow_dir: &Path,
        package_name: Option<&str>,
        home_dir: &Path, // For resolving ~/.stow-global-ignore
    ) -> Result<Self, IgnoreError> {
        // 1. Try package-local ignore list: <stow_dir>/<package_name>/.stow-local-ignore
        if let Some(name) = package_name {
            let local_ignore_path: PathBuf = stow_dir.join(name).join(".stow-local-ignore");
            if local_ignore_path.is_file() {
                // Check if it's a file
                return Ok(IgnorePatterns {
                    patterns: read_patterns_from_file(&local_ignore_path)?,
                });
            }
        }

        // 2. Try global ignore list: <home_dir>/.stow-global-ignore
        let global_ignore_path: PathBuf = home_dir.join(".stow-global-ignore");
        if global_ignore_path.is_file() {
            // Check if it's a file
            return Ok(IgnorePatterns {
                patterns: read_patterns_from_file(&global_ignore_path)?,
            });
        }

        // 3. Use built-in default ignore list
        Ok(IgnorePatterns {
            patterns: get_default_ignore_patterns()?,
        })
    }
}

// For filter_items test purposes, a simplified item structure.
// The actual StowItem/RawStowItem will be more complex and likely live in another module.
#[derive(Debug, Clone, PartialEq)]
pub struct MinimalStowableItem {
    pub package_relative_path: PathBuf, // Path relative to package root, e.g., "foo/bar.txt" or "baz.conf"
    pub basename: String,               // Basename of the item, e.g., "bar.txt" or "baz.conf"
}

pub fn filter_items(
    raw_items: Vec<MinimalStowableItem>,
    ignore_patterns: &IgnorePatterns,
) -> Vec<MinimalStowableItem> {
    raw_items
        .into_iter()
        .filter(|item| {
            // Construct the path starting with "/" for is_ignored.
            // e.g., if item.package_relative_path is "foo/bar.txt", path_for_is_ignored becomes "/foo/bar.txt".
            // e.g., if item.package_relative_path is "file.txt", path_for_is_ignored becomes "/file.txt".
            let path_for_is_ignored = PathBuf::from("/").join(&item.package_relative_path);
            !is_ignored(&path_for_is_ignored, &item.basename, ignore_patterns)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    // Note: The tempdir crate is not available in this environment.
    // Tests will create files in subdirectories and clean them up.

    // Helper for creating temporary ignore files for tests
    fn create_temp_file_for_test(path: &Path, content: &str) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    // --- Tests for is_ignored ---
    #[test]
    fn test_is_ignored_empty_patterns() {
        let patterns = IgnorePatterns::new_for_test(vec![]);
        assert!(!is_ignored(Path::new("/foo.txt"), "foo.txt", &patterns));
        assert!(!is_ignored(Path::new("/bar/baz.rs"), "baz.rs", &patterns));
    }

    #[test]
    fn test_is_ignored_basename_match() {
        let patterns =
            IgnorePatterns::new_for_test(vec![r"\.log$", r"temp", r"^exact_filename\.rs$"]);

        // Matches "\.log$" (ends with .log)
        assert!(is_ignored(Path::new("/mylog.log"), "mylog.log", &patterns));
        assert!(is_ignored(
            Path::new("/dir/access.log"),
            "access.log",
            &patterns
        ));
        assert!(!is_ignored(
            Path::new("/logger.txt"),
            "logger.txt",
            &patterns
        ));

        // Matches "temp" (contains "temp" in basename)
        assert!(is_ignored(
            Path::new("/foo/temporary_file.txt"),
            "temporary_file.txt",
            &patterns
        ));
        assert!(is_ignored(
            Path::new("/bar/my_temp_dir"),
            "my_temp_dir",
            &patterns
        ));
        assert!(is_ignored(Path::new("/baz/temp"), "temp", &patterns));
        assert!(!is_ignored(
            Path::new("/qux/archive.zip"),
            "archive.zip",
            &patterns
        ));

        // Matches "^exact_filename\.rs$" (exactly "exact_filename.rs")
        assert!(is_ignored(
            Path::new("/src/exact_filename.rs"),
            "exact_filename.rs",
            &patterns
        ));
        assert!(!is_ignored(
            Path::new("/src/exact_filename_extra.rs"),
            "exact_filename_extra.rs",
            &patterns
        ));
    }

    #[test]
    fn test_is_ignored_fullpath_match() {
        let patterns = IgnorePatterns::new_for_test(vec![r"^/specific/file\.txt$", r"^/config/"]);
        assert!(is_ignored(
            Path::new("/specific/file.txt"),
            "file.txt",
            &patterns
        ));
        assert!(!is_ignored(
            Path::new("/notspecific/file.txt"),
            "file.txt",
            &patterns
        ));
        assert!(is_ignored(
            Path::new("/config/settings.json"),
            "settings.json",
            &patterns
        ));
        assert!(!is_ignored(
            Path::new("/conf/settings.json"),
            "settings.json",
            &patterns
        ));
    }

    #[test]
    fn test_is_ignored_default_patterns_examples_from_spec() {
        let patterns = IgnorePatterns {
            patterns: get_default_ignore_patterns().unwrap(),
        };

        // Basename matches from default
        assert!(is_ignored(Path::new("/.git"), ".git", &patterns));
        assert!(is_ignored(Path::new("/some/dir/.git"), ".git", &patterns));
        assert!(is_ignored(Path::new("/file.txt~"), "file.txt~", &patterns));
        assert!(is_ignored(
            Path::new("/#save.txt#"),
            "#save.txt#",
            &patterns
        ));
        assert!(is_ignored(
            Path::new("/.#lockfile"),
            ".#lockfile",
            &patterns
        )); // Matches `\.#.+`
        assert!(is_ignored(Path::new("/ver,v"), "ver,v", &patterns)); // Matches `\.+,v`
        assert!(is_ignored(
            Path::new("/.stow-local-ignore"),
            ".stow-local-ignore",
            &patterns
        ));

        // Full path matches from default
        assert!(is_ignored(Path::new("/README.md"), "README.md", &patterns));
        assert!(is_ignored(
            Path::new("/LICENSE.txt"),
            "LICENSE.txt",
            &patterns
        ));
        assert!(is_ignored(Path::new("/COPYING"), "COPYING", &patterns));
        assert!(!is_ignored(
            Path::new("/docs/README.md"),
            "README.md",
            &patterns
        )); // Not at root
        assert!(!is_ignored(Path::new("/src/COPYING"), "COPYING", &patterns)); // Not at root
        assert!(!is_ignored(
            Path::new("/COPYING.bak"),
            "COPYING.bak",
            &patterns
        )); // Not an exact match for ^/COPYING$
    }

    // --- Tests for IgnorePatterns::load ---
    // Base directory for load tests to avoid polluting the project root.
    const TEST_LOAD_BASE_DIR: &str = "target/test_ignore_load_data";

    fn setup_load_test_dir(test_name: &str) -> PathBuf {
        let base = PathBuf::from(TEST_LOAD_BASE_DIR).join(test_name);
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn teardown_load_test_dir(base_dir: &Path) {
        fs::remove_dir_all(base_dir).unwrap();
    }

    #[test]
    fn test_load_ignore_patterns_local_only() {
        let base_dir = setup_load_test_dir("load_local");
        let stow_dir = base_dir.join("stow_root");
        fs::create_dir_all(&stow_dir).unwrap();
        let package_name = "mypkg";
        let package_dir = stow_dir.join(package_name);
        fs::create_dir_all(&package_dir).unwrap();

        let local_ignore_content = ".*\\.log\n# Comment\n\ntemp_file";
        create_temp_file_for_test(
            &package_dir.join(".stow-local-ignore"),
            local_ignore_content,
        )
        .unwrap();
        let home_dir = base_dir.join("home_dummy"); // Should not be accessed
        fs::create_dir_all(&home_dir).unwrap();

        let patterns = IgnorePatterns::load(&stow_dir, Some(package_name), &home_dir).unwrap();
        assert_eq!(patterns.patterns.len(), 2);
        assert_eq!(patterns.patterns[0].as_str(), ".*\\.log");
        assert_eq!(patterns.patterns[1].as_str(), "temp_file");

        teardown_load_test_dir(&base_dir);
    }

    #[test]
    fn test_load_ignore_patterns_global_only() {
        let base_dir = setup_load_test_dir("load_global");
        let stow_dir = base_dir.join("stow_root_no_local");
        fs::create_dir_all(&stow_dir).unwrap();

        let home_dir = base_dir.join("actual_home");
        fs::create_dir_all(&home_dir).unwrap();
        let global_ignore_content = "^/glob/\n\\.cache";
        create_temp_file_for_test(&home_dir.join(".stow-global-ignore"), global_ignore_content)
            .unwrap();

        let patterns = IgnorePatterns::load(&stow_dir, Some("pkg_no_local"), &home_dir).unwrap();
        assert_eq!(patterns.patterns.len(), 2);
        assert_eq!(patterns.patterns[0].as_str(), "^/glob/");
        assert_eq!(patterns.patterns[1].as_str(), "\\.cache");

        teardown_load_test_dir(&base_dir);
    }

    #[test]
    fn test_load_ignore_patterns_default_only() {
        let base_dir = setup_load_test_dir("load_default");
        let stow_dir = base_dir.join("stow_root_no_files");
        fs::create_dir_all(&stow_dir).unwrap();
        let home_dir = base_dir.join("home_no_files");
        fs::create_dir_all(&home_dir).unwrap();

        let patterns = IgnorePatterns::load(&stow_dir, Some("pkg_no_files"), &home_dir).unwrap();
        let default_expected = get_default_ignore_patterns().unwrap();
        assert_eq!(patterns.patterns.len(), default_expected.len());
        assert!(
            patterns
                .patterns
                .iter()
                .zip(default_expected.iter())
                .all(|(a, b)| a.as_str() == b.as_str())
        );

        teardown_load_test_dir(&base_dir);
    }

    #[test]
    fn test_load_ignore_patterns_local_overrides_global() {
        let base_dir = setup_load_test_dir("load_local_over_global");
        let stow_dir = base_dir.join("stow_root");
        fs::create_dir_all(&stow_dir).unwrap();
        let package_name = "mypkg_with_local";
        let package_dir = stow_dir.join(package_name);
        fs::create_dir_all(&package_dir).unwrap();

        create_temp_file_for_test(&package_dir.join(".stow-local-ignore"), "local_rule").unwrap();

        let home_dir = base_dir.join("home_with_global");
        fs::create_dir_all(&home_dir).unwrap();
        create_temp_file_for_test(&home_dir.join(".stow-global-ignore"), "global_rule").unwrap(); // Should be ignored

        let patterns = IgnorePatterns::load(&stow_dir, Some(package_name), &home_dir).unwrap();
        assert_eq!(patterns.patterns.len(), 1);
        assert_eq!(patterns.patterns[0].as_str(), "local_rule");

        teardown_load_test_dir(&base_dir);
    }

    #[test]
    fn test_load_ignore_patterns_invalid_regex_in_file() {
        let base_dir = setup_load_test_dir("invalid_regex_test");
        let stow_dir = base_dir.join("stow");
        let home_dir = base_dir.join("home");
        let package_name = "pkg_with_invalid_regex";
        let package_dir = stow_dir.join(package_name);
        fs::create_dir_all(&package_dir).unwrap();

        // File with an invalid regex pattern, e.g., an unclosed parenthesis
        let ignore_content = "valid_pattern\n*[invalid\nanother_valid";
        create_temp_file_for_test(&package_dir.join(".stow-local-ignore"), ignore_content).unwrap();

        let result = IgnorePatterns::load(&stow_dir, Some(package_name), &home_dir);
        assert!(result.is_err());
        match result.err().unwrap() {
            IgnoreError::InvalidPattern { pattern, source: _ } => {
                assert_eq!(pattern, "*[invalid"); // Check that the correct failing pattern is reported
            },
            // Remove or comment out the catch-all for other error types if not expected
            // Or, if Io errors are possible here (e.g. if file disappears after check),
            // add a specific match arm for `IgnoreError::FileIoError { .. }`
            // For now, let's assume only InvalidPattern is expected here.
            e => panic!("Expected InvalidPattern error, but got {:?}", e),
        }

        teardown_load_test_dir(&base_dir);
    }

    // --- Tests for filter_items ---
    #[test]
    fn test_filter_items() {
        let patterns =
            IgnorePatterns::new_for_test(vec![r"\.log$", r"^/secrets/", r"config\.json"]);
        let items = vec![
            MinimalStowableItem {
                package_relative_path: PathBuf::from("mylog.log"),
                basename: "mylog.log".to_string(),
            }, // ignore
            MinimalStowableItem {
                package_relative_path: PathBuf::from("data/file.txt"),
                basename: "file.txt".to_string(),
            }, // keep
            MinimalStowableItem {
                package_relative_path: PathBuf::from("secrets/key.pem"),
                basename: "key.pem".to_string(),
            }, // ignore
            MinimalStowableItem {
                package_relative_path: PathBuf::from("myapp/config.json"),
                basename: "config.json".to_string(),
            }, // ignore
            MinimalStowableItem {
                package_relative_path: PathBuf::from("myapp/settings.xml"),
                basename: "settings.xml".to_string(),
            }, // keep
        ];

        let filtered = filter_items(items, &patterns);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].basename, "file.txt");
        assert_eq!(filtered[1].basename, "settings.xml");
    }
}
