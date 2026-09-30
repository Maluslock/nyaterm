use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use tauri_plugin_updater::{Update, UpdaterExt};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::runtime::AppRuntime;

const PORTABLE_ROOT: &str = "NyaTerm-portable";
const PORTABLE_EXE: &str = "NyaTerm.exe";
const PORTABLE_MCP: &str = "nyaterm-mcp.exe";
const PORTABLE_MARKER: &str = "portable.flag";
const CONPTY_FILES: [&str; 6] = [
    "conpty/x64/conpty.dll",
    "conpty/x64/x64/OpenConsole.exe",
    "conpty/x64/arm64/OpenConsole.exe",
    "conpty/arm64/conpty.dll",
    "conpty/arm64/arm64/OpenConsole.exe",
    "conpty/LICENSE.txt",
];
const MCP_BACKUP_PREFIX: &str = ".nyaterm-mcp-update-backup-";
const LEGACY_MCP_BACKUP: &str = ".nyaterm-mcp-update-backup.exe";
const HELPER_FLAG: &str = "--nyaterm-portable-update-helper";
const CLEANUP_ENV: &str = "NYATERM_PORTABLE_UPDATE_CLEANUP";
const WORK_DIR_PREFIX: &str = "nyaterm-portable-update-";
const MAX_ARCHIVE_ENTRIES: usize = 128;
const MAX_PAYLOAD_BYTES: u64 = 512 * 1024 * 1024;
const STALE_WORK_DIR_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableUpdateInfo {
    version: String,
    date: Option<String>,
    body: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableUpdateProgress {
    downloaded: u64,
    total: u64,
}

#[derive(Debug)]
struct StagedPortableUpdate {
    work_dir: PathBuf,
    helper_exe: PathBuf,
    payload_exe: PathBuf,
    payload_mcp: PathBuf,
    payload_marker: PathBuf,
    payload_conpty: PathBuf,
}

#[derive(Default)]
pub struct PortableUpdateState {
    downloading: AtomicBool,
    staged: Mutex<Option<StagedPortableUpdate>>,
}

struct DownloadGuard<'a>(&'a AtomicBool);

impl Drop for DownloadGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub async fn check(
    app: &tauri::AppHandle,
    runtime: &AppRuntime,
) -> AppResult<Option<PortableUpdateInfo>> {
    ensure_portable_runtime(runtime)?;
    Ok(portable_update(app, std::env::consts::ARCH)
        .await?
        .map(|update| PortableUpdateInfo {
            version: update.version,
            date: update.date.and_then(|date| date.format(&Rfc3339).ok()),
            body: update.body,
        }))
}

pub async fn download(
    app: &tauri::AppHandle,
    runtime: &AppRuntime,
    state: State<'_, PortableUpdateState>,
    on_progress: Channel<PortableUpdateProgress>,
) -> AppResult<()> {
    ensure_portable_runtime(runtime)?;
    ensure_install_directory_writable(runtime.executable_dir())?;

    state
        .downloading
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| AppError::Config("A portable update download is already running".into()))?;
    let _guard = DownloadGuard(&state.downloading);

    let update = portable_update(app, std::env::consts::ARCH)
        .await?
        .ok_or_else(|| AppError::Config("No portable update is available".into()))?;
    let mut downloaded = 0_u64;
    let bytes = update
        .download(
            |chunk_len, total| {
                downloaded = downloaded.saturating_add(chunk_len as u64);
                let _ = on_progress.send(PortableUpdateProgress {
                    downloaded,
                    total: total.unwrap_or(0),
                });
            },
            || {},
        )
        .await
        .map_err(|error| {
            AppError::Config(format!(
                "Portable update download or signature verification failed: {error}"
            ))
        })?;
    if bytes.len() as u64 > MAX_PAYLOAD_BYTES {
        return Err(AppError::Config(
            "Portable update archive is larger than the allowed limit".into(),
        ));
    }
    let _ = on_progress.send(PortableUpdateProgress {
        downloaded: bytes.len() as u64,
        total: bytes.len() as u64,
    });

    let staged = stage_verified_archive(&bytes)?;
    let mut slot = state
        .staged
        .lock()
        .map_err(|_| AppError::Config("Portable update state is unavailable".into()))?;
    if let Some(previous) = slot.replace(staged) {
        let _ = fs::remove_dir_all(previous.work_dir);
    }
    Ok(())
}

