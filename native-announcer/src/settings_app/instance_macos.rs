use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

// macOS has no abstract socket namespace, so the activation socket is a short
// per-user, per-data-directory path guarded by the same exclusive lock file.
pub(super) struct InstanceGuard {
    listener: UnixListener,
    socket: PathBuf,
    lock: File,
    activation_requested: Cell<bool>,
}

impl InstanceGuard {
    pub(super) fn acquire(data: &Path) -> Result<Option<Self>, String> {
        std::fs::create_dir_all(data).map_err(|error| error.to_string())?;
        let data = data.canonicalize().map_err(|error| error.to_string())?;
        let uid = unsafe { libc::geteuid() };
        let digest = format!("{:x}", Sha256::digest(data.as_os_str().as_bytes()));
        let socket = PathBuf::from(format!("/tmp/herald-settings-{uid}-{}.sock", &digest[..32]));
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(data.join(".settings-app.lock")).map_err(|error| error.to_string())?;
        for _ in 0..40 {
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                let _ = std::fs::remove_file(&socket);
                let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
                listener.set_nonblocking(true).map_err(|error| error.to_string())?;
                return Ok(Some(Self { listener, socket, lock, activation_requested: Cell::new(false) }));
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::WouldBlock { return Err(error.to_string()); }
            if let Ok(mut existing) = UnixStream::connect(&socket) {
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
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            let mut request = [0; 8];
            if stream.read_exact(&mut request).is_ok() && &request == b"activate" {
                window.activate_window();
                self.activation_requested.set(true);
                let _ = stream.write_all(&[1]);
            }
        }
    }

    // Ordering a window front does not activate a background macOS app; the
    // caller activates the application when this returns true.
    pub(super) fn take_activation_request(&self) -> bool { self.activation_requested.replace(false) }

    pub(super) fn keep_alive(&self) { let _ = &self.lock; }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.socket); }
}

fn same_user(stream: &UnixStream) -> bool {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_invocation_requests_activation_and_lock_releases_on_exit() {
        let data = std::env::temp_dir().join(format!("herald-instance-{}-{}", std::process::id(), rand::random::<u64>()));
        let first = InstanceGuard::acquire(&data).unwrap().unwrap();
        let listener = first.listener.try_clone().unwrap();
        let server = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        assert!(same_user(&stream));
                        stream.set_nonblocking(false).unwrap();
                        let mut request = [0; 8];
                        stream.read_exact(&mut request).unwrap();
                        assert_eq!(&request, b"activate");
                        stream.write_all(&[1]).unwrap();
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("{error}"),
                }
                assert!(started.elapsed() < Duration::from_secs(2));
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        assert!(InstanceGuard::acquire(&data).unwrap().is_none());
        server.join().unwrap();
        drop(first);
        assert!(InstanceGuard::acquire(&data).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&data);
    }
}
