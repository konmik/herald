use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn new_and_existing_paths_remain_private() {
    let data = std::env::temp_dir().join(format!("herald-private-{}-{}", std::process::id(), crate::state::timestamp()));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    directory(&data).unwrap();
    assert_eq!(std::fs::metadata(&data).unwrap().permissions().mode() & 0o777, 0o700);
    let queue = data.join("queue.json");
    std::fs::write(&queue, "secret").unwrap();
    std::fs::set_permissions(&queue, std::fs::Permissions::from_mode(0o644)).unwrap();
    harden(&queue).unwrap();
    assert_eq!(std::fs::read_to_string(&queue).unwrap(), "secret");
    write(&data.join("queue.tmp"), b"replacement").unwrap();
    std::fs::rename(data.join("queue.tmp"), &queue).unwrap();
    assert_eq!(std::fs::metadata(&queue).unwrap().permissions().mode() & 0o777, 0o600);
    let history = data.join("history.jsonl");
    file(&history, true).unwrap().write_all(b"record\n").unwrap();
    assert_eq!(std::fs::metadata(history).unwrap().permissions().mode() & 0o777, 0o600);
    std::fs::remove_dir_all(data).unwrap();
}
