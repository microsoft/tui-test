use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::Path;

pub(crate) fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let path = std::path::absolute(path)?;
    let path = if path.is_symlink() {
        path.canonicalize()?
    } else {
        path
    };
    let permissions = match fs::metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "screenshot output must be a regular file",
                )
                .into());
            }
            let permissions = metadata.permissions();
            if permissions.readonly() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "screenshot output is read-only",
                )
                .into());
            }
            Some(permissions)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "output has no parent"))?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".tui-test-").suffix(".tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Match File::create's permissions, including the caller's umask.
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut temporary = builder.tempfile_in(parent)?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    write_and_flush(&mut BufWriter::new(temporary.as_file_mut()), write)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path).map_err(|error| error.error)?;
    Ok(())
}

fn write_and_flush(
    output: &mut dyn Write,
    write: impl FnOnce(&mut dyn Write) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    write(output)?;
    output.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingWriter<'a> {
        output: &'a mut dyn Write,
        fail_flush: bool,
        written: bool,
    }

    impl Write for FailingWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.fail_flush && self.written {
                return Err(io::Error::other("injected write failure"));
            }
            self.written = true;
            self.output.write(&bytes[..bytes.len().min(3)])
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                Err(io::Error::other("injected flush failure"))
            } else {
                self.output.flush()
            }
        }
    }

    #[test]
    fn output_is_replaced_only_after_the_write_completes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("screen.svg");
        fs::write(&path, "previous image").unwrap();

        write_atomic(&path, |output| {
            output.write_all(b"replacement image")?;
            assert_eq!(fs::read_to_string(&path).unwrap(), "previous image");
            Ok(())
        })
        .unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement image");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn write_and_flush_failures_do_not_publish_partial_output() {
        for extension in ["svg", "png"] {
            for existing in [false, true] {
                for fail_flush in [false, true] {
                    let directory = tempfile::tempdir().unwrap();
                    let path = directory.path().join(format!("screen.{extension}"));
                    if existing {
                        fs::write(&path, "previous image").unwrap();
                    }

                    let error = write_atomic(&path, |output| {
                        write_and_flush(
                            &mut FailingWriter {
                                output,
                                fail_flush,
                                written: false,
                            },
                            |output| {
                                output.write_all(b"replacement image")?;
                                Ok(())
                            },
                        )
                    })
                    .unwrap_err();

                    let failure = if fail_flush { "flush" } else { "write" };
                    assert_eq!(error.to_string(), format!("injected {failure} failure"));
                    if existing {
                        assert_eq!(fs::read_to_string(&path).unwrap(), "previous image");
                    } else {
                        assert!(!path.exists());
                    }
                    assert_eq!(
                        fs::read_dir(directory.path()).unwrap().count(),
                        usize::from(existing)
                    );
                }
            }
        }
    }

    #[test]
    fn failed_replacement_cleans_up_the_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("screen.svg");
        write_atomic(&path, |output| {
            output.write_all(b"replacement image")?;
            fs::create_dir(&path)?;
            fs::write(path.join("retained"), "keep this")?;
            Ok(())
        })
        .unwrap_err();

        assert_eq!(
            fs::read_to_string(path.join("retained")).unwrap(),
            "keep this"
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn readonly_output_is_not_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("screen.svg");
        fs::write(&path, "previous image").unwrap();
        let original = fs::metadata(&path).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly).unwrap();

        let result = write_atomic(&path, |output| {
            output.write_all(b"replacement image")?;
            Ok(())
        });
        fs::set_permissions(&path, original).unwrap();

        assert_eq!(
            result
                .unwrap_err()
                .downcast_ref::<io::Error>()
                .unwrap()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "previous image");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn unwinding_does_not_publish_partial_output() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("screen.svg");
        fs::write(&path, "previous image").unwrap();
        let result = std::panic::catch_unwind(|| {
            let _ = write_atomic(&path, |output| {
                output.write_all(b"partial image")?;
                panic!("injected render panic");
            });
        });

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "previous image");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn output_preserves_permissions_and_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("screen.svg");
        let control = directory.path().join("control.svg");
        fs::write(&control, "control image").unwrap();
        write_atomic(&path, |output| {
            output.write_all(b"first image")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode(),
            fs::metadata(&control).unwrap().permissions().mode()
        );

        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let link = directory.path().join("link.svg");
        symlink("screen.svg", &link).unwrap();
        write_atomic(&link, |output| {
            output.write_all(b"replacement image")?;
            Ok(())
        })
        .unwrap();
        assert!(link.is_symlink());
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement image");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 3);
    }
}
