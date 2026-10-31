//! In-game tests on this computer. Relay keeps its own Minecraft + Fabric copy (shared by
//! every run) and starts it hidden with the test bot. Other launchers' folders are only read,
//! to reuse asset objects instead of downloading them again.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

const TESTBOT: &[u8] = include_bytes!("../assets/relay-testbot.jar");
const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const JAVA_RUNTIMES: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

pub struct Job {
    pub version: String,
    pub loader: String,
    pub scenario: String,
    pub timeout: Duration,
    /// Holds the mod jars received from the server.
    pub mods: PathBuf,
    /// Screenshots, report and logs of this run.
    pub out: PathBuf,
}

struct Launch {
    loader: String,
    java: PathBuf,
    main: String,
    classpath: Vec<PathBuf>,
    assets: PathBuf,
    index: String,
}

struct Item {
    url: String,
    path: PathBuf,
    sha1: Option<String>,
    size: Option<u64>,
}

pub fn root() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Relay")
        .join("gametest")
}

/// Prepares the game if needed, runs the scenario and returns the summary for Claude.
pub fn run(job: &Job, cancel: &AtomicBool, progress: &(dyn Fn(String) + Sync)) -> Result<Value> {
    let started = Instant::now();
    let launch = prepare(&job.version, &job.loader, cancel, progress)?;
    anyhow::ensure!(!cancel.load(Ordering::Relaxed), "Тест остановлен");
    prune_runs(10);
    let instance = root().join("instance");
    let mods = instance.join("mods");
    fs::create_dir_all(&mods)?;
    for entry in fs::read_dir(&mods)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "jar") {
            fs::remove_file(path)?;
        }
    }
    for entry in fs::read_dir(&job.mods)? {
        let path = entry?.path();
        fs::copy(&path, mods.join(path.file_name().context("mod name")?))?;
    }
    fs::write(mods.join("relay-testbot.jar"), TESTBOT)?;
    fs::create_dir_all(&job.out)?;
    let scenario = job.out.join("scenario.txt");
    fs::write(&scenario, &job.scenario)?;
    let total = job
        .scenario
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && *l != "quit")
        .count();

    progress("Запускаем игру".into());
    let separator = if cfg!(windows) { ";" } else { ":" };
    let classpath = launch
        .classpath
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join(separator);
    let log = fs::File::create(job.out.join("game.log"))?;
    let mut command = Command::new(&launch.java);
    command
        .arg("-Xmx3G")
        .arg(format!("-Drelay.test.scenario={}", scenario.display()))
        .arg(format!("-Drelay.test.out={}", job.out.display()))
        .arg(format!("-Drelay.test.timeout={}", job.timeout.as_secs()))
        .args(["-cp", &classpath, &launch.main])
        .args(["--username", "RelayTest", "--uuid", "5a1d7e57-0000-3000-8000-000000000001"])
        .args(["--accessToken", "0", "--userType", "legacy", "--versionType", "release"])
        .args(["--version", &job.version])
        .arg("--gameDir")
        .arg(&instance)
        .arg("--assetsDir")
        .arg(&launch.assets)
        .args(["--assetIndex", &launch.index, "--width", "1280", "--height", "720"])
        .current_dir(&instance)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().context("Не удалось запустить Java")?;
    let deadline = Instant::now() + job.timeout + Duration::from_secs(90);
    let mut last = String::new();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if cancel.load(Ordering::Relaxed) || Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        let done = count_done(&job.out.join("game.log"));
        let message = format!("шаг {}/{}", done.min(total), total);
        if message != last {
            progress(message.clone());
            last = message;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let _ = fs::copy(instance.join("logs").join("latest.log"), job.out.join("latest.log"));
    let report = fs::read_to_string(job.out.join("report.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let tail = tail(&job.out.join("game.log"), 40);
    let message = match (&report, status) {
        (Some(_), _) => None,
        (None, None) if cancel.load(Ordering::Relaxed) => Some("Тест остановлен".to_owned()),
        (None, None) => Some("Игра не уложилась во время теста и была закрыта".to_owned()),
        (None, Some(status)) => Some(format!("Игра завершилась без отчёта ({status})")),
    };
    let ok = report.as_ref().is_some_and(|r| r["ok"] == true);
    Ok(json!({
        "ok": ok,
        "report": report,
        "message": message,
        "seconds": started.elapsed().as_secs(),
        "loader": launch.loader,
        "java": launch.java,
        "log_tail": if ok { Value::Null } else { json!(tail) },
    }))
}

/// Keeps the newest runs only; each holds a copy of the mods and its screenshots.
fn prune_runs(keep: usize) {
    let mut runs: Vec<_> = fs::read_dir(root().join("runs"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    runs.sort();
    for (_, path) in runs.iter().rev().skip(keep) {
        let _ = fs::remove_dir_all(path);
    }
}

fn count_done(log: &Path) -> usize {
    fs::read_to_string(log)
        .map(|text| text.lines().filter(|l| l.contains("[relay-test]") && l.contains("✓")).count())
        .unwrap_or(0)
}

fn tail(path: &Path, lines: usize) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let all: Vec<_> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent("Relay-desktop")
        .build()
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<Value> {
    let text = agent.get(url).call().with_context(|| format!("Нет ответа от {url}"))?.into_string()?;
    Ok(serde_json::from_str(&text)?)
}

fn sha1_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hash = Sha1::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Some(format!("{:x}", hash.finalize()))
}

/// Cached JSON that is fetched again only when its hash differs.
fn cached_json(agent: &ureq::Agent, url: &str, path: &Path, sha1: Option<&str>) -> Result<Value> {
    let fresh = path.exists() && sha1.is_none_or(|h| sha1_file(path).as_deref() == Some(h));
    if !fresh {
        let text = agent.get(url).call()?.into_string()?;
        fs::create_dir_all(path.parent().context("cache folder")?)?;
        fs::write(path, text)?;
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

fn rules_allow(rules: &Value) -> bool {
    let Some(rules) = rules.as_array() else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        let os = &rule["os"];
        let matches = rule["features"].is_null()
            && (os.is_null() || (os["name"].as_str().is_none_or(|n| n == os_name()) && os["arch"].is_null()));
        if matches {
            allowed = rule["action"] == "allow";
        }
    }
    allowed
}

/// Natives jars for other architectures carry libraries with the same file names.
fn native_matches(classifier: &str) -> bool {
    let own = format!("natives-{}", os_name());
    let arm = std::env::consts::ARCH == "aarch64";
    if !classifier.starts_with(&own) {
        return !classifier.starts_with("natives-");
    }
    let suffix = &classifier[own.len()..];
    if arm { suffix == "-arm64" } else { suffix.is_empty() }
}

fn maven_path(name: &str) -> Option<String> {
    let parts: Vec<_> = name.split(':').collect();
    let (group, artifact, version) = (parts.first()?, parts.get(1)?, parts.get(2)?);
    let file = match parts.get(3) {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.jar"),
        None => format!("{artifact}-{version}.jar"),
    };
    Some(format!("{}/{artifact}/{version}/{file}", group.replace('.', "/")))
}

fn prepare(version: &str, loader: &str, cancel: &AtomicBool, progress: &(dyn Fn(String) + Sync)) -> Result<Launch> {
    progress("Проверяем файлы игры".into());
    let agent = agent();
    let game = root().join("game");
    let version_dir = game.join("versions").join(version);
    let manifest = get_json(&agent, MANIFEST)?;
    let entry = manifest["versions"]
        .as_array()
        .and_then(|v| v.iter().find(|e| e["id"] == version))
        .with_context(|| format!("Нет версии Minecraft {version}"))?;
    let vj = cached_json(
        &agent,
        entry["url"].as_str().context("version url")?,
        &version_dir.join(format!("{version}.json")),
        entry["sha1"].as_str(),
    )?;
    let loader = if loader.is_empty() {
        let list = get_json(&agent, &format!("https://meta.fabricmc.net/v2/versions/loader/{version}"))?;
        list.as_array()
            .and_then(|l| l.iter().find(|e| e["loader"]["stable"] == true))
            .and_then(|e| e["loader"]["version"].as_str())
            .context("Fabric не поддерживает эту версию")?
            .to_owned()
    } else {
        loader.to_owned()
    };
    let fj = cached_json(
        &agent,
        &format!("https://meta.fabricmc.net/v2/versions/loader/{version}/{loader}/profile/json"),
        &version_dir.join(format!("fabric-{loader}.json")),
        None,
    )?;

    let client = version_dir.join(format!("{version}.jar"));
    let mut items = vec![Item {
        url: vj["downloads"]["client"]["url"].as_str().context("client url")?.into(),
        path: client.clone(),
        sha1: vj["downloads"]["client"]["sha1"].as_str().map(Into::into),
        size: vj["downloads"]["client"]["size"].as_u64(),
    }];
    let mut classpath = vec![];
    let mut fabric = vec![];
    for lib in fj["libraries"].as_array().into_iter().flatten() {
        let name = lib["name"].as_str().context("library name")?;
        let rel = maven_path(name).context("library name")?;
        let base = lib["url"].as_str().unwrap_or("https://maven.fabricmc.net/").trim_end_matches('/');
        let path = game.join("libraries").join(&rel);
        items.push(Item {
            url: format!("{base}/{rel}"),
            path: path.clone(),
            sha1: lib["sha1"].as_str().map(Into::into),
            size: lib["size"].as_u64(),
        });
        classpath.push(path);
        fabric.push(name.split(':').take(2).collect::<Vec<_>>().join(":"));
    }
    for lib in vj["libraries"].as_array().into_iter().flatten() {
        let artifact = &lib["downloads"]["artifact"];
        if !rules_allow(&lib["rules"]) || artifact.is_null() {
            continue;
        }
        let name = lib["name"].as_str().unwrap_or("");
        let parts: Vec<_> = name.split(':').collect();
        if parts.len() == 3 && fabric.contains(&parts[..2].join(":")) {
            continue;
        }
        if parts.len() > 3 && !native_matches(parts[3]) {
            continue;
        }
        let path = game.join("libraries").join(artifact["path"].as_str().context("library path")?);
        items.push(Item {
            url: artifact["url"].as_str().context("library url")?.into(),
            path: path.clone(),
            sha1: artifact["sha1"].as_str().map(Into::into),
            size: artifact["size"].as_u64(),
        });
        classpath.push(path);
    }
    classpath.push(client);

    let assets = game.join("assets");
    let index_id = vj["assetIndex"]["id"].as_str().context("asset index")?.to_owned();
    let index = cached_json(
        &agent,
        vj["assetIndex"]["url"].as_str().context("asset index url")?,
        &assets.join("indexes").join(format!("{index_id}.json")),
        vj["assetIndex"]["sha1"].as_str(),
    )?;
    let seeds = asset_sources(&assets);
    for object in index["objects"].as_object().into_iter().flatten().map(|(_, o)| o) {
        let hash = object["hash"].as_str().context("asset hash")?;
        let rel = Path::new(&hash[..2]).join(hash);
        let target = assets.join("objects").join(&rel);
        if target.exists() {
            continue;
        }
        if let Some(seed) = seeds.iter().map(|s| s.join(&rel)).find(|p| p.exists()) {
            fs::create_dir_all(target.parent().context("asset folder")?)?;
            if fs::copy(&seed, &target).is_ok() {
                continue;
            }
        }
        items.push(Item {
            url: format!("https://resources.download.minecraft.net/{}/{hash}", &hash[..2]),
            path: target,
            sha1: Some(hash.into()),
            size: object["size"].as_u64(),
        });
    }

    let major = vj["javaVersion"]["majorVersion"].as_u64().unwrap_or(21);
    let java = match find_java(major) {
        Some(java) => java,
        None => java_runtime(&agent, vj["javaVersion"]["component"].as_str().unwrap_or("java-runtime-delta"), &mut items)?,
    };
    fetch(&agent, items, cancel, progress)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&java, fs::Permissions::from_mode(0o755));
    }
    Ok(Launch {
        loader,
        java,
        main: fj["mainClass"].as_str().context("main class")?.into(),
        classpath,
        assets,
        index: index_id,
    })
}

fn fetch(agent: &ureq::Agent, items: Vec<Item>, cancel: &AtomicBool, progress: &(dyn Fn(String) + Sync)) -> Result<()> {
    let todo: Vec<_> = items
        .into_iter()
        .filter(|i| match (fs::metadata(&i.path), i.size) {
            (Ok(m), Some(size)) => m.len() != size,
            (Ok(_), None) => false,
            (Err(_), _) => true,
        })
        .collect();
    if todo.is_empty() {
        return Ok(());
    }
    let total = todo.len();
    let queue = Mutex::new(todo);
    let done = Mutex::new(0usize);
    let error = Mutex::new(None::<anyhow::Error>);
    progress(format!("Скачиваем игру · 0/{total}"));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| loop {
                if cancel.load(Ordering::Relaxed) || error.lock().unwrap().is_some() {
                    return;
                }
                let Some(item) = queue.lock().unwrap().pop() else {
                    return;
                };
                if let Err(e) = download(agent, &item) {
                    *error.lock().unwrap() = Some(e);
                    return;
                }
                let mut count = done.lock().unwrap();
                *count += 1;
                if *count % 50 == 0 || *count == total {
                    progress(format!("Скачиваем игру · {count}/{total}"));
                }
            });
        }
    });
    if let Some(e) = error.into_inner().unwrap() {
        return Err(e);
    }
    anyhow::ensure!(!cancel.load(Ordering::Relaxed), "Тест остановлен");
    Ok(())
}

