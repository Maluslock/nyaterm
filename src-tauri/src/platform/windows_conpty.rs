use serde::Serialize;

use crate::runtime::AppRuntime;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalConptyInfo {
    pub available: bool,
    pub active_bundled: usize,
    pub active_system: usize,
    pub last_used: Option<&'static str>,
    pub fallback: bool,
    pub version: String,
}

pub(crate) fn configure(runtime: &AppRuntime) {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => {
            tracing::warn!(
                arch = other,
                "No bundled ConPTY build for this architecture"
            );
            return;
        }
    };
    let dll = runtime
        .executable_dir()
        .join("conpty")
        .join(arch)
        .join("conpty.dll");
    if let Err(error) = portable_pty::win::configure_bundled_conpty(dll) {
        tracing::warn!(error, "Could not configure bundled ConPTY");
    }
}

pub(crate) fn support_info() -> LocalConptyInfo {
    let status = portable_pty::win::conpty_status();
    let package: serde_json::Value =
        serde_json::from_str(include_str!("../../conpty-package.json"))
            .expect("valid bundled ConPTY package metadata");
    LocalConptyInfo {
        available: crate::platform::windows_version::current_windows_version()
            .is_none_or(|version| version.supports_conpty()),
        active_bundled: status.active_bundled,
        active_system: status.active_system,
        last_used: status.last_used.map(|backend| match backend {
            portable_pty::win::ConptyBackend::Bundled => "bundled",
            portable_pty::win::ConptyBackend::System => "system",
        }),
        fallback: status.fallback_reason.is_some(),
        version: package["version"].as_str().unwrap().to_string(),
    }
}
