// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure-Rust updater.
//!
//! Supported channel: `zsync|<control-url>`. `check` verifies the control
//! file is reachable and parses it; `apply` downloads the full target image
//! (documented v1 scope — no delta reconstruction yet) with progress,
//! then atomically swaps it over the original with a backup held until
//! success. Other schemes report `UnsupportedScheme` honestly.

use oma_appimage::extract::extract_metadata;
use oma_core::{Error, Result};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Available {
        remote_len: u64,
        download_url: String,
    },
    NoUpdateInfo,
    UnsupportedScheme(String),
    Failed(String),
}

#[derive(Debug, Clone)]
struct ZsyncControl {
    download_url: String,
    remote_len: u64,
}

fn parse_control(text: &str) -> Option<ZsyncControl> {
    let mut map = HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Some(ZsyncControl {
        download_url: map.get("URL")?.clone(),
        remote_len: map.get("Length")?.parse().ok()?,
    })
}

fn fetch_text(url: &str) -> Result<String> {
    let mut resp =
        reqwest::blocking::get(url).map_err(|e| Error::Update(format!("fetch {url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Update(format!(
            "fetch {url}: HTTP {}",
            resp.status()
        )));
    }
    let mut text = String::new();
    resp.read_to_string(&mut text)
        .map_err(|e| Error::Update(format!("read {url}: {e}")))?;
    Ok(text)
}

/// Check whether `path` has a reachable update channel.
pub fn check(path: &Path) -> Check {
    let meta = match extract_metadata(path) {
        Ok(m) => m,
        Err(e) => return Check::Failed(format!("extract: {e}")),
    };
    if meta.update_info.trim().is_empty() {
        return Check::NoUpdateInfo;
    }
    let Some((scheme, rest)) = meta.update_info.split_once('|') else {
        return Check::Failed("malformed update information".to_string());
    };
    if scheme != "zsync" {
        return Check::UnsupportedScheme(scheme.to_string());
    }
    let control_url = rest.trim().to_string();
    let text = match fetch_text(&control_url) {
        Ok(t) => t,
        Err(e) => return Check::Failed(format!("{e}")),
    };
    match parse_control(&text) {
        Some(control) => Check::Available {
            remote_len: control.remote_len,
            download_url: control.download_url,
        },
        None => Check::Failed("unparseable zsync control file".to_string()),
    }
}

