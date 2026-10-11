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
