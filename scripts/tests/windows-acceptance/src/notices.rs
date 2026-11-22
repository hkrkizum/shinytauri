//! Check the redistribution notices in a product installed by NSIS.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{fs, io::Read, path::Path};

pub fn check(
    offline: &Path,
    originals: &Path,
    installers: &Path,
    r_version: &str,
    rhino: bool,
) -> Result<()> {
    let notices = offline.join("notices");
    for name in ["host.txt", "rust-stdlib.html"] {
        let path = notices.join(name);
        let installed = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let original = fs::read(originals.join(name))?;
        ensure!(
            !installed.is_empty() && installed == original,
            "changed or empty installed {name}"
        );
    }

    let path = notices.join("components.json");
    let components: Value = serde_json::from_slice(
        &fs::read(&path).with_context(|| format!("read {}", path.display()))?,
    )?;
    ensure!(
        components["schemaVersion"] == 1 && components["r"]["version"] == r_version,
        "wrong schema or portable R version in {}",
        path.display()
    );
    let packages = components["packages"]
        .as_array()
        .context("expected packages in components.json")?;
    let shiny = packages
        .iter()
        .find(|p| p["name"] == "shiny")
        .context("missing shiny in components.json")?;
    let description = fs::read(offline.join("library/shiny/DESCRIPTION"))?;
    let description = String::from_utf8_lossy(&description);
    let version = description
        .lines()
        .find_map(|line| line.strip_prefix("Version:"))
        .context("missing installed shiny version")?
        .trim();
    ensure!(
        shiny["version"] == version,
        "installed shiny version differs from components.json"
    );
    let quarto_info = components
        .get("quarto")
        .context("missing quarto in components.json")?;
    let source = components
        .get("sourceBundle")
        .context("missing sourceBundle in components.json")?;
    if rhino {
        check_quarto(&offline.join("quarto"), quarto_info)?;
        check_source_bundle(installers, source)?;
    } else {
        ensure!(source.is_null(), "unexpected sourceBundle for minimal");
        ensure!(
            quarto_info.is_null(),
            "unexpected Quarto in components.json"
        );
    }
    Ok(())
}

