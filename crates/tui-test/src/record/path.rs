use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

pub(super) struct ProtectedFile {
    path: PathBuf,
    identity: Option<(u64, u64)>,
}

impl ProtectedFile {
    pub fn new(path: &Path, file: &File) -> io::Result<Self> {
        Ok(Self {
            path: std::fs::canonicalize(path)?,
            identity: identity(file)?,
        })
    }

    pub fn check_path(&self, path: &Path) -> io::Result<()> {
        match std::fs::canonicalize(path) {
            Ok(path) => self.check(&path, identity_at(&path)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn check_file(&self, path: &Path, file: &File) -> io::Result<()> {
        self.check(&std::fs::canonicalize(path)?, identity(file)?)
    }

    fn check(&self, path: &Path, identity: Option<(u64, u64)>) -> io::Result<()> {
        if path == self.path || self.identity.is_some() && self.identity == identity {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "manual recording must not overwrite the automatic recording",
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(unix)]
fn identity(file: &File) -> io::Result<Option<(u64, u64)>> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(Some((metadata.dev(), metadata.ino())))
}

#[cfg(unix)]
fn identity_at(path: &Path) -> io::Result<Option<(u64, u64)>> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    Ok(Some((metadata.dev(), metadata.ino())))
}

#[cfg(windows)]
fn identity(file: &File) -> io::Result<Option<(u64, u64)>> {
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    #[derive(Default)]
    struct FileInformation {
        attributes: u32,
        creation_time: [u32; 2],
        last_access_time: [u32; 2],
        last_write_time: [u32; 2],
        volume_serial_number: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(
            file: *mut std::ffi::c_void,
            information: *mut FileInformation,
        ) -> i32;
    }

    let mut information = FileInformation::default();
    // The repr(C) buffer matches BY_HANDLE_FILE_INFORMATION and remains live
    // for the call; the borrowed File keeps its handle open.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(1 | 50) => Ok(None),
            _ => Err(error),
        };
    }
    Ok(Some((
        u64::from(information.volume_serial_number),
        u64::from(information.index_high) << 32 | u64::from(information.index_low),
    )))
}

#[cfg(windows)]
fn identity_at(path: &Path) -> io::Result<Option<(u64, u64)>> {
    use std::os::windows::fs::OpenOptionsExt;

    let file = File::options().read(true).access_mode(0).open(path)?;
    identity(&file)
}

#[cfg(not(any(unix, windows)))]
fn identity(_: &File) -> io::Result<Option<(u64, u64)>> {
    Ok(None)
}

#[cfg(not(any(unix, windows)))]
fn identity_at(_: &Path) -> io::Result<Option<(u64, u64)>> {
    Ok(None)
}
