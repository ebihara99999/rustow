use std::io;
use thiserror::Error;
use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum RustowError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Stow(#[from] StowError),
    #[error(transparent)]
    Fs(#[from] FsError),
    #[error(transparent)]
    Ignore(#[from] IgnoreError),
    #[error("CLI error: {0}")]
    Cli(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Regex(#[from] regex::Error),
    #[error("Invalid ignore pattern: {0}")]
    InvalidPattern(String),
}

#[allow(dead_code)]
#[derive(Error, Debug, Clone, PartialEq)]
pub enum ConfigError {
    #[error("Invalid target directory: {0}")]
    InvalidTargetDir(String),
    #[error("Invalid stow directory: {0}")]
    InvalidStowDir(String),
    #[error("Invalid package name: {0}")]
    InvalidPackageName(String),
    #[error("Invalid regex pattern: {0}")]
    InvalidRegexPattern(String),
    #[error("Invalid verbosity level: {0}")]
    InvalidVerbosityLevel(u8),
}

#[allow(dead_code)]
#[derive(Error, Debug, Clone, PartialEq)]
pub enum StowError {
    #[error("Conflict detected: {0}")]
    Conflict(String),
    #[error("Package not found: {0}")]
    PackageNotFound(String),
    #[error("Invalid package structure: {0}")]
    InvalidPackageStructure(String),
    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum FsError {
    #[error("IO error for path {path:?}: {source:?}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to canonicalize path {path:?}: {source:?}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Path {0:?} not found")]
    NotFound(PathBuf),
    #[error("Path {0:?} is not a directory")]
    NotADirectory(PathBuf),
    #[error("Path {0:?} is not a symbolic link")]
    NotASymlink(PathBuf),
    #[error("Failed to create symlink from {link_path:?} to {target_path:?}: {source:?}")]
    CreateSymlink {
        link_path: PathBuf,
        target_path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to read symlink {path:?}: {source:?}")]
    ReadSymlink {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to delete symlink {path:?}: {source:?}")]
    DeleteSymlink {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to create directory {path:?}: {source:?}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to delete directory {path:?}: {source:?}")]
    DeleteDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to move item from {source_path:?} to {destination_path:?}: {source_io_error:?}")]
    MoveItem {
        source_path: PathBuf,
        destination_path: PathBuf,
        #[source]
        source_io_error: std::io::Error,
    },
    #[error("Source and destination are the same for move: {0:?}")]
    MoveSamePath(PathBuf),
    #[error("WalkDir error for path {path:?}: {source:?}")]
    WalkDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[allow(dead_code)]
#[derive(Error, Debug, Clone, PartialEq)]
pub enum IgnoreError {
    #[error("Failed to load ignore patterns: {0}")]
    LoadPatternsError(String),
    #[error("Invalid ignore pattern: {0}")]
    InvalidPattern(String),
}

pub type Result<T, E = RustowError> = std::result::Result<T, E>;

// PartialEq for FsError variants containing std::io::Error for testing purposes.
// This compares based on the error kind.
impl PartialEq for FsError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FsError::Io { path: p1, source: s1 }, FsError::Io { path: p2, source: s2 }) =>
                p1 == p2 && s1.kind() == s2.kind(),
            (FsError::Canonicalize { path: p1, source: s1 }, FsError::Canonicalize { path: p2, source: s2 }) =>
                p1 == p2 && s1.kind() == s2.kind(),
            (FsError::NotFound(p1), FsError::NotFound(p2)) => p1 == p2,
            (FsError::NotADirectory(p1), FsError::NotADirectory(p2)) => p1 == p2,
            (FsError::NotASymlink(p1), FsError::NotASymlink(p2)) => p1 == p2,
            (FsError::CreateSymlink { link_path: lp1, target_path: tp1, source: s1 }, FsError::CreateSymlink { link_path: lp2, target_path: tp2, source: s2 }) =>
                lp1 == lp2 && tp1 == tp2 && s1.kind() == s2.kind(),
            (FsError::ReadSymlink { path: p1, source: s1 }, FsError::ReadSymlink { path: p2, source: s2 }) =>
                p1 == p2 && s1.kind() == s2.kind(),
            (FsError::DeleteSymlink { path: p1, source: s1 }, FsError::DeleteSymlink { path: p2, source: s2 }) =>
                p1 == p2 && s1.kind() == s2.kind(),
            (FsError::CreateDirectory { path: p1, source: s1 }, FsError::CreateDirectory { path: p2, source: s2 }) =>
                p1 == p2 && s1.kind() == s2.kind(),
            (FsError::DeleteDirectory { path: p1, source: s1 }, FsError::DeleteDirectory { path: p2, source: s2 }) =>
                p1 == p2 && s1.kind() == s2.kind(),
            (FsError::MoveItem { source_path: sp1, destination_path: dp1, source_io_error: s1 }, FsError::MoveItem { source_path: sp2, destination_path: dp2, source_io_error: s2 }) =>
                sp1 == sp2 && dp1 == dp2 && s1.kind() == s2.kind(),
            (FsError::MoveSamePath(p1), FsError::MoveSamePath(p2)) => p1 == p2,
            (FsError::WalkDir { path: p1, source: s1 }, FsError::WalkDir { path: p2, source: s2 }) =>
                p1 == p2 && s1.kind() == s2.kind(),
            _ => false, // Different enum variants
        }
    }
}

impl PartialEq for RustowError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (RustowError::Fs(a), RustowError::Fs(b)) => a == b,
            (RustowError::Io(a), RustowError::Io(b)) => a.kind() == b.kind(),
            (RustowError::InvalidPattern(a), RustowError::InvalidPattern(b)) => a == b,
            (RustowError::Cli(a), RustowError::Cli(b)) => a == b,
            (RustowError::Stow(a), RustowError::Stow(b)) => a == b,
            (RustowError::Ignore(a), RustowError::Ignore(b)) => a == b,
            (RustowError::Config(a), RustowError::Config(b)) => a == b,
            (RustowError::Regex(a), RustowError::Regex(b)) => a.to_string() == b.to_string(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustow_error_display() {
        let err = RustowError::Cli("invalid argument".to_string());
        assert_eq!(err.to_string(), "CLI error: invalid argument");
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::InvalidTargetDir("/invalid/path".to_string());
        assert_eq!(err.to_string(), "Invalid target directory: /invalid/path");
    }

    #[test]
    fn test_stow_error_display() {
        let err = StowError::PackageNotFound("mypackage".to_string());
        assert_eq!(err.to_string(), "Package not found: mypackage");
    }

    #[test]
    fn test_fs_error_display() {
        let err = FsError::CreateSymlink {
            link_path: PathBuf::from("/source"),
            target_path: PathBuf::from("/target"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied"),
        };
        let expected_error_message = format!(
            "Failed to create symlink from \"/source\" to \"/target\": Custom {{ kind: PermissionDenied, error: \"permission denied\" }}"
        );
        assert_eq!(err.to_string(), expected_error_message);
    }

    #[test]
    fn test_ignore_error_display() {
        let err = IgnoreError::InvalidPattern("*[invalid".to_string());
        assert_eq!(err.to_string(), "Invalid ignore pattern: *[invalid");
    }

    #[test]
    fn test_error_conversion() {
        let config_err = ConfigError::InvalidTargetDir("bad path".to_string());
        let rustow_err: RustowError = config_err.into();
        assert!(matches!(rustow_err, RustowError::Config(_)));

        let stow_err = StowError::PackageNotFound("missing".to_string());
        let rustow_err: RustowError = stow_err.into();
        assert!(matches!(rustow_err, RustowError::Stow(_)));
    }

    #[test]
    fn test_fs_error_partial_eq() {
        let p1 = PathBuf::from("/test/path1");
        let p2 = PathBuf::from("/test/path2");
        let err_kind = io::ErrorKind::NotFound;

        // Test Io variant
        assert_eq!(
            FsError::Io { path: p1.clone(), source: io::Error::from(err_kind) },
            FsError::Io { path: p1.clone(), source: io::Error::from(err_kind) }
        );
        assert_ne!(
            FsError::Io { path: p1.clone(), source: io::Error::from(err_kind) },
            FsError::Io { path: p2.clone(), source: io::Error::from(err_kind) }
        );
        assert_ne!(
            FsError::Io { path: p1.clone(), source: io::Error::from(err_kind) },
            FsError::Io { path: p1.clone(), source: io::Error::from(io::ErrorKind::PermissionDenied) }
        );

        // Test NotFound variant
        assert_eq!(FsError::NotFound(p1.clone()), FsError::NotFound(p1.clone()));
        assert_ne!(FsError::NotFound(p1.clone()), FsError::NotFound(p2.clone()));

        // Test different variants
        assert_ne!(
            FsError::Io { path: p1.clone(), source: io::Error::from(err_kind) },
            FsError::NotFound(p1.clone())
        );
    }

    #[test]
    fn test_rustow_error_partial_eq() {
        let p1 = PathBuf::from("/test/path1");
        let err_kind = io::ErrorKind::NotFound;

        // Reconstruct errors for comparison as FsError is not Clone
        assert_eq!(RustowError::Fs(FsError::NotFound(p1.clone())), RustowError::Fs(FsError::NotFound(p1.clone())));
        assert_ne!(RustowError::Fs(FsError::NotADirectory(p1.clone())), RustowError::Fs(FsError::NotFound(p1.clone())));

        assert_eq!(
            RustowError::Io(io::Error::from(err_kind)),
            RustowError::Io(io::Error::from(err_kind))
        );
        assert_ne!(
            RustowError::Io(io::Error::from(err_kind)),
            RustowError::Io(io::Error::from(io::ErrorKind::PermissionDenied))
        );
        // Reconstruct FsError for this comparison
        assert_ne!(RustowError::Fs(FsError::NotFound(p1.clone())), RustowError::Io(io::Error::from(err_kind)));

        assert_eq!(RustowError::InvalidPattern("pat1".to_string()), RustowError::InvalidPattern("pat1".to_string()));
        assert_ne!(RustowError::InvalidPattern("pat1".to_string()), RustowError::InvalidPattern("pat2".to_string()));
    }
}