fn download(agent: &ureq::Agent, item: &Item) -> Result<()> {
    let mut last = None;
    for _ in 0..3 {
        match (|| -> Result<()> {
            let mut body = vec![];
            agent.get(&item.url).call()?.into_reader().read_to_end(&mut body)?;
            if let Some(expected) = &item.sha1 {
                anyhow::ensure!(&format!("{:x}", Sha1::digest(&body)) == expected, "Контрольная сумма не совпала");
            }
            fs::create_dir_all(item.path.parent().context("folder")?)?;
            let part = item.path.with_extension("part");
            fs::File::create(&part)?.write_all(&body)?;
            fs::rename(&part, &item.path)?;
            Ok(())
        })() {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap().context(format!("Не скачался {}", item.url)))
}

/// Asset objects of any installed launcher, so a first run copies instead of downloading.
fn asset_sources(own: &Path) -> Vec<PathBuf> {
    let mut found = vec![];
    let mut bases = vec![];
    if let Some(home) = dirs::home_dir() {
        bases.push(home.join(".minecraft"));
        bases.push(home.join(".local/share"));
    }
    bases.extend(dirs::config_dir());
    bases.extend(dirs::data_local_dir());
    fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if dir.file_name().is_some_and(|n| n == "assets") && dir.join("objects").is_dir() {
            found.push(dir.join("objects"));
            return;
        }
        if depth == 0 {
            return;
        }
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&entry.path(), depth - 1, found);
            }
        }
    }
    for base in bases {
        walk(&base, 4, &mut found);
    }
    found.retain(|p| !p.starts_with(own));
    found.sort();
    found.dedup();
    found
}

