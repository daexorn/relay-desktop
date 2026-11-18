"""Official server-side CLI installer and updater for Claude Code, OpenAI Codex, and Antigravity (`agy`).
Python stdlib only. Accepts only fixed agent identifiers — never arbitrary URLs or shell strings."""
import io
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import threading
import urllib.request

AGENTS = ("claude", "codex", "agy")
AGENT_LABELS = {
    "claude": "Claude Code",
    "codex": "OpenAI Codex CLI",
    "agy": "Antigravity CLI (agy)",
}

CLAUDE_INSTALL_URL = "https://claude.ai/install.sh"
AGY_INSTALL_URL = "https://antigravity.google/cli/install.sh"
CODEX_RELEASE_URL_FMT = "https://github.com/openai/codex/releases/latest/download/codex-{target}.tar.gz"

ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")


def clean_log(text):
    cleaned = ANSI_RE.sub("", str(text or ""))
    cleaned = re.sub(r"(?:sk-ant-|sk-proj-|sk-|AIza|ya29\.)[A-Za-z0-9_.-]{16,}", "[скрыто]", cleaned)
    return cleaned.strip()[-2400:]


def ensure_user_bin_env():
    local_bin = Path.home() / ".local/bin"
    local_bin.mkdir(parents=True, mode=0o755, exist_ok=True)
    npm_bin = Path.home() / ".npm-global/bin"
    agy_bin = Path.home() / ".gemini/antigravity-cli/bin"
    extra = [str(local_bin), str(npm_bin), str(agy_bin), "/usr/local/bin", "/usr/bin", "/usr/sbin"]
    current = os.environ.get("PATH", "")
    parts = [p for p in current.split(":") if p]
    for p in reversed(extra):
        if p not in parts:
            parts.insert(0, p)
    merged = ":".join(parts)
    os.environ["PATH"] = merged
    env = dict(os.environ)
    env.pop("CLAUDECODE", None)
    env["PATH"] = merged
    return env, local_bin


def download_https(url, max_bytes=220 * 1024 * 1024, timeout=180):
    if not isinstance(url, str) or not url.startswith("https://"):
        raise ValueError("Разрешена загрузка только по HTTPS.")
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Relay-desktop/0.16.0"})
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read(max_bytes + 1)
        if len(body) > max_bytes:
            raise ValueError("Размер установочного пакета превысил допустимый предел.")
        if not body:
            raise ValueError("Получен пустой ответ при скачивании.")
        return body
    except Exception as first_err:
        curl = shutil.which("curl")
        if not curl:
            raise ValueError(f"Не удалось скачать пакет ({clean_log(first_err)}).") from None
        res = subprocess.run(
            [curl, "-fsSL", "--max-time", str(timeout), url],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=timeout + 15,
        )
        if res.returncode != 0 or not res.stdout:
            raise ValueError(f"Ошибка загрузки пакета: {clean_log(res.stderr.decode('utf-8', 'replace') or first_err)}") from None
        if len(res.stdout) > max_bytes:
            raise ValueError("Размер установочного пакета превысил допустимый предел.")
        return res.stdout


def run_install_script(url, env):
    script_bytes = download_https(url, max_bytes=4 * 1024 * 1024, timeout=60)
    shell = shutil.which("bash", path=env.get("PATH")) or shutil.which("sh", path=env.get("PATH")) or "/bin/sh"
    with tempfile.TemporaryDirectory(prefix="relay-cli-setup-") as tmp:
        script_path = Path(tmp) / "install.sh"
        script_path.write_bytes(script_bytes)
        script_path.chmod(0o700)
        res = subprocess.run(
            [shell, str(script_path)],
            cwd=str(Path.home()),
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=600,
        )
        return res.returncode == 0, clean_log(res.stdout + "\n" + res.stderr)