pub fn apply(
    app: &tauri::AppHandle,
    runtime: &AppRuntime,
    state: State<'_, PortableUpdateState>,
) -> AppResult<()> {
    ensure_portable_runtime(runtime)?;
    if state.downloading.load(Ordering::Acquire) {
        return Err(AppError::Config(
            "The portable update is still downloading".into(),
        ));
    }

    let staged = state
        .staged
        .lock()
        .map_err(|_| AppError::Config("Portable update state is unavailable".into()))?
        .take()
        .ok_or_else(|| AppError::Config("No portable update has been staged".into()))?;

    if !staged.helper_exe.is_file()
        || !staged.payload_exe.is_file()
        || !staged.payload_mcp.is_file()
        || !staged.payload_marker.is_file()
        || CONPTY_FILES
            .iter()
            .any(|path| !staged.payload_conpty.parent().unwrap().join(path).is_file())
    {
        let _ = fs::remove_dir_all(&staged.work_dir);
        return Err(AppError::Config(
            "The staged portable update is incomplete".into(),
        ));
    }

    let target_exe = std::env::current_exe()?;
    let spawn_result = Command::new(&staged.helper_exe)
        .arg(HELPER_FLAG)
        .arg(std::process::id().to_string())
        .arg(&staged.payload_exe)
        .arg(&target_exe)
        .arg(&staged.work_dir)
        .arg(&staged.payload_mcp)
        .arg(&staged.payload_conpty)
        .spawn();

    if let Err(error) = spawn_result {
        let _ = fs::remove_dir_all(&staged.work_dir);
        return Err(error.into());
    }

    crate::app::prepare_app_shutdown(app);
    app.exit(0);
    Ok(())
}

fn ensure_portable_runtime(runtime: &AppRuntime) -> AppResult<()> {
    if !cfg!(windows) || !runtime.portable() {
        return Err(AppError::Config(
            "Portable updates are only available in the Windows portable edition".into(),
        ));
    }
    Ok(())
}

async fn portable_update(app: &tauri::AppHandle, arch: &str) -> AppResult<Option<Update>> {
    let target = portable_target_for_arch(arch)?;
    let updater = app
        .updater_builder()
        .target(target)
        .build()
        .map_err(|error| {
            AppError::Config(format!("Failed to configure portable updater: {error}"))
        })?;
    match updater.check().await {
        Ok(update) => Ok(update),
        Err(
            tauri_plugin_updater::Error::TargetNotFound(_)
            | tauri_plugin_updater::Error::TargetsNotFound(_),
        ) => Ok(None),
        Err(error) => Err(AppError::Config(format!(
            "Failed to check for a portable update: {error}"
        ))),
    }
}

fn portable_target_for_arch(arch: &str) -> AppResult<&'static str> {
    match arch {
        "x86_64" => Ok("windows-x86_64-portable"),
        "aarch64" => Ok("windows-aarch64-portable"),
        other => Err(AppError::Config(format!(
            "Unsupported portable update architecture: {other}"
        ))),
    }
}

fn ensure_install_directory_writable(directory: &Path) -> AppResult<()> {
    let probe = directory.join(format!(".nyaterm-update-write-test-{}", Uuid::new_v4()));
    fs::write(&probe, b"update-write-test")?;
    fs::remove_file(probe)?;
    Ok(())
}

fn stage_verified_archive(bytes: &[u8]) -> AppResult<StagedPortableUpdate> {
    let work_dir = std::env::temp_dir().join(format!("{WORK_DIR_PREFIX}{}", Uuid::new_v4()));
    fs::create_dir(&work_dir)?;

    let result = (|| {
        let payload_dir = work_dir.join("payload");
        fs::create_dir(&payload_dir)?;
        extract_portable_payload(bytes, &payload_dir)?;

        let current_exe = std::env::current_exe()?;
        let helper_exe = work_dir.join("NyaTerm-update-helper.exe");
        fs::copy(current_exe, &helper_exe)?;

        Ok(StagedPortableUpdate {
            payload_exe: payload_dir.join(PORTABLE_EXE),
            payload_mcp: payload_dir.join(PORTABLE_MCP),
            payload_marker: payload_dir.join(PORTABLE_MARKER),
            payload_conpty: payload_dir.join("conpty"),
            helper_exe,
            work_dir: work_dir.clone(),
        })
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&work_dir);
    }
    result
}

