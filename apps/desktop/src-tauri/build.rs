fn main() {
    // Custom commands are denied by default: each one needs an `allow-*` permission in
    // `capabilities/default.json`.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "office_ensure_running",
            "organization_create",
            "organizations_list",
            "organization_open",
            "org_snapshot",
            "org_command",
            "connections_list",
        ]),
    ))
    .expect("failed to run tauri-build");
}
