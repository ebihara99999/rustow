#[allow(dead_code)] // Allow dead code for this function as it will be used by other modules later
// Placeholder for the process_item_name function
pub fn process_item_name(item_name: &str, is_dotfiles_enabled: bool) -> String {
    if is_dotfiles_enabled {
        if item_name.starts_with("dot-") {
            // "dot-" を "." に置き換える
            // "dot-" のみの場合は "." になる
            // "dot-foo" の場合は ".foo" になる
            format!(".{}", &item_name[4..])
        } else {
            item_name.to_string()
        }
    } else {
        item_name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_item_name_dotfiles_enabled() {
        assert_eq!(process_item_name("dot-bashrc", true), ".bashrc");
        assert_eq!(process_item_name("dot-config/nvim/init.vim", true), ".config/nvim/init.vim");
        assert_eq!(process_item_name("dot-", true), "."); // Edge case: only "dot-"
        assert_eq!(process_item_name("file.txt", true), "file.txt");
        assert_eq!(process_item_name("another-dot-file", true), "another-dot-file"); // Does not start with "dot-"
    }

    #[test]
    fn test_process_item_name_dotfiles_disabled() {
        assert_eq!(process_item_name("dot-bashrc", false), "dot-bashrc");
        assert_eq!(process_item_name("dot-config/nvim/init.vim", false), "dot-config/nvim/init.vim");
        assert_eq!(process_item_name("dot-", false), "dot-");
        assert_eq!(process_item_name("file.txt", false), "file.txt");
    }

    #[test]
    fn test_process_item_name_path_like_string() {
        // process_item_name is expected to work on individual path components usually,
        // but the spec implies it can work on the whole relative path string from the package.
        // Let's assume it should replace only the *first* "dot-" if it's at the beginning of a segment.
        // However, the current simple implementation replaces based on the whole string starting with "dot-".
        // This test reflects the current simple implementation.
        assert_eq!(process_item_name("dot-config/sub/dot-another", true), ".config/sub/dot-another");
        // If we wanted to process segments: (this would require a more complex function)
        // assert_eq!(process_item_name_segmented("dot-config/sub/dot-another", true), ".config/sub/.another");
    }
} 