fn extract_portable_payload(bytes: &[u8], destination: &Path) -> AppResult<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|error| AppError::Config(format!("Invalid portable update archive: {error}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AppError::Config(
            "Portable update archive contains too many entries".into(),
        ));
    }

    let mut found_exe = false;
    let mut found_mcp = false;
    let mut found_marker = false;
    let mut found_conpty = [false; CONPTY_FILES.len()];
    let mut payload_bytes = 0_u64;

    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            AppError::Config(format!("Failed to read portable update archive: {error}"))
        })?;
        let enclosed = entry.enclosed_name().ok_or_else(|| {
            AppError::Config("Portable update archive contains an unsafe path".into())
        })?;
        let mut components = enclosed.components();
        if components.next() != Some(Component::Normal(PORTABLE_ROOT.as_ref())) {
            return Err(AppError::Config(
                "Portable update archive has an unexpected root directory".into(),
            ));
        }
        let mut relative = PathBuf::new();
        for component in components {
            let Component::Normal(name) = component else {
                return Err(AppError::Config(
                    "Portable update archive contains an unsafe relative path".into(),
                ));
            };
            relative.push(name);
        }
        if relative.as_os_str().is_empty() || entry.is_dir() {
            continue;
        }
        if entry.is_symlink() {
            return Err(AppError::Config(
                "Portable update archive contains a symbolic link".into(),
            ));
        }

        if relative.starts_with("data") {
            continue;
        }

        let output = if relative == Path::new(PORTABLE_EXE) {
            if found_exe {
                return Err(AppError::Config(
                    "Portable update archive contains duplicate NyaTerm.exe entries".into(),
                ));
            }
            found_exe = true;
            destination.join(PORTABLE_EXE)
        } else if relative == Path::new(PORTABLE_MCP) {
            if found_mcp {
                return Err(AppError::Config(
                    "Portable update archive contains duplicate nyaterm-mcp.exe entries".into(),
                ));
            }
            found_mcp = true;
            destination.join(PORTABLE_MCP)
        } else if relative == Path::new(PORTABLE_MARKER) {
            if found_marker {
                return Err(AppError::Config(
                    "Portable update archive contains duplicate portable.flag entries".into(),
                ));
            }
            found_marker = true;
            destination.join(PORTABLE_MARKER)
        } else if let Some(index) = CONPTY_FILES
            .iter()
            .position(|allowed| relative == Path::new(allowed))
        {
            if found_conpty[index] {
                return Err(AppError::Config(format!(
                    "Portable update archive contains duplicate ConPTY file: {}",
                    relative.display()
                )));
            }
            found_conpty[index] = true;
            destination.join(&relative)
        } else {
            return Err(AppError::Config(format!(
                "Portable update archive contains an unexpected file: {}",
                relative.display()
            )));
        };

        payload_bytes = payload_bytes.saturating_add(entry.size());
        if payload_bytes > MAX_PAYLOAD_BYTES {
            return Err(AppError::Config(
                "Portable update archive is larger than the allowed limit".into(),
            ));
        }

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(output)?;
        let copied = std::io::copy(&mut entry.take(MAX_PAYLOAD_BYTES + 1), &mut file)?;
        if copied > MAX_PAYLOAD_BYTES {
            return Err(AppError::Config(
                "Portable update archive entry is larger than the allowed limit".into(),
            ));
        }
    }

    if !found_exe || !found_mcp || !found_marker || found_conpty.contains(&false) {
        return Err(AppError::Config(
            "Portable update archive is missing a required application or ConPTY file".into(),
        ));
    }
    Ok(())
}

