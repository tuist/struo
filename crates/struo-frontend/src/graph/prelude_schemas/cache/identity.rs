use std::fs::File;
use std::io::{self, Read};

use sha2::{Digest, Sha256};

pub(super) fn current() -> Option<String> {
    #[cfg(any(
        all(target_os = "linux", target_pointer_width = "64"),
        target_os = "macos",
        windows
    ))]
    let linked = buildid::build_id();
    #[cfg(not(any(
        all(target_os = "linux", target_pointer_width = "64"),
        target_os = "macos",
        windows
    )))]
    let linked = None;

    resolve(linked, || {
        #[cfg(target_os = "linux")]
        {
            File::open("/proc/self/exe")
        }
        #[cfg(not(target_os = "linux"))]
        {
            File::open(std::env::current_exe()?)
        }
    })
}

fn resolve(linked: Option<&[u8]>, open: impl FnOnce() -> io::Result<File>) -> Option<String> {
    if let Some(id) = linked.filter(|id| id.len() >= 16 && id.iter().any(|byte| *byte != 0)) {
        return Some(format!(
            "linked:{}:{}:{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            hex::encode(id)
        ));
    }
    let mut file = open().ok()?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 64 * 1024].into_boxed_slice();
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Some(format!("sha256:{}", hex::encode(hash.finalize())))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn linked_identity_survives_an_unreadable_image_and_distinguishes_builds() {
        let missing = || Err(io::Error::from(io::ErrorKind::NotFound));
        let first = resolve(Some(&[1; 20]), missing).unwrap();
        assert_eq!(first, resolve(Some(&[1; 20]), missing).unwrap());
        let mut other = [1; 20];
        other[19] = 2;
        assert_ne!(first, resolve(Some(&other), missing).unwrap());
        let mut shortest = [0; 16];
        shortest[15] = 1;
        assert!(resolve(Some(&shortest), missing).is_some());
        assert!(resolve(None, missing).is_none());
    }

    #[test]
    fn missing_or_invalid_ids_hash_the_complete_file() {
        let image = tempfile::tempfile().unwrap();
        (&image).write_all(b"abc").unwrap();
        let open = || {
            use std::io::{Seek, SeekFrom};
            let mut file = image.try_clone()?;
            file.seek(SeekFrom::Start(0))?;
            Ok(file)
        };
        for id in [None, Some(&[][..]), Some(&[1; 15][..]), Some(&[0; 20][..])] {
            assert_eq!(
                resolve(id, open).unwrap(),
                "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            );
        }
        (&image).write_all(&vec![0; 65_537]).unwrap();
        let before = resolve(None, open).unwrap();
        (&image)
            .write_all(b"changed beyond the first buffer")
            .unwrap();
        assert_ne!(before, resolve(None, open).unwrap());
    }
}
