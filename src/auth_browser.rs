//! A native browser window only while signing in; never injected into Claude pages.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
};
use url::Url;

#[derive(Serialize, Deserialize)]
struct Request {
    url: String,
    server: String,
    title: String,
}

pub fn valid_login_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    value.len() <= 12000
        && !value.chars().any(char::is_control)
        && url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.path().trim_end_matches('/'),
            "/oauth/authorize" | "/cai/oauth/authorize"
        )
        && matches!(
            url.host_str(),
            Some("claude.ai" | "claude.com" | "console.anthropic.com" | "platform.claude.com")
        )
}

fn profile(server: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    crate::storage::directory()
        .join("browser")
        .join(format!("{:x}", Sha256::digest(server.as_bytes())))
}

pub struct Browser {
    process: Option<Child>,
}
impl Browser {
    pub fn open(server: &str, title: &str, url: &str) -> Result<Self> {
        anyhow::ensure!(
            valid_login_url(url),
            "Сервер вернул неизвестную страницу входа."
        );
        let mut process = Command::new(std::env::current_exe()?)
            .arg("--auth-browser")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Не удалось открыть встроенный браузер.")?;
        let request = Request {
            url: url.into(),
            server: server.into(),
            title: title.chars().take(100).collect(),
        };
        let write = process
            .stdin
            .as_mut()
            .context("Не удалось передать страницу входа.")
            .and_then(|input| {
                serde_json::to_writer(&mut *input, &request)?;
                input.write_all(b"\n")?;
                input.flush()?;
                Ok(())
            });
        if let Err(error) = write {
            let _ = process.kill();
            let _ = process.wait();
            return Err(error);
        }
        Ok(Self {
            process: Some(process),
        })
    }
    pub fn running(&mut self) -> bool {
        self.process
            .as_mut()
            .is_some_and(|p| matches!(p.try_wait(), Ok(None)))
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            // Let the native web context close cleanly and flush its browser profile. Never
            // stall the chat/UI while waiting, and still reap a browser that fails to exit.
            drop(process.stdin.take());
            std::thread::spawn(move || {
                for _ in 0..50 {
                    if !matches!(process.try_wait(), Ok(None)) {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                let _ = process.kill();
                let _ = process.wait();
            });
        }
    }
}

pub fn run(smoke: bool) -> Result<()> {
    use tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        window::WindowBuilder,
    };
    use wry::{WebContext, WebViewBuilder};
    enum Action {
        Close,
        Navigate(String),
        Ready,
    }
    let request = if smoke {
        Request {
            url: String::new(),
            server: "smoke".into(),
            title: "Проверка браузера".into(),
        }
    } else {
        let mut line = String::new();
        std::io::stdin().lock().take(20000).read_line(&mut line)?;
        let request: Request = serde_json::from_str(&line)?;
        anyhow::ensure!(valid_login_url(&request.url), "Unknown login page");
        request
    };
    let directory = profile(&request.server);
    std::fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let events = EventLoopBuilder::<Action>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title(format!("Вход в Claude · {} — Relay", request.title))
        .with_inner_size(tao::dpi::LogicalSize::new(960., 760.))
        .with_min_inner_size(tao::dpi::LogicalSize::new(600., 480.))
        .build(&events)?;
    let mut context = WebContext::new(Some(directory));
    let navigate = events.create_proxy();
    let mut builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_navigation_handler(|value| {
            Url::parse(&value).is_ok_and(|u| {
                matches!(u.scheme(), "https" | "about")
                    || u.scheme() == "http"
                        && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
            })
        })
        .with_new_window_req_handler(move |url, _| {
            let _ = navigate.send_event(Action::Navigate(url));
            wry::NewWindowResponse::Deny
        });
    if smoke {
        let ready = events.create_proxy();
        builder=builder.with_html("<!doctype html><html lang='ru'><meta charset='utf-8'><style>body{background:#17181c;color:#e8e9ef;font:24px sans-serif;padding:48px}</style><h1>Вход в Claude</h1><p>Встроенный браузер Relay работает.</p><script>window.ipc.postMessage('ready')</script></html>")
            .with_ipc_handler(move|request|{if request.body()=="ready"{let _=ready.send_event(Action::Ready);}});
    } else {
        builder = builder.with_url(&request.url);
    }
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().context("No browser container")?)?
    };
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window)?;
    if !smoke {
        let close = events.create_proxy();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            let _ = close.send_event(Action::Close);
        });
    } else {
        let close = events.create_proxy();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(15));
            let _ = close.send_event(Action::Close);
        });
    }
    events.run(move |event, _, control| {
        *control = ControlFlow::Wait;
        // Context and window must live as long as the webview on all three platforms.
        let _keep = (&context, &window);
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(Action::Close) => *control = ControlFlow::Exit,
            Event::UserEvent(Action::Navigate(url)) => {
                if Url::parse(&url).is_ok_and(|u| u.scheme() == "https") {
                    let _ = webview.load_url(&url);
                }
            }
            Event::UserEvent(Action::Ready) => {
                println!("{{\"browser_smoke\":true}}");
                *control = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_official_initial_auth_pages() {
        for url in [
            "https://claude.ai/oauth/authorize?state=fixture",
            "https://console.anthropic.com/oauth/authorize?code=true",
        ] {
            assert!(valid_login_url(url));
        }
        for url in [
            "http://claude.ai/oauth/authorize",
            "https://claude.ai.evil.test/oauth/authorize",
            "https://claude.ai@evil.test/oauth/authorize",
            "https://evil.test@claude.ai/oauth/authorize",
            "https://claude.ai:444/oauth/authorize",
            "https://claude.ai/",
            "file:///etc/passwd",
            "javascript:alert(1)",
        ] {
            assert!(!valid_login_url(url), "{url}");
        }
    }
    #[test]
    fn profiles_are_separate_and_cannot_escape_store() {
        assert_ne!(profile("a"), profile("b"));
        assert_eq!(profile("a"), profile("a"));
        assert_eq!(
            profile("../../other").parent(),
            Some(crate::storage::directory().join("browser").as_path())
        );
    }
}
