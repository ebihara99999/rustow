pub fn process_item_name(item_name: &str, is_dotfiles_enabled: bool) -> String {
    if !is_dotfiles_enabled {
        return item_name.to_string();
    }

    let mut processed = std::path::PathBuf::new();
    for component in std::path::Path::new(item_name).components() {
        match component {
            std::path::Component::Normal(name) => {
                let name_str = name.to_string_lossy();
                if let Some(stripped) = name_str.strip_prefix("dot-") {
                    processed.push(format!(".{}", stripped));
                } else {
                    processed.push(name_str.as_ref());
                }
            },
            std::path::Component::CurDir => processed.push("."),
            std::path::Component::ParentDir => processed.push(".."),
            std::path::Component::RootDir => processed.push(std::path::Path::new("/")),
            std::path::Component::Prefix(prefix) => processed.push(prefix.as_os_str()),
        }
    }

    processed.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_item_name_dotfiles_enabled() {
        assert_eq!(process_item_name("dot-bashrc", true), ".bashrc");
        assert_eq!(
            process_item_name("dot-config/nvim/init.vim", true),
            ".config/nvim/init.vim"
        );
        assert_eq!(process_item_name("dot-", true), "."); // Edge case: only "dot-"
        assert_eq!(process_item_name("file.txt", true), "file.txt");
        assert_eq!(
            process_item_name("another-dot-file", true),
            "another-dot-file"
        ); // Does not start with "dot-"
    }

    #[test]
    fn test_process_item_name_dotfiles_disabled() {
        assert_eq!(process_item_name("dot-bashrc", false), "dot-bashrc");
        assert_eq!(
            process_item_name("dot-config/nvim/init.vim", false),
            "dot-config/nvim/init.vim"
        );
        assert_eq!(process_item_name("dot-", false), "dot-");
        assert_eq!(process_item_name("file.txt", false), "file.txt");
    }

    #[test]
    fn test_process_item_name_path_like_string() {
        // process_item_name is expected to work on individual path components usually,
        // but the spec implies it can work on the whole relative path string from the package.
        assert_eq!(
            process_item_name("dot-config/sub/dot-another", true),
            ".config/sub/.another"
        );
    }
}