fn java_name() -> &'static str {
    if cfg!(windows) { "javaw.exe" } else { "java" }
}

fn java_major(java: &Path) -> Option<u64> {
    let release = java.parent()?.parent()?.join("release");
    let text = fs::read_to_string(release).ok()?;
    let line = text.lines().find(|l| l.starts_with("JAVA_VERSION="))?;
    let version = line.trim_start_matches("JAVA_VERSION=").trim_matches('"');
    version.split(['.', '_', '+']).next()?.parse().ok()
}

fn find_java(major: u64) -> Option<PathBuf> {
    let mut candidates = vec![];
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        candidates.push(PathBuf::from(home).join("bin").join(java_name()));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|p| p.join(java_name())));
    }
    let mut roots = vec![root().join("runtime")];
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(dir) = std::env::var_os(var) {
                let dir = PathBuf::from(dir);
                roots.extend(["Eclipse Adoptium", "Java", "Microsoft", "Zulu", "BellSoft"].map(|v| dir.join(v)));
            }
        }
    } else {
        roots.extend(["/usr/lib/jvm", "/usr/java", "/opt"].map(PathBuf::from));
    }
    roots.extend(dirs::config_dir());
    roots.extend(dirs::data_local_dir());
    fn walk(dir: &Path, depth: usize, name: &str, out: &mut Vec<PathBuf>) {
        let candidate = dir.join("bin").join(name);
        if candidate.is_file() {
            out.push(candidate);
            return;
        }
        if depth == 0 {
            return;
        }
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&entry.path(), depth - 1, name, out);
            }
        }
    }
    for dir in roots {
        walk(&dir, 4, java_name(), &mut candidates);
    }
    candidates
        .into_iter()
        .filter(|p| p.is_file())
        .filter_map(|p| java_major(&p).map(|v| (v, p)))
        .filter(|(v, _)| *v >= major)
        .min_by_key(|(v, _)| *v)
        .map(|(_, p)| p)
}