pub fn run_helper_if_requested() -> bool {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).and_then(|arg| arg.to_str()) != Some(HELPER_FLAG) {
        return false;
    }

    if let Err(error) = run_helper(&args) {
        let target = args.get(4).map(PathBuf::from);
        if let Some(target_exe) = target.as_deref() {
            write_helper_error(target_exe, &error.to_string());
            let mut command = Command::new(target_exe);
            if let Some(work_dir) = args.get(5) {
                command.env(CLEANUP_ENV, work_dir);
            }
            let _ = command.spawn();
        }
    }
    true
}

fn run_helper(args: &[OsString]) -> AppResult<()> {
    if args.len() != 8 {
        return Err(AppError::Config(
            "Invalid portable update helper arguments".into(),
        ));
    }
    let parent_pid = args[2]
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| AppError::Config("Invalid portable update parent process ID".into()))?;
    let source_exe = PathBuf::from(&args[3]);
    let target_exe = PathBuf::from(&args[4]);
    let work_dir = PathBuf::from(&args[5]);
    let source_mcp = PathBuf::from(&args[6]);
    let source_conpty = PathBuf::from(&args[7]);

    wait_for_process_exit(parent_pid)?;
    replace_portable_files(&source_exe, &target_exe, &source_mcp, &source_conpty)?;
    Command::new(&target_exe)
        .env(CLEANUP_ENV, &work_dir)
        .spawn()?;
    Ok(())
}

fn replace_portable_files(
    source_exe: &Path,
    target_exe: &Path,
    source_mcp: &Path,
    source_conpty: &Path,
) -> AppResult<()> {
    let target_dir = target_exe
        .parent()
        .ok_or_else(|| AppError::Config("Portable executable has no parent directory".into()))?;
    let target_mcp = target_dir.join(PORTABLE_MCP);
    let new_exe = target_dir.join(".nyaterm-update-new.exe");
    let backup_exe = target_dir.join(".nyaterm-update-backup.exe");
    let new_mcp = target_dir.join(".nyaterm-mcp-update-new.exe");
    let target_conpty = target_dir.join("conpty");
    let new_conpty = target_dir.join(".nyaterm-conpty-update-new");
    let backup_conpty =
        target_dir.join(format!(".nyaterm-conpty-update-backup-{}", Uuid::new_v4()));
    cleanup_stale_mcp_backups(target_dir);
    let backup_mcp = target_dir.join(format!("{MCP_BACKUP_PREFIX}{}.exe", Uuid::new_v4()));
    let _ = fs::remove_file(&new_exe);
    let _ = fs::remove_file(&backup_exe);
    let _ = fs::remove_file(&new_mcp);
    if new_conpty.exists() {
        fs::remove_dir_all(&new_conpty)?;
    }
    fs::copy(source_exe, &new_exe)?;
    if let Err(error) = fs::copy(source_mcp, &new_mcp) {
        let _ = fs::remove_file(&new_exe);
        return Err(error.into());
    }
    if let Err(error) = stage_conpty_files(source_conpty, &new_conpty) {
        let _ = fs::remove_file(&new_exe);
        let _ = fs::remove_file(&new_mcp);
        let _ = fs::remove_dir_all(&new_conpty);
        return Err(error);
    }

    if target_conpty.exists() && !target_conpty.is_dir() {
        let _ = fs::remove_dir_all(&new_conpty);
        return Err(AppError::Config(format!(
            "Portable ConPTY target is not a directory: {}",
            target_conpty.display()
        )));
    }
    let had_conpty = target_conpty.is_dir();
    if had_conpty {
        fs::rename(&target_conpty, &backup_conpty)?;
    }
    if let Err(error) = fs::rename(&new_conpty, &target_conpty) {
        if had_conpty {
            let _ = fs::rename(&backup_conpty, &target_conpty);
        }
        return Err(error.into());
    }

    let result = commit_portable_files(
        &new_exe,
        target_exe,
        &backup_exe,
        &new_mcp,
        &target_mcp,
        &backup_mcp,
        |from, to| fs::rename(from, to),
    );
    let _ = fs::remove_file(&new_exe);
    let _ = fs::remove_file(&new_mcp);
    if let Err(error) = &result {
        let remove_result = fs::remove_dir_all(&target_conpty);
        let restore_result = if had_conpty {
            fs::rename(&backup_conpty, &target_conpty)
        } else {
            Ok(())
        };
        if remove_result.is_err() || restore_result.is_err() {
            return Err(AppError::Config(format!(
                "Portable update failed ({error}); ConPTY rollback failed: remove={remove_result:?}, restore={restore_result:?}"
            )));
        }
    }
    if result.is_ok() {
        let _ = fs::remove_file(&backup_exe);
        let _ = fs::remove_file(&backup_mcp);
        if had_conpty {
            let _ = fs::remove_dir_all(&backup_conpty);
        }
    }
    result
}

