use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use portable_pty::{native_pty_system, PtySize};
use serde_json::{json, Value};
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
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
    let mut data = vec![0u8; size];
    unsafe {
        std::ptr::copy_nonoverlapping(resource_data.cast::<u8>(), data.as_mut_ptr(), size);
    }
    version_from_resource(&data)
}

fn version_from_resource(data: &[u8]) -> Result<String> {
    const KEY: &[u8] = b"V\0S\0_\0V\0E\0R\0S\0I\0O\0N\0_\0I\0N\0F\0O\0\0\0";
    const VALUE_OFFSET: usize = 40;
    const VALUE_LENGTH: usize = 52;
    ensure!(
        data.len() >= VALUE_OFFSET + VALUE_LENGTH,
        "truncated file version resource"
    );
    let length = u16::from_le_bytes(data[..2].try_into().unwrap()) as usize;
    let value_length = u16::from_le_bytes(data[2..4].try_into().unwrap()) as usize;
    let kind = u16::from_le_bytes(data[4..6].try_into().unwrap());
    ensure!(
        length <= data.len()
            && length >= VALUE_OFFSET + VALUE_LENGTH
            && value_length == VALUE_LENGTH
            && kind == 0
            && &data[6..6 + KEY.len()] == KEY,
        "invalid file version resource header"
    );
    let info = &data[VALUE_OFFSET..VALUE_OFFSET + VALUE_LENGTH];
    let word = |offset| u32::from_le_bytes(info[offset..offset + 4].try_into().unwrap());
    ensure!(word(0) == 0xfeef04bd, "invalid file version signature");
    let ms = word(8);
    let ls = word(12);
    Ok(format!(
        "{}.{}.{}.{}",
        ms >> 16,
        ms & 0xffff,
        ls >> 16,
        ls & 0xffff,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource() -> Vec<u8> {
        let mut data = vec![0u8; 92];
        data[..2].copy_from_slice(&92u16.to_le_bytes());
        data[2..4].copy_from_slice(&52u16.to_le_bytes());
        for (index, unit) in "VS_VERSION_INFO\0".encode_utf16().enumerate() {
            data[6 + index * 2..8 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        data[40..44].copy_from_slice(&0xfeef04bdu32.to_le_bytes());
        data[48..52].copy_from_slice(&(10u32 << 16).to_le_bytes());
        data[52..56].copy_from_slice(&((26100u32 << 16) | 9549).to_le_bytes());
        data
    }

    #[test]
    fn version_resource_is_read_within_its_declared_bounds() {
        let data = resource();
        assert_eq!(version_from_resource(&data).unwrap(), "10.0.26100.9549");
        // VerQueryValueW can accept this header and return a value outside it.
        assert!(version_from_resource(&data[..40]).is_err());
        assert!(version_from_resource(&data[..6]).is_err());
    }

    #[test]
    fn version_resource_rejects_malformed_headers() {
        for (offset, value) in [(0, 93u16), (0, 80), (2, 53), (4, 1), (6, 0)] {
            let mut data = resource();
            data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            assert!(version_from_resource(&data).is_err());
        }
        let mut data = resource();
        data[40] = 0;
        assert!(version_from_resource(&data).is_err());
    }
}
