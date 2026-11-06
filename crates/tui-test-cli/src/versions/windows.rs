use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use portable_pty::{native_pty_system, PtySize};
use serde_json::{json, Value};
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::Storage::FileSystem::{VerQueryValueW, VS_FIXEDFILEINFO};
use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, GetModuleFileNameW, GetModuleHandleW, LoadLibraryExW, LoadResource,
    LockResource, SizeofResource, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

pub(super) fn detect(dll: &str, host: &str) -> Result<Value> {
    // Let portable-pty choose and load ConPTY rather than approximating its
    // DLL search or mistaking an unloadable sideloaded DLL for the active one.
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 1,
            cols: 1,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("initialize portable-pty's ConPTY implementation")?;
    let dll: Vec<u16> = dll.encode_utf16().chain(Some(0)).collect();
    let module = unsafe { GetModuleHandleW(dll.as_ptr()) };
    let mut buffer = vec![0u16; 32768];
    let path = if module.is_null() {
        let length =
            unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        ensure!(
            length > 0 && length < buffer.len(),
            "cannot locate the Windows system directory"
        );
        PathBuf::from(OsString::from_wide(&buffer[..length])).join(host)
    } else {
        let length = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) }
            as usize;
        ensure!(
            length > 0 && length < buffer.len(),
            "cannot locate portable-pty's loaded ConPTY DLL"
        );
        PathBuf::from(OsString::from_wide(&buffer[..length]))
    };
    drop(pair);
    let version = file_version(&path)
        .with_context(|| format!("read ConPTY file version from {}", path.display()))?;
    Ok(json!({ "version": version, "source": path }))
}

fn file_version(path: &Path) -> Result<String> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    };
    ensure!(
        !module.is_null(),
        "load version resource: {}",
        std::io::Error::last_os_error()
    );
    let result = resource_version(module);
    unsafe { FreeLibrary(module) };
    result
}

fn resource_version(module: HMODULE) -> Result<String> {
    // Read VS_VERSION_INFO (ID 1, RT_VERSION 16) directly: GetFileVersionInfo
    // rewrites system file versions to 6.2 for executables without an OS manifest.
    let resource = unsafe { FindResourceW(module, 1usize as _, 16usize as _) };
    ensure!(!resource.is_null(), "missing version resource");
    let size = unsafe { SizeofResource(module, resource) } as usize;
    let handle = unsafe { LoadResource(module, resource) };
    let resource_data = unsafe { LockResource(handle) };
    ensure!(
        size > 0 && !resource_data.is_null(),
        "cannot read version resource"
    );
    let mut data = vec![0u32; size.div_ceil(4)];
    unsafe {
        std::ptr::copy_nonoverlapping(resource_data.cast::<u8>(), data.as_mut_ptr().cast(), size);
    }
    let mut info = std::ptr::null_mut();
    let mut length = 0;
    ensure!(
        unsafe {
            VerQueryValueW(
                data.as_ptr().cast(),
                windows_sys::core::w!("\\"),
                &mut info,
                &mut length,
            )
        } != 0
            && length as usize >= std::mem::size_of::<VS_FIXEDFILEINFO>(),
        "missing fixed file version information"
    );
    let info = std::ptr::NonNull::new(info.cast::<VS_FIXEDFILEINFO>())
        .context("missing fixed file version information")?;
    let info = unsafe { info.as_ptr().read_unaligned() };
    ensure!(
        info.dwSignature == 0xfeef04bd,
        "invalid file version signature"
    );
    Ok(format!(
        "{}.{}.{}.{}",
        info.dwFileVersionMS >> 16,
        info.dwFileVersionMS & 0xffff,
        info.dwFileVersionLS >> 16,
        info.dwFileVersionLS & 0xffff,
    ))
}