fn stage_conpty_files(source: &Path, destination: &Path) -> AppResult<()> {
    for path in CONPTY_FILES {
        let relative = Path::new(path).strip_prefix("conpty").unwrap();
        let target = destination.join(relative);
        fs::create_dir_all(target.parent().unwrap())?;
        fs::copy(source.join(relative), target)?;
    }
    Ok(())
}

fn cleanup_stale_mcp_backups(target_dir: &Path) {
    let _ = fs::remove_file(target_dir.join(LEGACY_MCP_BACKUP));
    let Ok(entries) = fs::read_dir(target_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(MCP_BACKUP_PREFIX) && name.ends_with(".exe") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn commit_portable_files<F>(
    new_exe: &Path,
    target_exe: &Path,
    backup_exe: &Path,
    new_mcp: &Path,
    target_mcp: &Path,
    backup_mcp: &Path,
    move_file: F,
) -> AppResult<()>
where
    F: Fn(&Path, &Path) -> std::io::Result<()>,
{
    move_file(target_exe, backup_exe)?;
    let had_mcp = target_mcp.is_file();
    if target_mcp.exists() && !had_mcp {
        let _ = fs::rename(backup_exe, target_exe);
        return Err(AppError::Config(format!(
            "Portable MCP target is not a file: {}",
            target_mcp.display()
        )));
    }

    if had_mcp && let Err(error) = move_file(target_mcp, backup_mcp) {
        if let Err(rollback_error) = fs::rename(backup_exe, target_exe) {
            return Err(AppError::Config(format!(
                "Failed to prepare nyaterm-mcp.exe for update ({error}) and restore NyaTerm.exe ({rollback_error})"
            )));
        }
        return Err(error.into());
    }

    if let Err(error) = move_file(new_exe, target_exe) {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = fs::rename(backup_exe, target_exe) {
            rollback_errors.push(format!("NyaTerm.exe: {rollback_error}"));
        }
        if had_mcp && let Err(rollback_error) = fs::rename(backup_mcp, target_mcp) {
            rollback_errors.push(format!("nyaterm-mcp.exe: {rollback_error}"));
        }
        if !rollback_errors.is_empty() {
            return Err(AppError::Config(format!(
                "Failed to install NyaTerm.exe ({error}) and restore the previous files ({})",
                rollback_errors.join(", ")
            )));
        }
        return Err(error.into());
    }

    if let Err(error) = move_file(new_mcp, target_mcp) {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = fs::remove_file(target_exe) {
            rollback_errors.push(format!("remove new NyaTerm.exe: {rollback_error}"));
        }
        if let Err(rollback_error) = fs::rename(backup_exe, target_exe) {
            rollback_errors.push(format!("restore NyaTerm.exe: {rollback_error}"));
        }
        if had_mcp && let Err(rollback_error) = fs::rename(backup_mcp, target_mcp) {
            rollback_errors.push(format!("restore nyaterm-mcp.exe: {rollback_error}"));
        }
        if !rollback_errors.is_empty() {
            return Err(AppError::Config(format!(
                "Failed to install nyaterm-mcp.exe ({error}) and restore the previous files ({})",
                rollback_errors.join(", ")
            )));
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(windows)]
fn wait_for_process_exit(process_id: u32) -> AppResult<()> {
    use windows::Win32::Foundation::{CloseHandle, E_INVALIDARG, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    let process = match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, process_id) } {
        Ok(process) => process,
        Err(error) if error.code() == E_INVALIDARG => return Ok(()),
        Err(error) => {
            return Err(AppError::Config(format!(
                "Failed to open parent process: {error}"
            )));
        }
    };
    let result = unsafe { WaitForSingleObject(process, 120_000) };
    let _ = unsafe { CloseHandle(process) };
    if result != WAIT_OBJECT_0 {
        return Err(AppError::Config(
            "Timed out waiting for NyaTerm to exit".into(),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn wait_for_process_exit(_process_id: u32) -> AppResult<()> {
    Err(AppError::Config(
        "Portable update helper is only supported on Windows".into(),
    ))
}

fn write_helper_error(target_exe: &Path, message: &str) {
    let Some(target_dir) = target_exe.parent() else {
        return;
    };
    let log_dir = target_dir.join("data").join("logs");
    if fs::create_dir_all(&log_dir).is_ok() {
        let _ = fs::write(log_dir.join("portable-update-error.log"), message);
    }
}

pub fn schedule_cleanup_from_environment() {
    let explicit_cleanup = std::env::var_os(CLEANUP_ENV).and_then(|raw_path| {
        unsafe {
            std::env::remove_var(CLEANUP_ENV);
        }
        let path = PathBuf::from(raw_path);
        is_portable_work_dir(&path).then_some(path)
    });

    std::thread::spawn(move || {
        if let Some(path) = explicit_cleanup {
            std::thread::sleep(Duration::from_secs(3));
            let _ = fs::remove_dir_all(path);
        }
        cleanup_stale_work_dirs();
    });
}

fn is_portable_work_dir(path: &Path) -> bool {
    path.parent() == Some(std::env::temp_dir().as_path())
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(WORK_DIR_PREFIX))
}

fn cleanup_stale_work_dirs() {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_stale_directory = entry.file_type().is_ok_and(|kind| kind.is_dir())
            && is_portable_work_dir(&path)
            && entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= STALE_WORK_DIR_AGE);
        if is_stale_directory {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("nyaterm-{name}-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        for (name, bytes) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn write_test_conpty(source_dir: &Path) -> PathBuf {
        let conpty = source_dir.join("conpty");
        for path in CONPTY_FILES {
            let relative = Path::new(path).strip_prefix("conpty").unwrap();
            let target = conpty.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, b"new-conpty").unwrap();
        }
        conpty
    }

    #[test]
    fn maps_supported_windows_architectures() {
        assert_eq!(
            portable_target_for_arch("x86_64").unwrap(),
            "windows-x86_64-portable"
        );
        assert_eq!(
            portable_target_for_arch("aarch64").unwrap(),
            "windows-aarch64-portable"
        );
        assert!(portable_target_for_arch("x86").is_err());
    }

    #[test]
    fn extracts_program_files_without_touching_data() {
        let destination = test_dir("portable-extract");
        let bytes = archive(&[
            ("NyaTerm-portable/NyaTerm.exe", b"new-exe"),
            ("NyaTerm-portable/nyaterm-mcp.exe", b"new-mcp"),
            ("NyaTerm-portable/portable.flag", b""),
            ("NyaTerm-portable/conpty/x64/conpty.dll", b"x64-dll"),
            (
                "NyaTerm-portable/conpty/x64/x64/OpenConsole.exe",
                b"x64-host",
            ),
            (
                "NyaTerm-portable/conpty/x64/arm64/OpenConsole.exe",
                b"arm64-host",
            ),
            ("NyaTerm-portable/conpty/arm64/conpty.dll", b"arm64-dll"),
            (
                "NyaTerm-portable/conpty/arm64/arm64/OpenConsole.exe",
                b"arm64-host",
            ),
            ("NyaTerm-portable/conpty/LICENSE.txt", b"license"),
            ("NyaTerm-portable/data/.keep", b"package-data"),
        ]);

        extract_portable_payload(&bytes, &destination).unwrap();

        assert_eq!(
            fs::read(destination.join(PORTABLE_EXE)).unwrap(),
            b"new-exe"
        );
        assert_eq!(
            fs::read(destination.join(PORTABLE_MCP)).unwrap(),
            b"new-mcp"
        );
        assert!(destination.join(PORTABLE_MARKER).is_file());
        assert_eq!(
            fs::read(destination.join("conpty/x64/conpty.dll")).unwrap(),
            b"x64-dll"
        );
        assert!(!destination.join("data").exists());
        fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn rejects_unsafe_or_incomplete_archives() {
        let destination = test_dir("portable-invalid");
        let unsafe_archive = archive(&[
            ("../NyaTerm.exe", b"bad"),
            ("NyaTerm-portable/nyaterm-mcp.exe", b"new-mcp"),
            ("NyaTerm-portable/portable.flag", b""),
        ]);
        assert!(extract_portable_payload(&unsafe_archive, &destination).is_err());

        let nested_escape = archive(&[
            ("NyaTerm-portable/NyaTerm.exe", b"new-exe"),
            ("NyaTerm-portable/nyaterm-mcp.exe", b"new-mcp"),
            ("NyaTerm-portable/portable.flag", b""),
            ("NyaTerm-portable/data/../../escape", b"bad"),
        ]);
        assert!(extract_portable_payload(&nested_escape, &destination).is_err());

        let missing_marker = archive(&[
            ("NyaTerm-portable/NyaTerm.exe", b"new-exe"),
            ("NyaTerm-portable/nyaterm-mcp.exe", b"new-mcp"),
        ]);
        assert!(extract_portable_payload(&missing_marker, &destination).is_err());

        let missing_mcp = archive(&[
            ("NyaTerm-portable/NyaTerm.exe", b"new-exe"),
            ("NyaTerm-portable/portable.flag", b""),
        ]);
        assert!(extract_portable_payload(&missing_mcp, &destination).is_err());
        let unexpected_conpty = archive(&[("NyaTerm-portable/conpty/other.dll", b"bad")]);
        assert!(extract_portable_payload(&unexpected_conpty, &destination).is_err());
        fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn portable_replace_installs_mcp_when_missing() {
        let directory = test_dir("portable-replace");
        let source_dir = directory.join("source");
        fs::create_dir(&source_dir).unwrap();
        let source_exe = source_dir.join(PORTABLE_EXE);
        let source_mcp = source_dir.join(PORTABLE_MCP);
        let target_exe = directory.join(PORTABLE_EXE);
        let target_mcp = directory.join(PORTABLE_MCP);
        fs::write(&source_exe, b"new-exe").unwrap();
        fs::write(&source_mcp, b"new-mcp").unwrap();
        fs::write(&target_exe, b"old-exe").unwrap();
        let source_conpty = write_test_conpty(&source_dir);

        replace_portable_files(&source_exe, &target_exe, &source_mcp, &source_conpty).unwrap();

        assert_eq!(fs::read(&target_exe).unwrap(), b"new-exe");
        assert_eq!(fs::read(&target_mcp).unwrap(), b"new-mcp");
        assert_eq!(
            fs::read(directory.join("conpty/x64/conpty.dll")).unwrap(),
            b"new-conpty"
        );
        assert!(!directory.join(".nyaterm-update-backup.exe").exists());
        assert!(!directory.join(".nyaterm-mcp-update-backup.exe").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn portable_replace_ignores_unremovable_stale_mcp_backup() {
        let directory = test_dir("portable-stale-mcp-backup");
        let source_dir = directory.join("source");
        fs::create_dir(&source_dir).unwrap();
        let source_exe = source_dir.join(PORTABLE_EXE);
        let source_mcp = source_dir.join(PORTABLE_MCP);
        let target_exe = directory.join(PORTABLE_EXE);
        let target_mcp = directory.join(PORTABLE_MCP);
        let stale_backup = directory.join(format!("{MCP_BACKUP_PREFIX}stale.exe"));
        fs::write(&source_exe, b"new-exe").unwrap();
        fs::write(&source_mcp, b"new-mcp").unwrap();
        fs::write(&target_exe, b"old-exe").unwrap();
        fs::write(&target_mcp, b"old-mcp").unwrap();

        // A directory makes remove_file fail on every platform, simulating a stale backup
        // that Windows cannot delete while an older sidecar process still has it open.
        fs::create_dir(&stale_backup).unwrap();
        let source_conpty = write_test_conpty(&source_dir);

        replace_portable_files(&source_exe, &target_exe, &source_mcp, &source_conpty).unwrap();

        assert_eq!(fs::read(&target_exe).unwrap(), b"new-exe");
        assert_eq!(fs::read(&target_mcp).unwrap(), b"new-mcp");
        assert!(stale_backup.is_dir());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn portable_replace_restores_conpty_when_mcp_target_is_invalid() {
        let directory = test_dir("portable-conpty-rollback");
        let source_dir = directory.join("source");
        fs::create_dir(&source_dir).unwrap();
        let source_exe = source_dir.join(PORTABLE_EXE);
        let source_mcp = source_dir.join(PORTABLE_MCP);
        let target_exe = directory.join(PORTABLE_EXE);
        fs::write(&source_exe, b"new-exe").unwrap();
        fs::write(&source_mcp, b"new-mcp").unwrap();
        fs::write(&target_exe, b"old-exe").unwrap();
        fs::create_dir(directory.join(PORTABLE_MCP)).unwrap();
        let source_conpty = write_test_conpty(&source_dir);
        let target_conpty = directory.join("conpty");
        fs::create_dir_all(target_conpty.join("x64")).unwrap();
        fs::write(target_conpty.join("x64/conpty.dll"), b"old-conpty").unwrap();

        assert!(
            replace_portable_files(&source_exe, &target_exe, &source_mcp, &source_conpty).is_err()
        );
        assert_eq!(fs::read(&target_exe).unwrap(), b"old-exe");
        assert_eq!(
            fs::read(target_conpty.join("x64/conpty.dll")).unwrap(),
            b"old-conpty"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn portable_commit_rolls_back_both_files_when_mcp_install_fails() {
        let directory = test_dir("portable-rollback");
        let target_exe = directory.join(PORTABLE_EXE);
        let target_mcp = directory.join(PORTABLE_MCP);
        let new_exe = directory.join("new.exe");
        let new_mcp = directory.join("new-mcp.exe");
        let backup_exe = directory.join("backup.exe");
        let backup_mcp = directory.join("backup-mcp.exe");
        fs::write(&target_exe, b"old-exe").unwrap();
        fs::write(&target_mcp, b"old-mcp").unwrap();
        fs::write(&new_exe, b"new-exe").unwrap();
        fs::write(&new_mcp, b"new-mcp").unwrap();

        let result = commit_portable_files(
            &new_exe,
            &target_exe,
            &backup_exe,
            &new_mcp,
            &target_mcp,
            &backup_mcp,
            |from, to| {
                if from == new_mcp {
                    Err(std::io::Error::other("simulated MCP install failure"))
                } else {
                    fs::rename(from, to)
                }
            },
        );

        assert!(result.is_err());
        assert_eq!(fs::read(&target_exe).unwrap(), b"old-exe");
        assert_eq!(fs::read(&target_mcp).unwrap(), b"old-mcp");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn portable_commit_restores_exe_when_first_mcp_install_fails() {
        let directory = test_dir("portable-first-mcp");
        let target_exe = directory.join(PORTABLE_EXE);
        let target_mcp = directory.join(PORTABLE_MCP);
        let new_exe = directory.join("new.exe");
        let new_mcp = directory.join("new-mcp.exe");
        let backup_exe = directory.join("backup.exe");
        let backup_mcp = directory.join("backup-mcp.exe");
        fs::write(&target_exe, b"old-exe").unwrap();
        fs::write(&new_exe, b"new-exe").unwrap();
        fs::write(&new_mcp, b"new-mcp").unwrap();

        let result = commit_portable_files(
            &new_exe,
            &target_exe,
            &backup_exe,
            &new_mcp,
            &target_mcp,
            &backup_mcp,
            |from, to| {
                if from == new_mcp {
                    Err(std::io::Error::other("simulated MCP install failure"))
                } else {
                    fs::rename(from, to)
                }
            },
        );

        assert!(result.is_err());
        assert_eq!(fs::read(&target_exe).unwrap(), b"old-exe");
        assert!(!target_mcp.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
