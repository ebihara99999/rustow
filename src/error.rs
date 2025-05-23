use std::io;
use thiserror::Error;

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
}

#[allow(dead_code)]
#[derive(Error, Debug)]
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
#[derive(Error, Debug)]
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
    #[error("Failed to create symlink from {from} to {to}: {reason}")]
    CreateSymlinkError {
        from: String,
        to: String,
        reason: String,
    },
    #[error("Failed to delete symlink at {path}: {reason}")]
    DeleteSymlinkError {
        path: String,
        reason: String,
    },
    #[error("Failed to create directory at {path}: {reason}")]
    CreateDirError {
        path: String,
        reason: String,
    },
    #[error("Failed to delete directory at {path}: {reason}")]
    DeleteDirError {
        path: String,
        reason: String,
    },
    #[error("Failed to move file from {from} to {to}: {reason}")]
    MoveFileError {
        from: String,
        to: String,
        reason: String,
    },
    #[error("Path not found: {0}")]
    PathNotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum IgnoreError {
    #[error("Failed to load ignore patterns: {0}")]
    LoadPatternsError(String),
    #[error("Invalid ignore pattern: {0}")]
    InvalidPattern(String),
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
        let err = FsError::CreateSymlinkError {
            from: "/source".to_string(),
            to: "/target".to_string(),
            reason: "permission denied".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Failed to create symlink from /source to /target: permission denied"
        );
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
} 
