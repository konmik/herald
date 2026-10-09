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
mod tests {
    use super::*;

    struct TestData(std::path::PathBuf);

    impl TestData {
        fn new() -> Self {
            let path = std::path::PathBuf::from("/tmp/opencode").join(format!("herald-instance-{}-{}", std::process::id(), rand::random::<u64>()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestData {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join(".settings-app.lock"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn repeated_invocation_requests_activation_and_lock_releases_on_exit() {
        let data = TestData::new();
        let first = InstanceGuard::acquire(&data.0).unwrap().unwrap();
        let listener = first.listener.try_clone().unwrap();
        let server = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        assert!(same_user(&stream));
                        stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
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
        assert!(InstanceGuard::acquire(&data.0).unwrap().is_none());
        server.join().unwrap();
        drop(first);
        assert!(InstanceGuard::acquire(&data.0).unwrap().is_some());
    }

    #[test]
    fn separate_data_directories_have_independent_instances() {
        let one = TestData::new();
        let two = TestData::new();
        let _first = InstanceGuard::acquire(&one.0).unwrap().unwrap();
        let _second = InstanceGuard::acquire(&two.0).unwrap().unwrap();
    }
}
