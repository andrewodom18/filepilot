use std::path::Path;

pub(crate) fn move_to_trash(path: &Path) -> Result<(), trash::Error> {
    platform_trash_context().delete(path)
}

fn platform_trash_context() -> trash::TrashContext {
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};

        let mut context = trash::TrashContext::new();
        // Finder is the trash crate's default on macOS, but it shells out to
        // AppleScript and can wait indefinitely for Automation permission.
        // NSFileManager provides the same recoverable Trash behavior without
        // requiring FilePilot to control Finder.
        context.set_delete_method(DeleteMethod::NsFileManager);
        context
    }

    #[cfg(not(target_os = "macos"))]
    {
        trash::TrashContext::new()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_ns_file_manager_without_finder_automation() {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};

        assert!(matches!(
            super::platform_trash_context().delete_method(),
            DeleteMethod::NsFileManager
        ));
    }
}