fn check_quarto(directory: &Path, info: &Value) -> Result<()> {
    let version = fs::read_to_string(directory.join("share/version"))?;
    ensure!(
        !version.trim().is_empty() && info["version"] == version.trim(),
        "installed Quarto version differs from components.json"
    );
    let tools = info["tools"]
        .as_array()
        .context("expected Quarto tools in components.json")?;
    ensure!(
        directory.join("bin/tools/pandoc.exe").is_file()
            && tools.iter().any(|t| t["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("pandoc"))),
        "expected Pandoc in the installed Quarto and components.json"
    );
    Ok(())
}

const SOURCE_EXAMPLE: &[u8] = include_bytes!("../../fixtures/source-bundle/example.R");

fn sha256(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn check_source_bundle(installers: &Path, source: &Value) -> Result<()> {
    let digest = source["sha256"]
        .as_str()
        .context("missing sourceBundle SHA-256")?;
    ensure!(
        digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid sourceBundle SHA-256"
    );
    let filename = format!("shinytauri-sources-{digest}.tar.gz");
    ensure!(
        source["file"] == filename,
        "wrong sourceBundle file in components.json"
    );
    let sidecar = installers.join(&filename);
    let bytes =
        fs::read(&sidecar).with_context(|| format!("read transferred sidecar {filename}"))?;
    ensure!(
        sha256(&bytes) == digest,
        "transferred sourceBundle SHA-256 differs from components.json"
    );

    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
    let mut saw_example = false;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if entry.header().entry_type().is_dir() && path.as_ref() == Path::new(".") {
            continue;
        }
        ensure!(
            entry.header().entry_type().is_file()
                && path.strip_prefix(".").unwrap_or(&path) == Path::new("example.R")
                && !saw_example,
            "unexpected sourceBundle member {}",
            path.display()
        );
        ensure!(
            entry.size() == SOURCE_EXAMPLE.len() as u64,
            "sourceBundle fixture size changed"
        );
        let mut content = Vec::new();
        entry.read_to_end(&mut content)?;
        ensure!(
            content == SOURCE_EXAMPLE,
            "sourceBundle fixture bytes changed"
        );
        saw_example = true;
    }
    ensure!(saw_example, "missing sourceBundle fixture");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_truncated_notices_and_a_manifest_for_a_different_payload() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let originals = temp.path().join("originals");
        let offline = temp.path().join("offline");
        let notices = offline.join("notices");
        fs::create_dir(&originals)?;
        fs::create_dir_all(&notices)?;
        for name in ["host.txt", "rust-stdlib.html"] {
            fs::write(originals.join(name), name)?;
            fs::write(notices.join(name), name)?;
        }
        fs::create_dir_all(offline.join("library/shiny"))?;
        fs::write(
            offline.join("library/shiny/DESCRIPTION"),
            "Version: 1.2.3\r\n",
        )?;
        fs::write(notices.join("components.json"), br#"{"schemaVersion":1,"r":{"version":"4.5.3"},"packages":[{"name":"shiny","version":"1.2.3"}],"quarto":null,"sourceBundle":null}"#)?;
        check(&offline, &originals, temp.path(), "4.5.3", false)?;
        assert!(check(&offline, &originals, temp.path(), "4.6.2", false).is_err());
        assert!(check(&offline, &originals, temp.path(), "4.5.3", true).is_err());
        let path = notices.join("components.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path)?)?;
        manifest["sourceBundle"] = serde_json::json!({"file": "unexpected.zip"});
        fs::write(&path, serde_json::to_vec(&manifest)?)?;
        assert!(check(&offline, &originals, temp.path(), "4.5.3", false).is_err());
        manifest["sourceBundle"] = Value::Null;
        fs::write(&path, serde_json::to_vec(&manifest)?)?;
        fs::write(notices.join("host.txt"), b"host")?;
        assert!(check(&offline, &originals, temp.path(), "4.5.3", false).is_err());
        Ok(())
    }
    #[test]
    fn checks_quarto_version_and_pandoc_against_the_installed_product() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let dir = temp.path();
        fs::create_dir_all(dir.join("share"))?;
        fs::create_dir_all(dir.join("bin/tools"))?;
        fs::write(dir.join("share/version"), "1.11.9\r\n")?;
        fs::write(dir.join("bin/tools/pandoc.exe"), "fixture")?;
        let mut info = serde_json::json!({"version": "1.11.9", "tools": [{"name": "Pandoc"}]});
        check_quarto(dir, &info)?;
        info["version"] = "0.0.0".into();
        assert!(check_quarto(dir, &info).is_err());
        info["version"] = "1.11.9".into();
        info["tools"] = serde_json::json!([null]);
        assert!(check_quarto(dir, &info).is_err());
        info["tools"] = serde_json::json!([{"name": "Pandoc"}]);
        fs::remove_file(dir.join("bin/tools/pandoc.exe"))?;
        assert!(check_quarto(dir, &info).is_err());
        Ok(())
    }
    #[test]
    fn requires_the_installed_manifest_to_name_a_matching_source_archive() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let dir = temp.path();
        let packed = dir.join("source.tar.gz");
        for fixture in [SOURCE_EXAMPLE, &[b'x'; SOURCE_EXAMPLE.len()]] {
            let gzip = flate2::write::GzEncoder::new(
                fs::File::create(&packed)?,
                flate2::Compression::default(),
            );
            let mut tar = tar::Builder::new(gzip);
            let mut header = tar::Header::new_gnu();
            header.set_size(fixture.len() as u64);
            header.set_mode(0o644);
            tar.append_data(&mut header, "./example.R", fixture)?;
            tar.into_inner()?.finish()?;
            let digest = sha256(&fs::read(&packed)?);
            let filename = format!("shinytauri-sources-{digest}.tar.gz");
            let source = serde_json::json!({"file": filename, "sha256": digest});
            assert!(check_source_bundle(dir, &source).is_err());
            fs::rename(&packed, dir.join(&filename))?;
            assert_eq!(
                check_source_bundle(dir, &source).is_ok(),
                fixture == SOURCE_EXAMPLE
            );
            fs::write(dir.join(&filename), b"changed in transit")?;
            assert!(check_source_bundle(dir, &source).is_err());
        }
        Ok(())
    }
}
