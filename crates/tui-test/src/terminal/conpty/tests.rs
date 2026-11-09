use super::*;
use crate::terminal::integration::CommandTracker;
use crate::terminal::pty::{Pty, SpawnOptions};
use std::os::windows::ffi::OsStringExt;
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, SetConsoleMode, DISABLE_NEWLINE_AUTO_RETURN,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tui-test-conpty-{}-{} space '\u{96ea}'",
            std::process::id(),
            TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn conpty_cache_is_reusable_and_rejects_tampering() {
    let root = TestDirectory::new();
    let first = prepare(&root.0).unwrap();
    let path = first.dll.clone();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let second = prepare(&root.0).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    assert!(std::fs::write(&path, "cannot replace a held image").is_err());
    drop((first, second));
    std::fs::write(&path, "corrupt image").unwrap();
    let error = prepare(&root.0).err().expect("reject the changed image");
    assert!(error.to_string().contains("check"));
}

#[test]
fn conpty_cache_can_be_published_concurrently() {
    let root = TestDirectory::new();
    let start = Arc::new(Barrier::new(4));
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let start = start.clone();
            let root = &root.0;
            scope.spawn(move || {
                start.wait();
                let prepared = prepare(root).unwrap();
                assert!(prepared.dll.is_file());
            });
        }
    });
}

#[test]
fn conpty_cache_rejects_a_directory_in_place_of_an_image() {
    let root = TestDirectory::new();
    let first = prepare(&root.0).unwrap();
    let path = first.dll.clone();
    drop(first);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(prepare(&root.0).is_err());
}

#[test]
fn conpty_preferred_host_is_verified_and_cannot_be_replaced() {
    let root = TestDirectory::new();
    let prepared = prepare(&root.0).unwrap();
    let sibling = prepared.dll.with_file_name("OpenConsole.exe");
    let host = native_host().unwrap();
    assert_eq!(std::fs::read(&sibling).unwrap(), host.decode().unwrap());
    assert!(std::fs::write(&sibling, "replacement host").is_err());
    assert!(std::fs::remove_file(&sibling).is_err());
    let replacement = root.0.join("replacement.exe");
    std::fs::write(&replacement, "replacement host").unwrap();
    assert!(std::fs::rename(&replacement, &sibling).is_err());
    drop(prepared);
    // Preserve the valid architecture-directory image while replacing only the sibling.
    std::fs::remove_file(&sibling).unwrap();
    std::fs::write(&sibling, "unchecked sibling").unwrap();
    assert!(prepare(&root.0).is_err());
}

#[test]
fn conpty_rejects_a_reparse_point_in_the_preferred_host_location() {
    let root = TestDirectory::new();
    let prepared = prepare(&root.0).unwrap();
    let sibling = prepared.dll.with_file_name("OpenConsole.exe");
    drop(prepared);
    std::fs::remove_file(&sibling).unwrap();
    let target = root.0.join("junction-target");
    std::fs::create_dir(&target).unwrap();
    let result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&sibling)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(prepare(&root.0).is_err());
}

struct ProcessHandle(usize);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0 as _) };
    }
}

fn verify_launched_host(expected: &Path) -> Result<()> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    ensure!(
        snapshot != INVALID_HANDLE_VALUE,
        "cannot inspect child processes"
    );
    let _snapshot = ProcessHandle(snapshot as usize);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    ensure!(
        unsafe { Process32FirstW(snapshot, &mut entry) } != 0,
        "empty process snapshot"
    );
    let mut found = false;
    loop {
        let end = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if entry.th32ParentProcessID == std::process::id()
            && name.eq_ignore_ascii_case("OpenConsole.exe")
        {
            let handle =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
            ensure!(!handle.is_null(), "cannot inspect the console host");
            let _process = ProcessHandle(handle as usize);
            let mut buffer = vec![0u16; 32768];
            let mut length = buffer.len() as u32;
            ensure!(
                unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) }
                    != 0,
                "cannot locate the console host"
            );
            let actual = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
            ensure!(
                actual.canonicalize()? == expected.canonicalize()?,
                "unexpected console host: {}",
                actual.display()
            );
            found = true;
        }
        if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
            break;
        }
    }
    ensure!(found, "the bundled console host was not launched");
    Ok(())
}