/// Download the full target and atomically swap it over `path`.
/// `progress(downloaded, total)` is called monotonically.
pub fn apply(path: &Path, progress: &dyn Fn(u64, u64)) -> Result<()> {
    let available = match check(path) {
        Check::Available {
            remote_len,
            download_url,
        } => (remote_len, download_url),
        Check::NoUpdateInfo => return Err(Error::Update("no update information".to_string())),
        Check::UnsupportedScheme(s) => {
            return Err(Error::Update(format!("unsupported scheme: {s}")))
        }
        Check::Failed(msg) => return Err(Error::Update(msg)),
    };
    let (remote_len, download_url) = available;
    let mut resp = reqwest::blocking::get(&download_url)
        .map_err(|e| Error::Update(format!("download: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Update(format!("download: HTTP {}", resp.status())));
    }
    let part = path.with_extension("part");
    {
        let mut out = std::fs::File::create(&part).map_err(|e| Error::Io(part.clone(), e))?;
        let mut downloaded = 0u64;
        let mut buf = [0u8; 65536];
        loop {
            let n = resp
                .read(&mut buf)
                .map_err(|e| Error::Update(format!("download: {e}")))?;
            if n == 0 {
                break;
            }
            use std::io::Write;
            out.write_all(&buf[..n])
                .map_err(|e| Error::Io(part.clone(), e))?;
            downloaded += n as u64;
            progress(downloaded, remote_len);
        }
        out.sync_all().map_err(|e| Error::Io(part.clone(), e))?;
    }
    let got = std::fs::metadata(&part)
        .map_err(|e| Error::Io(part.clone(), e))?
        .len();
    if got != remote_len {
        std::fs::remove_file(&part).ok();
        return Err(Error::Update(format!(
            "truncated download: got {got} of {remote_len} bytes"
        )));
    }
    let backup = path.with_extension("bak");
    if backup.exists() {
        std::fs::remove_file(&backup).map_err(|e| Error::Io(backup.clone(), e))?;
    }
    std::fs::rename(path, &backup).map_err(|e| Error::Io(path.to_path_buf(), e))?;
    if std::fs::rename(&part, path).is_err() {
        std::fs::rename(&backup, path).ok();
        return Err(Error::Update(
            "atomic swap failed, original restored".to_string(),
        ));
    }
    std::fs::remove_file(&backup).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            std::fs::set_permissions(path, perms).ok();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use backhand::{FilesystemWriter, NodeHeader};
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::path::PathBuf;

    /// Bind first so the control file can embed the real base URL.
    fn serve(
        build_control: impl Fn(&str) -> String,
        payload: Vec<u8>,
        fail_control: bool,
        fail_midway: bool,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let control = build_control(&base);
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().take(8) {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let mut head = [0u8; 4096];
                let n = stream.read(&mut head).unwrap_or(0);
                let req = String::from_utf8_lossy(&head[..n]).to_string();
                let (status, body) = if req.contains("GET /app.zsync") {
                    if fail_control {
                        ("500 FAIL", Vec::new())
                    } else {
                        ("200 OK", control.clone().into_bytes())
                    }
                } else if req.contains("GET /app.AppImage") {
                    if fail_midway {
                        ("200 OK", payload[..payload.len() / 2].to_vec())
                    } else {
                        ("200 OK", payload.clone())
                    }
                } else {
                    ("404 NOT FOUND", Vec::new())
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).ok();
                stream.write_all(&body).ok();
            }
        });
        (base, handle)
    }

    fn appimage_with_info(dir: &Path, name: &str, info: &str) -> PathBuf {
        let desktop = format!("[Desktop Entry]\nName=Up\nExec=AppRun\nIcon=app\nX-AppImage-UpdateInformation={info}\n");
        let mut writer = FilesystemWriter::default();
        writer
            .push_file(
                std::io::Cursor::new(desktop.as_bytes()),
                "/App.desktop",
                NodeHeader::default(),
            )
            .expect("desktop");
        let mut squashfs = std::io::Cursor::new(Vec::<u8>::new());
        writer.write(&mut squashfs).expect("squashfs");
        let payload = squashfs.into_inner();
        let stub_len = 512u64;
        let mut img = vec![0u8; stub_len as usize];
        img[0..4].copy_from_slice(b"\x7fELF");
        img[4] = 2;
        img[5] = 1;
        img[8] = 0x41;
        img[9] = 0x49;
        img[10] = 0x02;
        img[16..18].copy_from_slice(&3u16.to_le_bytes());
        img[18..20].copy_from_slice(&62u16.to_le_bytes());
        img[32..40].copy_from_slice(&64u64.to_le_bytes());
        img[52..54].copy_from_slice(&64u16.to_le_bytes());
        img[54..56].copy_from_slice(&56u16.to_le_bytes());
        img[56..58].copy_from_slice(&1u16.to_le_bytes());
        let mut phdr = vec![0u8; 56];
        phdr[0..4].copy_from_slice(&1u32.to_le_bytes());
        phdr[32..40].copy_from_slice(&stub_len.to_le_bytes());
        phdr[40..48].copy_from_slice(&stub_len.to_le_bytes());
        img[64..120].copy_from_slice(&phdr);
        img.extend_from_slice(&payload);
        let path = dir.join(name);
        std::fs::write(&path, &img).expect("write");
        path
    }

    fn control_for(base: &str, len: usize) -> String {
        format!("zsync: 0.6.2\nFilename: app.AppImage\nURL: {base}/app.AppImage\nLength: {len}\nBlocksize: 4096\n")
    }

    #[test]
    fn check_and_apply_round_trip() {
        let dir = std::env::temp_dir().join(format!("oma-upd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let payload = vec![7u8; 200_000];
        let payload_len = payload.len();
        let (base, _srv) = serve(
            move |b| control_for(b, payload_len),
            payload.clone(),
            false,
            false,
        );
        let app = appimage_with_info(&dir, "Up.AppImage", &format!("zsync|{base}/app.zsync"));
        match check(&app) {
            Check::Available {
                remote_len,
                download_url,
            } => {
                assert_eq!(remote_len, payload.len() as u64);
                assert_eq!(download_url, format!("{base}/app.AppImage"));
            }
            other => panic!("unexpected {other:?}"),
        }
        let last = std::cell::Cell::new(0u64);
        let calls = std::cell::Cell::new(0u32);
        apply(&app, &|d, _t| {
            calls.set(calls.get() + 1);
            assert!(d >= last.get(), "progress monotonic");
            last.set(d);
        })
        .expect("apply");
        assert!(calls.get() > 0);
        assert_eq!(std::fs::read(&app).expect("read"), payload);
        assert!(!app.with_extension("part").exists());
        assert!(!app.with_extension("bak").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_info_and_bad_control() {
        let dir = std::env::temp_dir().join(format!("oma-upd2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let plain = appimage_with_info(&dir, "Plain.AppImage", "");
        assert!(matches!(check(&plain), Check::NoUpdateInfo));
        let (base, _srv) = serve(|_| String::new(), vec![], true, false);
        let broken =
            appimage_with_info(&dir, "Broken.AppImage", &format!("zsync|{base}/app.zsync"));
        assert!(matches!(check(&broken), Check::Failed(_)));
        let gh = appimage_with_info(&dir, "Gh.AppImage", "gh-releases|foo|bar|latest");
        assert!(matches!(check(&gh), Check::UnsupportedScheme(_)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn midway_failure_keeps_original() {
        let dir = std::env::temp_dir().join(format!("oma-upd3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let payload = vec![9u8; 200_000];
        let payload_len = payload.len();
        let (base, _srv) = serve(move |b| control_for(b, payload_len), payload, false, true);
        let app = appimage_with_info(&dir, "Half.AppImage", &format!("zsync|{base}/app.zsync"));
        let before = std::fs::read(&app).expect("read");
        let err = apply(&app, &|_, _| {}).expect_err("truncated must fail");
        assert!(err.to_string().contains("truncated"), "got {err}");
        assert_eq!(
            std::fs::read(&app).expect("read"),
            before,
            "original intact"
        );
        assert!(!app.with_extension("part").exists(), "part cleaned up");
        std::fs::remove_dir_all(&dir).ok();
    }
}
