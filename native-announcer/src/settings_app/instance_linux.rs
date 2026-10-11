use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::Path;
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(super) struct InstanceGuard {
    listener: UnixListener,
    lock: File,
}

impl InstanceGuard {
    pub(super) fn acquire(data: &Path) -> Result<Option<Self>, String> {
        std::fs::create_dir_all(data).map_err(|error| error.to_string())?;
        let data = data.canonicalize().map_err(|error| error.to_string())?;
        let uid = unsafe { libc::geteuid() };
        let digest = Sha256::digest(data.as_os_str().as_bytes());
        let socket = SocketAddr::from_abstract_name(format!("herald-settings-{uid}-{digest:x}")).map_err(|error| error.to_string())?;
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(data.join(".settings-app.lock")).map_err(|error| error.to_string())?;
        for _ in 0..40 {
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                let listener = UnixListener::bind_addr(&socket).map_err(|error| error.to_string())?;
                listener.set_nonblocking(true).map_err(|error| error.to_string())?;
                return Ok(Some(Self { lock, listener }));
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::WouldBlock { return Err(error.to_string()); }
            if let Ok(mut existing) = UnixStream::connect_addr(&socket) {
                if !same_user(&existing) { return Err("The settings activation socket belongs to another user.".into()); }
                existing.set_read_timeout(Some(Duration::from_secs(2))).map_err(|error| error.to_string())?;
                existing.set_write_timeout(Some(Duration::from_secs(2))).map_err(|error| error.to_string())?;
                existing.write_all(b"activate").map_err(|error| error.to_string())?;
                let mut ack = [0];
                if existing.read_exact(&mut ack).is_ok() && ack == [1] { return Ok(None); }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err("Another Herald settings window is running, but it could not be activated.".into())
    }

    pub(super) fn register(&self, _: &gpui_kit::Window) -> Result<(), String> { Ok(()) }

    pub(super) fn activate_pending(&self, window: &mut gpui_kit::Window) {
        while let Ok((mut stream, _)) = self.listener.accept() {
            if !same_user(&stream) { continue; }
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            let mut request = [0; 8];
            if stream.read_exact(&mut request).is_ok() && &request == b"activate" {
                window.activate_window();
                let _ = stream.write_all(&[1]);
            }
        }
    }

    pub(super) fn keep_alive(&self) { let _ = &self.lock; }
}

fn same_user(stream: &UnixStream) -> bool {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    unsafe {
        libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(), &mut size) == 0
            && credentials.uid == libc::geteuid()
    }
}

#[cfg(test)]
#[path = "../../unit_tests/settings_app/instance_linux.rs"]
mod tests;