#[test]
fn conpty_rejects_an_already_loaded_foreign_dll() {
    const PROBE: &str = "TUI_TEST_CONPTY_CONFLICT_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let root = TestDirectory::new();
        let prepared = prepare(&root.0).unwrap();
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
        assert!(!handle.is_null());
        let _foreign = Module(handle as usize);
        let error = initialize().expect_err("reject the foreign module");
        assert!(error.to_string().contains("another conpty.dll"));
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "terminal::conpty::tests::conpty_rejects_an_already_loaded_foreign_dll",
            "--nocapture",
        ])
        .env(PROBE, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn output(frame: usize) -> String {
    let mut output = (0..128)
        .map(|line| format!("frame-{frame}-line-{line:03}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    output.push_str(&format!("\r\nframe-{frame}-tail"));
    output
}

#[test]
fn conpty_marker_producer() {
    if std::env::var_os("TUI_TEST_CONPTY_PRODUCER").is_none() {
        return;
    }
    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    let mut mode = 0;
    assert_ne!(unsafe { GetConsoleMode(handle, &mut mode) }, 0);
    assert_ne!(
        unsafe {
            SetConsoleMode(
                handle,
                mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING | DISABLE_NEWLINE_AUTO_RETURN,
            )
        },
        0
    );
    let mut stdout = std::io::stdout().lock();
    for frame in 0..12 {
        let osc = if frame < 6 { 133 } else { 633 };
        let data = format!(
            "\x1b]{osc};A\x07\x1b]{osc};B\x07command-{frame}\
             \x1b]{osc};C\x07{}\x1b]{osc};D;0\x07\
             \x1b]{osc};A\x07prompt-{frame}\x1b]{osc};B\x07",
            output(frame)
        );
        stdout.write_all(data.as_bytes()).unwrap();
        stdout.flush().unwrap();
    }
}

#[test]
fn conpty_preserves_text_before_finish_and_prompt_markers() {
    let executable = std::env::current_exe().unwrap();
    let args = [
        "--exact",
        "terminal::conpty::tests::conpty_marker_producer",
        "--nocapture",
    ]
    .map(String::from);
    let (pty, mut reader) = Pty::spawn(
        executable.to_str().unwrap(),
        &args,
        &SpawnOptions {
            cols: 120,
            rows: 30,
            cwd: None,
            env: vec![("TUI_TEST_CONPTY_PRODUCER".into(), "1".into())],
        },
    )
    .unwrap();
    let (sender, receiver) = mpsc::channel();
    let reading = std::thread::spawn(move || {
        let mut bytes = [0; 17];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || sender.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let result = (|| {
        let bundle = BUNDLE.get().unwrap().as_ref().unwrap();
        verify_launched_host(&bundle._prepared.dll.with_file_name("OpenConsole.exe"))?;
        let mut tracker = CommandTracker::new();
        let mut transcript = Vec::new();
        let mut completed = 0;
        let deadline = Instant::now() + Duration::from_secs(30);
        while completed < 12 || !tracker.is_ready() {
            let chunk =
                receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
            let query_start = transcript.len().saturating_sub(3);
            transcript.extend_from_slice(&chunk);
            for query in transcript[query_start..].windows(4) {
                if query == b"\x1b[6n" {
                    pty.write(b"\x1b[1;1R")?;
                }
            }
            for byte in chunk {
                tracker.feed(std::slice::from_ref(&byte));
                if tracker.finished_count() > completed {
                    let expected = output(completed as usize).replace("\r\n", "\n");
                    ensure!(
                        tracker.last_output() == Some(expected.as_str()),
                        "frame {completed} completed before all text arrived: {:?}",
                        tracker.last_output()
                    );
                    ensure!(tracker.last_exit() == Some(0), "lost command exit code");
                    completed += 1;
                }
            }
        }
        ensure!(tracker.is_ready(), "lost final prompt marker");
        let loaded = unsafe { GetModuleHandleW(windows_sys::core::w!("conpty.dll")) };
        let bundle = BUNDLE.get().unwrap().as_ref().unwrap();
        ensure!(loaded as usize == bundle._module.0, "wrong ConPTY module");
        Ok::<_, anyhow::Error>(())
    })();
    pty.close();
    reading.join().unwrap();
    result.unwrap();
}
