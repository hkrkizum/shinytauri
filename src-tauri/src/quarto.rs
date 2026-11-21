use std::path::{Component, Path, PathBuf, Prefix};

pub(super) fn executable(payload: &Path) -> PathBuf {
    // Quarto converts its resource paths to file URLs and rejects the verbatim prefix.
    let mut components = payload.components();
    let prefix = match components.next() {
        Some(Component::Prefix(prefix)) => Some(prefix.kind()),
        _ => None,
    };
    let payload = match prefix {
        Some(Prefix::VerbatimDisk(drive)) => {
            PathBuf::from(format!("{}:", char::from(drive))).join(components.as_path())
        }
        Some(Prefix::VerbatimUNC(server, share)) => Path::new(r"\\")
            .join(server)
            .join(share)
            .join(components.as_path()),
        _ => payload.to_owned(),
    };
    payload.join(r"quarto\bin\quarto.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_quarto_uses_a_regular_drive_path() {
        let expected = Path::new(r"C:\集計 アプリ\offline\quarto\bin\quarto.exe");
        for payload in [r"\\?\C:\集計 アプリ\offline", r"C:\集計 アプリ\offline"] {
            assert_eq!(executable(Path::new(payload)), expected);
        }
    }

    #[test]
    fn bundled_quarto_preserves_the_network_share() {
        let expected = Path::new(r"\\server\share\集計 アプリ\offline\quarto\bin\quarto.exe");
        for payload in [
            r"\\?\UNC\server\share\集計 アプリ\offline",
            r"\\server\share\集計 アプリ\offline",
        ] {
            assert_eq!(executable(Path::new(payload)), expected);
        }
    }
}
