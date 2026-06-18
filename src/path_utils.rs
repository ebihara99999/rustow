use std::path::{Path, PathBuf};

pub(crate) fn resolve_symlink_target(symlink_path: &Path, link_target: &Path) -> PathBuf {
    if link_target.is_absolute() {
        link_target.to_path_buf()
    } else {
        symlink_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(link_target)
    }
}

pub(crate) fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized_components = Vec::new();

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized_components.pop();
            },
            std::path::Component::CurDir => {},
            other => {
                normalized_components.push(other);
            },
        }
    }

    normalized_components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_symlink_target_keeps_absolute_target() {
        assert_eq!(
            resolve_symlink_target(Path::new("/tmp/link"), Path::new("/var/target")),
            PathBuf::from("/var/target")
        );
    }

    #[test]
    fn test_resolve_symlink_target_joins_relative_target_to_link_parent() {
        assert_eq!(
            resolve_symlink_target(Path::new("/tmp/dir/link"), Path::new("../target")),
            PathBuf::from("/tmp/dir/../target")
        );
    }

    #[test]
    fn test_normalize_path_components_collapses_dot_and_parent_components() {
        assert_eq!(
            normalize_path_components(Path::new("/tmp/dir/../target/./file")),
            PathBuf::from("/tmp/target/file")
        );
    }
}
