use super::*;
use crate::terminal::integration::CommandTracker;
use crate::terminal::pty::{Pty, SpawnOptions};
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, SetConsoleMode, DISABLE_NEWLINE_AUTO_RETURN,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_OUTPUT_HANDLE,
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
