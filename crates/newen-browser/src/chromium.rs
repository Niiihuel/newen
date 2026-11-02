//! Finding and launching Chromium with its DevTools pipe (docs/browser.md §2): Chromium reads
//! commands on fd 3 and writes replies and events on fd 4, each a JSON text ended by a NUL.

use std::fs::File;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Child;
#[cfg(unix)]
use std::process::{Command, Stdio};

/// The names Chromium goes by on Linux, most specific first.
const NAMES: [&str; 5] = [
    "chromium",
    "chromium-browser",
    "google-chrome-stable",
    "google-chrome",
    "chrome",
];

/// The Chromium to run: `QUILL_CHROME`, else the one Settings names, else the first on `PATH`.
pub fn find(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("QUILL_CHROME").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    if let Some(path) = configured {
        return Some(path.to_owned());
    }
    // Where the other systems install it; the pipe itself is Unix-only (see `launch`).
    let installed = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    ]
    .map(PathBuf::from);
    if let Some(found) = installed.into_iter().find(|candidate| candidate.is_file()) {
        return Some(found);
    }
    let path = std::env::var_os("PATH")?;
    NAMES.iter().find_map(|name| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// A running Chromium and its end of the pipe.
pub struct Process {
    pub child: Child,
    /// Where Chromium's replies and events are read.
    pub reader: File,
    /// Where commands to it are written.
    pub writer: File,
}

/// The switches Playwright launches Chromium with to keep it quiet and steady, less the
/// automation ones (chromiumSwitches.ts): no first-run or default-browser prompts, no
/// background traffic to Google, no throttling of pages it does not know are on screen.
#[cfg(unix)]
const QUIET: [&str; 12] = [
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-default-apps",
    "--disable-breakpad",
    "--disable-sync",
    "--disable-renderer-backgrounding",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-features=Translate,MediaRouter,OptimizationHints",
    "--password-store=basic",
];

/// Launch `program` headless with its profile in `profile` and pages drawn at `scale`
/// device pixels per CSS pixel (Chromium takes one scale for the whole process).
/// Windows would need handles passed by number (`--remote-debugging-io-pipes`), not done.
#[cfg(not(unix))]
pub fn launch(_: &Path, _: &Path, _: f32) -> std::io::Result<Process> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "quill's browser does not drive Chromium on this system yet",
    ))
}

#[cfg(unix)]
pub fn launch(program: &Path, profile: &Path, scale: f32) -> std::io::Result<Process> {
    let (ours_read, theirs_write) = pipe()?;
    let (theirs_read, ours_write) = pipe()?;
    // The pipe's ends are first moved above 4, so putting them on 3 and 4 in the child never
    // lands one on itself, which would leave it close-on-exec and Chromium without it.
    let (theirs_read, theirs_write) = (high(theirs_read)?, high(theirs_write)?);
    let (read_fd, write_fd) = (theirs_read.as_raw_fd(), theirs_write.as_raw_fd());
    let mut command = Command::new(program);
    command
        .arg("--headless")
        .arg("--remote-debugging-pipe")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--force-device-scale-factor={scale}"))
        .args(QUIET)
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(read_fd, 3) < 0 || libc::dup2(write_fd, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Chromium goes with quill even when quill is killed. Linux sends it when the
            // forking thread ends: one of the engine's runtime threads, which live as long.
            #[cfg(target_os = "linux")]
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    let child = command.spawn()?;
    // The child holds its own copies now.
    drop((theirs_read, theirs_write));
    Ok(Process {
        child,
        reader: File::from(ours_read),
        writer: File::from(ours_write),
    })
}

#[cfg(unix)]
fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: `fds` has room for the two descriptors pipe2 writes.
    #[cfg(target_os = "linux")]
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // macOS has no pipe2: close-on-exec is set after, a window in which a fork could inherit.
    #[cfg(not(target_os = "linux"))]
    // SAFETY: as above, and fcntl on the two descriptors just opened.
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) < 0 {
            return Err(std::io::Error::last_os_error());
        }
        for fd in fds {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    // SAFETY: both were just opened and belong to nothing else.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

#[cfg(unix)]
/// `fd` again as a descriptor numbered 10 or more, close-on-exec.
fn high(fd: OwnedFd) -> std::io::Result<OwnedFd> {
    // SAFETY: duplicating an open descriptor.
    let raised = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    if raised < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: fcntl just opened it.
    Ok(unsafe { OwnedFd::from_raw_fd(raised) })
}