/// Mojang's own Java build, listed file by file; the files join the main download queue.
fn java_runtime(agent: &ureq::Agent, component: &str, items: &mut Vec<Item>) -> Result<PathBuf> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86") => "windows-x86",
        ("windows", _) => "windows-x64",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        (_, "aarch64") => "linux-arm64",
        _ => "linux",
    };
    let all = get_json(agent, JAVA_RUNTIMES)?;
    let entry = &all[platform][component][0];
    let manifest = get_json(agent, entry["manifest"]["url"].as_str().context("Нет Java для этой системы")?)?;
    let base = root().join("runtime").join(component);
    for (path, file) in manifest["files"].as_object().into_iter().flatten() {
        if file["type"] == "file" {
            let raw = &file["downloads"]["raw"];
            items.push(Item {
                url: raw["url"].as_str().context("java file")?.into(),
                path: base.join(path),
                sha1: raw["sha1"].as_str().map(Into::into),
                size: raw["size"].as_u64(),
            });
        }
    }
    Ok(base.join("bin").join(java_name()))
}

/// Names of the files worth sending back, smallest first so logs arrive even if images are many.
pub fn results(out: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(out)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "png" || e == "json" || e == "log" || e == "txt")
        })
        .collect();
    files.sort_by_key(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_names_map_to_repository_paths() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.20.1").unwrap(),
            "net/fabricmc/fabric-loader/0.20.1/fabric-loader-0.20.1.jar"
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar"
        );
    }

    #[test]
    fn only_this_machines_natives_are_used() {
        let own = format!("natives-{}", os_name());
        assert!(native_matches(&own) != (std::env::consts::ARCH == "aarch64"));
        assert!(!native_matches(&format!("{own}-x86")));
        assert!(!native_matches("natives-someother"));
    }

    #[test]
    fn rules_follow_the_last_matching_entry() {
        assert!(rules_allow(&Value::Null));
        assert!(rules_allow(&json!([{"action":"allow"}])));
        assert!(!rules_allow(&json!([{"action":"allow","os":{"name":"nowhere"}}])));
        assert!(!rules_allow(&json!([{"action":"allow"},{"action":"disallow","os":{"name":os_name()}}])));
        assert!(!rules_allow(&json!([{"action":"allow","features":{"is_demo_user":true}}])));
    }
}
