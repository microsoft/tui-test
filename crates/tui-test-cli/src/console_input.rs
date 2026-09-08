use std::io::{self, Cursor, Read};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Console::{GetStdHandle, ReadConsoleW, STD_INPUT_HANDLE};

pub(crate) struct ConsoleInput {
    handle: HANDLE,
    bytes: Cursor<Vec<u8>>,
    high_surrogate: Option<u16>,
}

impl ConsoleInput {
    pub(crate) fn new() -> Self {
        Self {
            handle: unsafe { GetStdHandle(STD_INPUT_HANDLE) },
            bytes: Cursor::new(Vec::new()),
            high_surrogate: None,
        }
    }
}

impl Read for ConsoleInput {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            let read = self.bytes.read(buffer)?;
            if read != 0 {
                return Ok(read);
            }

            let mut utf16 = [0u16; 1024];
            let offset = if let Some(high) = self.high_surrogate.take() {
                utf16[0] = high;
                1
            } else {
                0
            };
            let mut count = 0;
            // std::io::Stdin strips a trailing Ctrl+Z on Windows. ReadConsoleW
            // preserves it, and avoids depending on the console's code page.
            let success = unsafe {
                ReadConsoleW(
                    self.handle,
                    utf16[offset..].as_mut_ptr().cast(),
                    (utf16.len() - offset) as u32,
                    &mut count,
                    std::ptr::null(),
                )
            };
            if success == 0 {
                self.high_surrogate = (offset != 0).then_some(utf16[0]);
                return Err(io::Error::last_os_error());
            }
            if count == 0 {
                return if offset == 0 {
                    Ok(0)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "incomplete console surrogate pair",
                    ))
                };
            }
            let mut end = count as usize + offset;
            if matches!(utf16[end - 1], 0xd800..=0xdbff) {
                self.high_surrogate = Some(utf16[end - 1]);
                end -= 1;
            }
            let text = String::from_utf16(&utf16[..end])
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            self.bytes = Cursor::new(text.into_bytes());
        }
    }
}