def run_npm_fallback(package_name, env):
    npm = shutil.which("npm", path=env.get("PATH"))
    if not npm:
        return False, "На сервере не установлен Node.js / npm."
    if os.geteuid() == 0:
        cmd = [npm, "install", "-g", package_name]
    else:
        prefix = Path.home() / ".local"
        prefix.mkdir(parents=True, mode=0o755, exist_ok=True)
        cmd = [npm, "install", "--prefix", str(prefix), "-g", package_name]
    res = subprocess.run(
        cmd,
        cwd=str(Path.home()),
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=600,
    )
    return res.returncode == 0, clean_log(res.stdout + "\n" + res.stderr)


def install_codex_release_binary(local_bin):
    machine = os.uname().machine.lower()
    if machine in ("x86_64", "amd64"):
        target = "x86_64-unknown-linux-musl"
    elif machine in ("aarch64", "arm64"):
        target = "aarch64-unknown-linux-musl"
    else:
        return False, f"Архитектура {machine}: пробуем установку через npm."
    url = CODEX_RELEASE_URL_FMT.format(target=target)
    archive_bytes = download_https(url, max_bytes=220 * 1024 * 1024, timeout=240)
    binary_bytes = None
    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:gz") as tf:
        for member in tf.getmembers():
            if not member.isfile():
                continue
            base = os.path.basename(member.name)
            if base in ("codex", f"codex-{target}") and 0 < member.size <= 220 * 1024 * 1024:
                extracted = tf.extractfile(member)
                if extracted is not None:
                    binary_bytes = extracted.read()
                    break
    if not binary_bytes:
        return False, "В архиве релиза Codex не найден исполняемый файл."
    dest = local_bin / "codex"
    fd, tmp_name = tempfile.mkstemp(prefix=".codex-install-", dir=str(local_bin))
    try:
        with os.fdopen(fd, "wb") as out:
            out.write(binary_bytes)
            out.flush()
            os.fsync(out.fileno())
        os.chmod(tmp_name, 0o755)
        os.replace(tmp_name, dest)
    finally:
        if os.path.exists(tmp_name):
            try:
                os.unlink(tmp_name)
            except OSError:
                pass
    return True, f"Установлен бинарный файл {dest}"


