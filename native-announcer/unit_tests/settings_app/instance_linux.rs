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
