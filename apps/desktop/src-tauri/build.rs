fn main() {
    // Custom commands are denied by default: each one needs an `allow-*` permission in
    // `capabilities/default.json`.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "office_ensure_running",
            "office_ping",
            "office_restart",
            "organizations_peek",
            "organization_create",
            "organizations_list",
            "organization_open",
            "org_snapshot",
            "org_command",
            "connections_list",
            "work_snapshot",
            "work_get",
            "work_command",
            "login_item_status",
            "login_item_set",
            "login_item_open_settings",
            "open_external_url",
        ]),
    ))
    .expect("failed to run tauri-build");
}
