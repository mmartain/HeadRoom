use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

/// Copy `src` to `dst` without blocking the owner from deleting/replacing `src`.
///
/// Windows `fs::copy` uses CopyFileEx without FILE_SHARE_DELETE, so a poll that
/// copies Cursor's `state.vscdb` can keep Cursor from reopening that DB on restart.
pub fn copy_shared(src: &Path, dst: &Path) -> std::io::Result<u64> {
    #[cfg(windows)]
    {
        copy_windows_shared(src, dst)
    }
    #[cfg(not(windows))]
    {
        std::fs::copy(src, dst)
    }
}

#[cfg(windows)]
fn copy_windows_shared(src: &Path, dst: &Path) -> std::io::Result<u64> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x00000001;
    const FILE_SHARE_WRITE: u32 = 0x00000002;
    const FILE_SHARE_DELETE: u32 = 0x00000004;
    const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x08000000;

    let mut from = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(src)?;
    let mut to = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dst)?;

    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        to.write_all(&buf[..n])?;
        total += n as u64;
    }
    to.flush()?;
    Ok(total)
}

/// Retry briefly — Cursor restart can return a sharing violation for a moment.
pub fn copy_shared_retry(src: &Path, dst: &Path, attempts: u32) -> std::io::Result<u64> {
    let attempts = attempts.max(1);
    let mut last = None;
    for i in 0..attempts {
        match copy_shared(src, dst) {
            Ok(n) => return Ok(n),
            Err(e) => {
                last = Some(e);
                if i + 1 < attempts {
                    thread::sleep(Duration::from_millis(40 * (i as u64 + 1)));
                }
            }
        }
    }
    Err(last.expect("attempts >= 1"))
}