class AgentInstaller:
    def __init__(self, resolvers, versions, on_installed=None):
        """
        resolvers: dict[agent -> callable() -> Optional[str]]
        versions: dict[agent -> callable(binary) -> str]
        on_installed: callable(agent, bin_path, version)
        """
        self.resolvers = resolvers
        self.versions = versions
        self.on_installed = on_installed
        self.locks = {a: threading.Lock() for a in AGENTS}
        self.active = set()
        self.state_lock = threading.Lock()

    def status(self):
        ensure_user_bin_env()
        out = {}
        with self.state_lock:
            running_set = set(self.active)
        for agent in AGENTS:
            bin_path = None
            try:
                bin_path = self.resolvers[agent]()
            except Exception:
                bin_path = None
            ver = ""
            if bin_path:
                try:
                    ver = self.versions[agent](bin_path) or ""
                except Exception:
                    ver = AGENT_LABELS[agent]
            out[agent] = {
                "installed": bool(bin_path),
                "binary": bin_path or "",
                "version": ver,
                "running": agent in running_set,
                "label": AGENT_LABELS[agent],
            }
        out["api"] = {"installed": True, "running": False, "version": "", "label": "Пользовательский API"}
        return out

    def install(self, agent, send, action="install"):
        if agent == "all":
            current = self.status()
            targets = [a for a in AGENTS if not current[a]["installed"]]
            if not targets:
                targets = list(AGENTS)
            for item in targets:
                self.install_async(item, send, action=action)
            return
        if agent not in AGENTS:
            raise ValueError("Неизвестный CLI-агент для установки.")
        self.install_async(agent, send, action=action)

    def install_async(self, agent, send, action="install"):
        label = AGENT_LABELS[agent]
        if not self.locks[agent].acquire(blocking=False):
            send({
                "type": "agent_install",
                "agent": agent,
                "running": True,
                "message": f"{label} уже устанавливается или обновляется на сервере…",
            })
            return

        with self.state_lock:
            self.active.add(agent)

        def worker():
            try:
                env, local_bin = ensure_user_bin_env()
                before_bin = None
                try:
                    before_bin = self.resolvers[agent]()
                except Exception:
                    before_bin = None
                before_ver = self.versions[agent](before_bin) if before_bin else ""
                verb = "Обновляем" if before_bin else "Устанавливаем"
                send({
                    "type": "agent_install",
                    "agent": agent,
                    "running": True,
                    "before": before_ver,
                    "message": f"{verb} {label} на сервере…",
                })

                ok = False
                logs = []

                if agent == "claude":
                    if before_bin and action == "update":
                        try:
                            res = subprocess.run(
                                [before_bin, "update"],
                                cwd=str(Path.home()),
                                env=env,
                                stdin=subprocess.DEVNULL,
                                capture_output=True,
                                text=True,
                                encoding="utf-8",
                                errors="replace",
                                timeout=600,
                            )
                            ok = res.returncode == 0
                            logs.append(clean_log(res.stdout + "\n" + res.stderr))
                        except (OSError, subprocess.SubprocessError) as err:
                            logs.append(clean_log(err))
                    if not ok:
                        try:
                            ok, out = run_install_script(CLAUDE_INSTALL_URL, env)
                            logs.append(out)
                        except Exception as err:
                            logs.append(clean_log(err))
                    if not ok:
                        ok, out = run_npm_fallback("@anthropic-ai/claude-code", env)
                        logs.append(out)

                elif agent == "codex":
                    try:
                        ok, out = install_codex_release_binary(local_bin)
                        logs.append(out)
                    except Exception as err:
                        logs.append(clean_log(err))
                        ok = False
                    if not ok:
                        ok, out = run_npm_fallback("@openai/codex", env)
                        logs.append(out)

                elif agent == "agy":
                    if before_bin and action == "update":
                        try:
                            res = subprocess.run(
                                [before_bin, "update"],
                                cwd=str(Path.home()),
                                env=env,
                                stdin=subprocess.DEVNULL,
                                capture_output=True,
                                text=True,
                                encoding="utf-8",
                                errors="replace",
                                timeout=600,
                            )
                            ok = res.returncode == 0
                            logs.append(clean_log(res.stdout + "\n" + res.stderr))
                        except (OSError, subprocess.SubprocessError) as err:
                            logs.append(clean_log(err))
                    if not ok:
                        try:
                            ok, out = run_install_script(AGY_INSTALL_URL, env)
                            logs.append(out)
                        except Exception as err:
                            logs.append(clean_log(err))

                ensure_user_bin_env()
                after_bin = None
                try:
                    after_bin = self.resolvers[agent]()
                except Exception:
                    after_bin = None
                after_ver = self.versions[agent](after_bin) if after_bin else ""
                installed = bool(after_bin)
                final_ok = bool(ok and installed)
                if final_ok and callable(self.on_installed):
                    try:
                        self.on_installed(agent, after_bin, after_ver)
                    except Exception:
                        pass

                msg = (
                    f"{label} готов ({after_ver}). Теперь можно войти или начать сессию."
                    if final_ok
                    else f"Не удалось установить {label}. Проверь доступ сервера в интернет."
                )
                send({
                    "type": "agent_install",
                    "agent": agent,
                    "running": False,
                    "ok": final_ok,
                    "installed": installed,
                    "before": before_ver,
                    "version": after_ver,
                    "changed": before_ver != after_ver,
                    "message": msg,
                    "output": "\n".join(x for x in logs if x)[-2000:],
                })
            except Exception as err:
                send({
                    "type": "agent_install",
                    "agent": agent,
                    "running": False,
                    "ok": False,
                    "installed": False,
                    "message": f"Ошибка установки {label}: {clean_log(err)}",
                    "output": clean_log(err),
                })
            finally:
                with self.state_lock:
                    self.active.discard(agent)
                self.locks[agent].release()
                try:
                    send({"type": "backends_status", "backends": self.status()})
                except Exception:
                    pass

        threading.Thread(target=worker, daemon=True).start()
