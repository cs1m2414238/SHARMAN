fn main() {
    #[cfg(windows)]
    {
        use std::{env, fs, path::PathBuf};

        // Windows' resource compiler mishandles apostrophes in quoted icon paths. Copy the icon
        // to a stable temp path so builds also work from directories such as `Sharman's`.
        let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("icons")
            .join("icon.ico");
        let resource_directory = env::temp_dir().join("agentrelay-tauri-build");
        fs::create_dir_all(&resource_directory).expect("failed to create resource directory");
        let icon = resource_directory.join("icon.ico");
        fs::copy(source, &icon).expect("failed to stage the AgentRelay icon");

        let windows = tauri_build::WindowsAttributes::new().window_icon_path(icon);
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("failed to build Tauri resources");
    }

    #[cfg(not(windows))]
    tauri_build::build();
}
