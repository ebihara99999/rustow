#[allow(dead_code)] // Allow dead code for this function as it will be used by other modules later
// Placeholder for the process_item_name function
pub fn process_item_name(item_name: &str, is_dotfiles_enabled: bool) -> String {
    if is_dotfiles_enabled && item_name.starts_with("dot-") {
        // Replace the "dot-" part with "."
        // Get the substring after "dot-" using item_name["dot-".len()..]
        format!(".{}", &item_name["dot-".len()..])
    } else {
        item_name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_item_name_dotfiles_enabled() {
        assert_eq!(process_item_name("dot-foo", true), ".foo");
        assert_eq!(process_item_name("dot-bar/baz", true), ".bar/baz");
        assert_eq!(process_item_name("nodotprefix", true), "nodotprefix");
        assert_eq!(process_item_name("", true), "");
        assert_eq!(process_item_name("already.dot", true), "already.dot");
    }

    #[test]
    fn test_process_item_name_dotfiles_disabled() {
        assert_eq!(process_item_name("dot-foo", false), "dot-foo");
        assert_eq!(process_item_name("dot-bar/baz", false), "dot-bar/baz");
        assert_eq!(process_item_name("nodotprefix", false), "nodotprefix");
        assert_eq!(process_item_name("", false), "");
        assert_eq!(process_item_name("already.dot", false), "already.dot");
    }
} 
