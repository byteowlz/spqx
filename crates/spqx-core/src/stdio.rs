//! Cross-platform CRT descriptor handling for clean JSON and binary stdout.
use std::io::{self, Write};

extern "C" {
    fn fflush(stream: *mut std::ffi::c_void) -> i32;
}

#[cfg(unix)]
extern "C" {
    fn dup(fd: i32) -> i32;
    fn dup2(old: i32, new: i32) -> i32;
    fn close(fd: i32) -> i32;
    fn write(fd: i32, data: *const u8, len: usize) -> isize;
}
#[cfg(windows)]
extern "C" {
    #[link_name = "_dup"]
    fn dup(fd: i32) -> i32;
    #[link_name = "_dup2"]
    fn dup2(old: i32, new: i32) -> i32;
    #[link_name = "_close"]
    fn close(fd: i32) -> i32;
    #[link_name = "_write"]
    fn crt_write(fd: i32, data: *const u8, len: u32) -> i32;
    fn _setmode(fd: i32, mode: i32) -> i32;
    fn _get_osfhandle(fd: i32) -> isize;
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetStdHandle(which: u32, handle: *mut std::ffi::c_void) -> i32;
}

fn sync_stdout_handle() -> io::Result<()> {
    #[cfg(windows)]
    unsafe {
        // Rust stdout uses the Win32 standard HANDLE, not the CRT descriptor.
        let handle = _get_osfhandle(1);
        if handle == -1 || SetStdHandle(-11_i32 as u32, handle as *mut _) == 0 {
            return Err(io::Error::other("cannot synchronize Windows stdout handle"));
        }
    }
    Ok(())
}

/// Owned CRT descriptor; unlike an OS HANDLE this is portable to Windows CRT I/O.
#[derive(Debug)]
pub struct Descriptor(i32);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            close(self.0);
        }
    }
}
impl Write for Descriptor {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        #[cfg(unix)]
        let count = unsafe { write(self.0, bytes.as_ptr(), bytes.len()) };
        #[cfg(windows)]
        let count = unsafe {
            crt_write(
                self.0,
                bytes.as_ptr(),
                bytes.len().min(u32::MAX as usize) as u32,
            )
        } as isize;
        if count < 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "CRT output write failed",
            ));
        }
        Ok(count as usize)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn save_stdout() -> io::Result<Descriptor> {
    io::stdout().flush()?;
    unsafe {
        fflush(std::ptr::null_mut());
    }
    let fd = unsafe { dup(1) };
    if fd < 0 {
        return Err(io::Error::other("cannot duplicate stdout"));
    }
    Ok(Descriptor(fd))
}
fn redirect() -> io::Result<()> {
    if unsafe { dup2(2, 1) } < 0 {
        return Err(io::Error::other("cannot redirect engine stdout"));
    }
    sync_stdout_handle()
}
/// Preserve the original stdout for frames, redirect model chatter to stderr.
/// The persistent worker intentionally retains the redirect until process exit.
pub fn framed_stdout() -> io::Result<Descriptor> {
    let saved = save_stdout()?;
    #[cfg(windows)]
    unsafe {
        // Text mode would corrupt PCM/frame bytes containing line feeds.
        if _setmode(saved.0, 0x8000) < 0 || _setmode(0, 0x8000) < 0 {
            return Err(io::Error::other("cannot enable binary worker I/O"));
        }
    }
    redirect()?;
    Ok(saved)
}
/// Scoped redirect used by the sequential CLI synthesis path.
#[derive(Debug)]
pub struct StdoutGuard(Descriptor);
impl StdoutGuard {
    /// Redirect engine progress for the lifetime of this guard.
    pub fn new() -> io::Result<Self> {
        let saved = save_stdout()?;
        redirect()?;
        Ok(Self(saved))
    }
}
impl Drop for StdoutGuard {
    fn drop(&mut self) {
        let _ = io::stdout().flush();
        unsafe {
            fflush(std::ptr::null_mut());
            dup2(self.0 .0, 1);
        }
        let _ = sync_stdout_handle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_redirect_catches_rust_and_crt_and_restores_stdout() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "stdio::tests::stdio_child",
                "--ignored",
                "--nocapture",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stdout.contains("RUST_ENGINE_NOISE") && !stdout.contains("C_ENGINE_NOISE"));
        assert!(stderr.contains("RUST_ENGINE_NOISE") && stderr.contains("C_ENGINE_NOISE"));
        assert!(stdout.contains("RESTORED_STDOUT"));
    }
    #[test]
    #[ignore = "subprocess helper, invoked by scoped_redirect test"]
    fn stdio_child() {
        extern "C" {
            fn puts(value: *const std::ffi::c_char) -> i32;
        }
        {
            let _guard = StdoutGuard::new().unwrap();
            io::stdout().write_all(b"RUST_ENGINE_NOISE\n").unwrap();
            unsafe {
                puts(c"C_ENGINE_NOISE".as_ptr());
            }
        }
        io::stdout().write_all(b"RESTORED_STDOUT\n").unwrap();
    }
}
