use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

pub fn directory(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn file(path: &Path, append: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true).append(append).truncate(!append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

pub fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    file(path, false)?.write_all(bytes)
}

pub fn harden(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            result => result?,
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
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
}
