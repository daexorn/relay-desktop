//! Stage a verified stable binary for the next launch. Running sessions keep
//! using the executable they started with.
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

const DOWNLOAD: &str = "https://github.com/daexorn/relay-desktop/releases/latest/download/";
fn version(text: &str) -> Option<[u32; 3]> {
    let parts: Vec<_> = text.split('.').map(str::parse::<u32>).collect::<Result<_, _>>().ok()?;
    parts.try_into().ok()
}

pub fn start(tui: bool) {
    if cfg!(debug_assertions) || !cfg!(all(target_arch = "x86_64", any(target_os = "linux", target_os = "windows"))) { return; }
    std::thread::spawn(move || loop {
        let delay = match check(tui) {
            Ok(true) => return,
            Ok(false) => 6 * 3600,
            Err(_) => 900,
        };
        std::thread::sleep(Duration::from_secs(delay));
    });
}

fn check(tui: bool) -> Result<bool> {
    let current = std::env::current_exe()?;
    let parent = current.parent().context("missing installation directory")?;
    let win = cfg!(windows);
    let family = if tui { "Relay-TUI" } else { "Relay" };
    let binary = match (win, tui) {
        (true, true) => "Relay-TUI.exe", (true, false) => "Relay.exe",
        (false, true) => "relay-tui", (false, false) => "relay",
    };
    ensure!(current.file_name().and_then(|s| s.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(binary)), "custom binary name");
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).build();
    let release: Value = serde_json::from_reader(agent.get(&format!("{DOWNLOAD}checksums.json")).set("User-Agent", "Relay-Updater").call()?.into_reader().take(1<<20))?;
    let suffix = if win { "-windows-x64.zip" } else { "-linux-x64.tar.gz" };
    let prefix = format!("{family}-");
    let installed = version(env!("CARGO_PKG_VERSION")).unwrap();
    let entries = release.as_object().context("invalid release manifest")?;
    let asset = entries.iter().filter_map(|(name, record)| {
        let candidate = version(name.strip_prefix(&prefix)?.strip_suffix(suffix)?)?;
        (candidate > installed).then_some((candidate, name, record))
    }).max_by_key(|(candidate, _, _)| *candidate);
    let Some((_, name, record)) = asset else { return Ok(false); };
    let url = format!("{DOWNLOAD}{name}");
    let size = record["size"].as_u64().context("missing size")?;
    let digest = record["sha256"].as_str().context("missing checksum")?;
    ensure!((1..=128*1024*1024).contains(&size) && digest.len() == 64, "invalid release asset");
    let lock = OpenOptions::new().write(true).create(true).truncate(false).open(parent.join(format!(".{binary}.update-lock")))?;
    lock.try_lock()?;
    let stage = parent.join(format!(".relay-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&stage)?;
    let result = (|| -> Result<()> {
        let mut body = Vec::new();
        agent.get(&url).set("User-Agent", "Relay-Updater").call()?.into_reader().take(size+1).read_to_end(&mut body)?;
        ensure!(body.len() as u64 == size && digest == format!("{:x}", Sha256::digest(&body)), "release checksum mismatch");
        let archive = stage.join("download");
        fs::write(&archive, body)?;
        let header = format!("{family}-{}-x64/{binary}", if win { "windows" } else { "linux" });
        let next = stage.join(binary);
        let output = fs::File::create(&next)?;
        let tar = if win {
            Path::new(&std::env::var_os("SystemRoot").context("missing SystemRoot")?).join("System32/tar.exe")
        } else { Path::new("/usr/bin/tar").to_owned() };
        let status = Command::new(tar).arg(if win { "-xOf" } else { "-xzOf" }).arg(archive).arg(header)
            .stdout(Stdio::from(output)).stderr(Stdio::null()).status()?;
        ensure!(status.success() && fs::metadata(&next)?.len()>0, "archive extraction failed");
        fs::set_permissions(&next, fs::metadata(&current)?.permissions())?;
        if win {
            let old = parent.join(format!(".{binary}.previous"));
            if old.exists() { fs::remove_file(&old)?; }
            fs::rename(&current, &old)?;
            if let Err(e) = fs::rename(&next, &current) { fs::rename(old,&current)?; return Err(e.into()); }
        } else { fs::rename(next, &current)?; }
        Ok(())
    })();
    let _ = fs::remove_dir_all(stage);
    result.map(|()| true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_versions_compare_numerically() {
        assert!(version("0.21.0") > version("0.9.12"));
        assert!(version("0.20.10") > version("0.20.9"));
        assert_eq!(version("0.20.4-beta.1"), None);
        assert_eq!(version("0.20"), None);
    }
}
