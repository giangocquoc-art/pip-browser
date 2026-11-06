//! Native save dialog and a loopback-only download verification fixture.
use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
use objc2_foundation::NSString;
use std::path::{Path, PathBuf};

pub fn suggested_name(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    let mut safe_name = String::new();
    for character in name.chars() {
        if character.is_control()
            || matches!(character, '/' | '\\' | ':')
            || matches!(character as u32, 0x202A..=0x202E | 0x2066..=0x2069)
        {
            continue;
        }
        if safe_name.len() + character.len_utf8() > 180 {
            break;
        }
        safe_name.push(character);
    }
    let name = safe_name;
    let name = name.trim_matches([' ', '.']);
    if name.is_empty() {
        "download".into()
    } else {
        name.into()
    }
}

fn unused_absolute_file(path: &Path) -> bool {
    path.is_absolute()
        && path.file_name().is_some()
        && path
            .symlink_metadata()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        && path.parent().is_some_and(Path::is_dir)
}

/// The native panel supplies the destination; no web content chooses a folder.
/// Existing targets, including dangling symlinks, are rejected. WKDownload also
/// fails an existing destination, so a race never silently replaces a file.
pub fn choose_destination(suggested: &Path) -> Result<Option<PathBuf>, String> {
    let mtm = MainThreadMarker::new().ok_or("Không thể mở hộp lưu ngoài luồng chính.")?;
    let panel = NSSavePanel::savePanel(mtm);
    panel.setTitle(Some(&NSString::from_str("Lưu tệp tải xuống — Pip")));
    panel.setNameFieldStringValue(&NSString::from_str(&suggested_name(suggested)));
    panel.setCanCreateDirectories(true);
    if panel.runModal() != NSModalResponseOK {
        return Ok(None);
    }
    let path = panel
        .URL()
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .ok_or("Không đọc được đường dẫn lưu tệp.")?;
    if !unused_absolute_file(&path) {
        return Err(
            "Tệp đã tồn tại hoặc thư mục không hợp lệ. Hãy tải lại và chọn tên khác.".into(),
        );
    }
    Ok(Some(path))
}

const PAYLOAD: &[u8] = b"Pip native download verification\n";

pub struct Fixture {
    pub url: String,
    directory: PathBuf,
    pub destination: PathBuf,
}

impl Fixture {
    pub fn start() -> std::io::Result<Self> {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            os::unix::fs::DirBuilderExt,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let directory = (0..32)
            .find_map(|_| {
                let path = std::env::temp_dir().join(format!(
                    "pip-download-verify-{}-{:016x}",
                    std::process::id(),
                    rand::random::<u64>()
                ));
                match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => Some(Ok(path)),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .unwrap_or_else(|| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "cannot create verification directory",
                ))
            })?;
        let fixture = Self {
            url: format!("http://{}/pip-native-download.txt", listener.local_addr()?),
            destination: directory.join("pip-native-download.txt"),
            directory,
        };
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(100);
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut request = Vec::new();
                        let mut chunk = [0; 1024];
                        while request.len() < 8192
                            && !request.windows(4).any(|part| part == b"\r\n\r\n")
                        {
                            match stream.read(&mut chunk) {
                                Ok(0) | Err(_) => break,
                                Ok(count) => request.extend_from_slice(&chunk[..count]),
                            }
                        }
                        let valid = std::str::from_utf8(&request).is_ok_and(|request| {
                            request.starts_with("GET /pip-native-download.txt HTTP/")
                        });
                        if valid {
                            let headers = format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"pip-native-download.txt\"\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", PAYLOAD.len());
                            let _ = stream
                                .write_all(headers.as_bytes())
                                .and_then(|_| stream.write_all(PAYLOAD));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(fixture)
    }

    pub fn passed(&self) -> bool {
        std::fs::read(&self.destination).is_ok_and(|bytes| bytes == PAYLOAD)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Remove only the exact file and directory this fixture created. Never
        // recurse or remove user files, even after a failed verification.
        let _ = std::fs::remove_file(&self.destination);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_suggested_names_cannot_hide_a_path_or_spoof_an_extension() {
        assert_eq!(
            suggested_name(Path::new("/remote/folder/report.pdf")),
            "report.pdf"
        );
        assert_eq!(suggested_name(Path::new("..")), "download");
        assert_eq!(
            suggested_name(Path::new("photo\u{202e}gpj.exe")),
            "photogpj.exe"
        );
        assert_eq!(
            suggested_name(Path::new("bad\nname:part.txt")),
            "badnamepart.txt"
        );
        let unicode_name = suggested_name(Path::new(&"🦫".repeat(100)));
        assert!(unicode_name.len() <= 180);
        assert_eq!(unicode_name.chars().count(), 45);
    }

    #[test]
    fn download_destination_must_be_new_and_absolute() {
        assert!(!unused_absolute_file(Path::new("relative.txt")));
        assert!(!unused_absolute_file(&std::env::temp_dir()));
    }

    #[test]
    fn download_refuses_existing_files_and_dangling_symlinks() {
        use std::os::unix::fs::{symlink, DirBuilderExt};
        let directory = std::env::temp_dir().join(format!(
            "pip-download-path-test-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let existing = directory.join("existing.txt");
        let dangling = directory.join("dangling.txt");
        let missing = directory.join("missing.txt");
        std::fs::write(&existing, b"keep user content").unwrap();
        symlink(&missing, &dangling).unwrap();
        let existing_refused = !unused_absolute_file(&existing);
        let dangling_refused = !unused_absolute_file(&dangling);
        let new_allowed = unused_absolute_file(&directory.join("new.txt"));
        let original = std::fs::read(&existing).unwrap();
        // Exact cleanup only, before assertions, including the dangling link.
        std::fs::remove_file(&dangling).unwrap();
        std::fs::remove_file(&existing).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        assert!(existing_refused);
        assert!(dangling_refused);
        assert!(new_allowed);
        assert_eq!(original, b"keep user content");
    }
}
