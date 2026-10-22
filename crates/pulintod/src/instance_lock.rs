//! Cross-session ownership of the physical keyboard input path.
//!
//! A D-Bus name only makes the daemon unique inside one session bus. The same
//! user can have a local desktop, VNC and a manually launched process, each on
//! a different bus, while `/dev/input/event*` is shared by all of them. Without
//! a process-wide lock, an old daemon can keep the keyboards grabbed while the
//! freshly installed one starts successfully but receives no input.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

pub struct InputInstanceLock {
    file: File,
}

impl InputInstanceLock {
    pub fn acquire() -> io::Result<Self> {
        let path = lock_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;

        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self { file })
    }
}

impl Drop for InputInstanceLock {
    fn drop(&mut self) {
        // Releasing is automatic when the process exits; doing it explicitly
        // also makes orderly shutdown and tests unambiguous.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn lock_path() -> PathBuf {
    // `/run/user/<uid>` is shared by all graphical sessions of one user and is
    // removed at logout. That is exactly the lifetime and scope we need; an
    // XDG_RUNTIME_DIR supplied by a nested/VNC session is not necessarily the
    // same directory and therefore cannot be the source of truth here.
    PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })).join("pulintod-input.lock")
}

#[cfg(test)]
mod tests {
    use super::lock_path;

    #[test]
    fn lock_is_scoped_to_the_unix_user_not_the_display() {
        let expected_parent = format!("/run/user/{}", unsafe { libc::geteuid() });
        assert_eq!(
            lock_path().parent().and_then(|path| path.to_str()),
            Some(expected_parent.as_str())
        );
    }
}
