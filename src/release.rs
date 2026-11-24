use std::time::Duration;

use serde::Deserialize;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/hirokawa-beach/TuiQuake/releases/latest";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Ready,
    Opening,
    Failed,
}

pub struct Release {
    pub tag: String,
    pub browser: Browser,
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
}

impl Release {
    pub fn check() -> Option<Self> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .into();
        let latest: LatestRelease = agent
            .get(LATEST_RELEASE_API)
            .header(
                "User-Agent",
                concat!("TuiQuake/", env!("CARGO_PKG_VERSION")),
            )
            .header("Accept", "application/vnd.github+json")
            .call()
            .ok()?
            .body_mut()
            .read_json()
            .ok()?;
        Self::newer(latest.tag_name, env!("CARGO_PKG_VERSION"))
    }

    pub fn newer(tag: String, current: &str) -> Option<Self> {
        let latest = stable_version(tag.strip_prefix('v')?)?;
        (latest > stable_version(current)?).then_some(Self {
            tag,
            browser: Browser::Ready,
        })
    }

    pub fn url(&self) -> String {
        format!("{}/releases/tag/{}", env!("CARGO_PKG_REPOSITORY"), self.tag)
    }
}

// Release builds use the same major.minor.patch format in the binary and tag.
fn stable_version(version: &str) -> Option<[u64; 3]> {
    let mut components = version.split('.');
    let mut parts = [0; 3];
    for part in &mut parts {
        let digits = components.next()?;
        if !digits.bytes().all(|digit| digit.is_ascii_digit()) {
            return None;
        }
        *part = digits.parse().ok()?;
    }
    components.next().is_none().then_some(parts)
}

#[cfg(windows)]
pub fn open_in_browser(url: &str) -> bool {
    use std::ptr::null;
    use windows_sys::Win32::{
        System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        },
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };

    let url: Vec<u16> = url.encode_utf16().chain([0]).collect();
    // This runs on a dedicated worker. Balance COM initialization on that thread.
    unsafe {
        let initialized = CoInitializeEx(
            null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        if initialized < 0 {
            return false;
        }
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            null(),
            url.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
        CoUninitialize();
        result as isize > 32
    }
}

#[cfg(unix)]
pub fn open_in_browser(url: &str) -> bool {
    use std::process::{Command, Stdio};

    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(opener)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(any(unix, windows)))]
pub fn open_in_browser(_url: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_only_newer_stable_release_tags() {
        let release = Release::newer("v0.10.0".into(), "0.9.0").unwrap();
        assert_eq!(
            release.url(),
            "https://github.com/hirokawa-beach/TuiQuake/releases/tag/v0.10.0"
        );
        for tag in [
            "v0.9.0",
            "v0.8.20",
            "v0.10.0-rc.1",
            "v0.10",
            "v0.+10.0",
            "latest",
        ] {
            assert!(Release::newer(tag.into(), "0.9.0").is_none(), "{tag}");
        }
    }
}
