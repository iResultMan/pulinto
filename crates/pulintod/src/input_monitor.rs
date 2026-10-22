//! Event-driven discovery of input devices through kernel uevents.
//!
//! Device nodes created by devtmpfs do not reliably produce inotify events.
//! The kernel does, however, multicast every device add/remove through the
//! kobject-uevent netlink socket. The reader sleeps in `recv(2)` until a real
//! input event occurs, so there is no timer and no idle CPU cost.

use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};

pub struct InputMonitor {
    socket: File,
}

impl InputMonitor {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
                libc::NETLINK_KOBJECT_UEVENT,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let socket = unsafe { File::from_raw_fd(fd) };

        let mut address: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        address.nl_family = libc::AF_NETLINK as libc::sa_family_t;
        // Zero lets the kernel pick a unique port: the process may hold more
        // than one of these (keyboards and, under Wayland, pointer devices),
        // and a second bind to the pid itself fails with EADDRINUSE.
        address.nl_pid = 0;
        address.nl_groups = 1;
        let result = unsafe {
            libc::bind(
                socket.as_raw_fd(),
                (&address as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { socket })
    }

    /// Block until the kernel announces a change to an input event device.
    pub fn wait(&mut self) -> io::Result<()> {
        let mut packet = [0u8; 8192];
        loop {
            let read = unsafe {
                libc::recv(
                    self.socket.as_raw_fd(),
                    packet.as_mut_ptr().cast(),
                    packet.len(),
                    0,
                )
            };
            if read > 0 {
                if relevant(&packet[..read as usize]) {
                    return Ok(());
                }
                continue;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

fn relevant(packet: &[u8]) -> bool {
    let mut input = false;
    let mut event_node = false;
    let mut changed = false;
    for field in packet.split(|byte| *byte == 0) {
        input |= field == b"SUBSYSTEM=input";
        event_node |= field
            .strip_prefix(b"DEVNAME=input/")
            .is_some_and(|name| name.starts_with(b"event"));
        changed |= matches!(field, b"ACTION=add" | b"ACTION=remove" | b"ACTION=change");
    }
    input && event_node && changed
}

#[cfg(test)]
mod tests {
    use super::relevant;

    #[test]
    fn accepts_only_input_event_device_changes() {
        assert!(relevant(
            b"add@/devices/virtual/input/input1/event14\0ACTION=add\0SUBSYSTEM=input\0DEVNAME=input/event14\0"
        ));
        assert!(!relevant(
            b"add@/devices/virtual/input/input1\0ACTION=add\0SUBSYSTEM=input\0"
        ));
        assert!(!relevant(
            b"add@/devices/pci/block/sda\0ACTION=add\0SUBSYSTEM=block\0DEVNAME=sda\0"
        ));
    }
}
