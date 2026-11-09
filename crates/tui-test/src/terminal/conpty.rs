//! Lazy loading of the pinned Microsoft ConPTY host on Windows.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use anyhow::{ensure, Context, Result};
use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use windows_sys::Win32::System::SystemInformation::{
    IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_I386,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};

use super::conpty_assets::{manifest, selected, Asset};

static BUNDLE: OnceLock<Result<LoadedBundle, String>> = OnceLock::new();
static TEMPORARY: AtomicU64 = AtomicU64::new(0);
const LICENSE: &[u8] = include_bytes!("../../assets/conpty/LICENSE");

struct Module(usize);

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0 as _) };
    }
}

struct LoadedBundle {
    _module: Module,
    _prepared: PreparedBundle,
}

struct PreparedBundle {
    dll: PathBuf,
    // Deny write/delete sharing while these images and directories are in use.
    _files: Vec<File>,
    _directories: Vec<File>,
}

/// Initialize the bundle before any portable-pty PTY is opened in this process.
pub fn initialize() -> Result<()> {
    match BUNDLE.get_or_init(|| load().map_err(|error| format!("{error:#}"))) {
        Ok(_) => Ok(()),
        Err(error) => anyhow::bail!("initialize bundled ConPTY: {error}"),
    }
}

/// The NuGet package version, available without loading or extracting the bundle.
pub fn package_version() -> &'static str {
    &manifest().version
}

/// The version recorded in the bundled binaries' Windows resources.
pub fn file_version() -> &'static str {
    &manifest().file_version
}

/// The name of the NuGet package containing the bundle.
pub fn package_name() -> &'static str {
    &manifest().package
}

fn load() -> Result<LoadedBundle> {
    let cache = dirs::cache_dir().context("cannot locate the user cache directory")?;
    let prepared = prepare(&cache)?;
    let path: Vec<u16> = prepared
        .dll
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let handle = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    ensure!(
        !handle.is_null(),
        "load {}: {}",
        prepared.dll.display(),
        std::io::Error::last_os_error()
    );
    let module = Module(handle as usize);
    // portable-pty opens the basename. It must resolve to the module we pinned.
    ensure!(
        unsafe { GetModuleHandleW(windows_sys::core::w!("conpty.dll")) } == handle,
        "another conpty.dll is already loaded; initialize tui-test's bundle first"
    );
    for name in [
        c"CreatePseudoConsole",
        c"ResizePseudoConsole",
        c"ClosePseudoConsole",
    ] {
        ensure!(
            unsafe { GetProcAddress(handle, name.as_ptr().cast()) }.is_some(),
            "bundled ConPTY is missing {}",
            name.to_string_lossy()
        );
    }
    Ok(LoadedBundle {
        _module: module,
        _prepared: prepared,
    })
}

fn prepare(cache: &Path) -> Result<PreparedBundle> {
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        other => anyhow::bail!("bundled ConPTY does not support {other}"),
    };
    let mut directories = Vec::new();
    let mut directory = cache.to_path_buf();
    for component in [
        "tui-test",
        "conpty",
        &format!("{}-{architecture}", package_version()),
    ] {
        directory.push(component);
        hold_directory(&directory, &mut directories)?;
    }
    let host = native_host()?;
    let mut files = Vec::new();
    for asset in selected().iter().chain(std::iter::once(&host)) {
        let path = directory.join(asset.path);
        if let Some(parent) = path.parent().filter(|parent| *parent != directory) {
            hold_directory(parent, &mut directories)?;
        }
        let file = match open_image(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if asset.path == "OpenConsole.exe" {
                    // ConPTY prefers this sibling over architecture subdirectories.
                    match std::fs::hard_link(directory.join(asset.key), &path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.into()),
                    }
                } else {
                    publish(&path, &asset.decode()?)?;
                }
                open_image(&path)?
            }
            Err(error) => return Err(error.into()),
        };
        asset
            .verify(&file)
            .with_context(|| format!("check {}", path.display()))?;
        files.push(file);
    }
    let license = directory.join("LICENSE");
    if !license.exists() {
        publish(&license, LICENSE)?;
    }
    let mut file = open_image(&license)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(LICENSE.len() as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes == LICENSE, "cached ConPTY license mismatch");
    file.rewind()?;
    files.push(file);
    Ok(PreparedBundle {
        dll: directory.join("conpty.dll"),
        _files: files,
        _directories: directories,
    })
}

fn native_host() -> Result<Asset> {
    let mut process_machine = 0;
    let mut native_machine = 0;
    ensure!(
        unsafe {
            IsWow64Process2(
                GetCurrentProcess(),
                &mut process_machine,
                &mut native_machine,
            )
        } != 0,
        "cannot determine native Windows architecture: {}",
        std::io::Error::last_os_error()
    );
    let key = match native_machine {
        IMAGE_FILE_MACHINE_AMD64 => "x64/OpenConsole.exe",
        IMAGE_FILE_MACHINE_ARM64 => "arm64/OpenConsole.exe",
        IMAGE_FILE_MACHINE_I386 => "x86/OpenConsole.exe",
        other => anyhow::bail!("unsupported native Windows machine {other:#x}"),
    };
    let asset = selected()
        .iter()
        .find(|asset| asset.key == key)
        .context("native ConPTY host is not included for this process architecture")?;
    Ok(Asset {
        key: asset.key,
        path: "OpenConsole.exe",
        compressed: asset.compressed,
    })
}

fn hold_directory(path: &Path, handles: &mut Vec<File>) -> Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = handle.metadata()?;
    ensure!(
        metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "ConPTY cache directory is not a regular directory: {}",
        path.display()
    );
    handles.push(handle);
    Ok(())
}

fn open_image(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(std::io::Error::other(
            "ConPTY cache file is not a regular file",
        ));
    }
    Ok(file)
}

fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    let (temporary, mut file) = loop {
        let name = format!(
            ".conpty-{}-{}",
            std::process::id(),
            TEMPORARY.fetch_add(1, Ordering::Relaxed)
        );
        let temporary = path.with_file_name(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        drop(file);
        match std::fs::hard_link(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    })();
    let cleanup = std::fs::remove_file(&temporary);
    result?;
    cleanup?;
    Ok(())
}

#[cfg(test)]
mod tests;
