#!/usr/bin/env python3
"""Run isolated uninstall/update persistence checks against the final grow binary."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("binary", type=Path)
args = parser.parse_args()
binary = args.binary.resolve(strict=True)


def git(cwd, *args):
    return subprocess.check_output(["git", *args], cwd=cwd, stderr=subprocess.PIPE, text=True).strip()


def manifest(path, name, version="1.0.0"):
    path.mkdir(parents=True, exist_ok=True)
    (path / "plugin.json").write_text(json.dumps({"name": name, "version": version}))


def run(home, command, fault=False, success=False):
    env = os.environ.copy()
    env.update(HOME=str(home), GROW_HOME=str(home / ".grow"),
               GROW_MARKETPLACE_REQUIRE_SHA="0", GIT_CONFIG_NOSYSTEM="1",
               GIT_CONFIG_GLOBAL=str(home / "absent-gitconfig"))
    env.pop("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", None)
    if fault:
        env["GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE"] = "1"
    result = subprocess.run([str(binary), "plugin", *command], cwd=home, env=env,
                            capture_output=True, text=True, timeout=30)
    print(f"plugin {' '.join(command)} fault={fault}: exit={result.returncode}", flush=True)
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", flush=True)
    assert (result.returncode == 0) == success, result
    if not success:
        assert "Uninstalled " not in result.stdout and ": updated (" not in result.stdout
    return result.stdout + result.stderr


def register(home, path, kind, plugins):
    registry_path = home / ".grow/installed-plugins/registry.json"
    registry_path.parent.mkdir(parents=True, exist_ok=True)
    entry = {"kind": kind, "installed_at": "2000-01-01T00:00:00Z",
             "updated_at": "2000-01-01T00:00:00Z", "path": str(path), "plugins": plugins}
    registry_path.write_text(json.dumps({"version": 1, "repos": {"fixture": entry}}))
    return registry_path


with tempfile.TemporaryDirectory(prefix="grow-cli-uninstall-") as root:
    home = Path(root)
    artifact = home / ".grow/installed-plugins/fixture"
    manifest(artifact, "demo")
    registry = register(home, artifact, {"type": "Local", "source_path": str(home / "source")},
                        {"demo": {"version": "1.0.0"}})
    before = registry.read_bytes()
    text = run(home, ["uninstall", "demo", "--keep-data"], fault=True)
    assert "incomplete" in text and "test-injected registry save failure" in text
    assert not artifact.exists() and registry.read_bytes() == before
    run(home, ["uninstall", "demo", "--keep-data"], success=True)
    assert json.loads(registry.read_text())["repos"] == {}
    registry.write_bytes(before)
    artifact.write_text("unexpected file")
    assert "still exists" in run(home, ["uninstall", "demo", "--keep-data"])
    assert artifact.read_text() == "unexpected file" and registry.read_bytes() == before
    artifact.unlink()
    manifest(artifact, "demo")
    registry.write_text("{ broken registry")
    assert "failed to load registry" in run(home, ["uninstall", "demo"])
    assert artifact.is_dir() and registry.read_text() == "{ broken registry"
    registry = register(home, artifact, {"type": "Local", "source_path": str(home / "source")},
                        {"demo": {"version": "1.0.0"}, "sibling": {"version": "1.0.0"}})
    before = registry.read_bytes()
    assert "--confirm" in run(home, ["uninstall", "demo"])
    assert artifact.is_dir() and registry.read_bytes() == before
    run(home, ["uninstall", "demo", "--confirm", "--keep-data"], success=True)
    assert not artifact.exists()

with tempfile.TemporaryDirectory(prefix="grow-cli-update-") as root:
    home = Path(root)
    source = home / "source"
    remote = home / "remote.git"
    artifact = home / ".grow/installed-plugins/fixture"
    manifest(source / "original", "demo")
    git(home, "init", "--bare", "--initial-branch=main", str(remote))
    git(source, "init", "--initial-branch=main")
    git(source, "config", "user.name", "CLI fixture")
    git(source, "config", "user.email", "cli-fixture@example.invalid")
    git(source, "add", ".")
    git(source, "commit", "-m", "initial")
    git(source, "remote", "add", "origin", str(remote))
    git(source, "push", "-u", "origin", "main")
    artifact.parent.mkdir(parents=True)
    git(home, "clone", "--branch", "main", str(remote), str(artifact))
    old = git(artifact, "rev-parse", "HEAD")
    registry = register(home, artifact, {"type": "Git", "url": str(remote), "git_ref": "main", "commit": old},
                        {"demo": {"version": "1.0.0", "subdir": "original"}})
    before = registry.read_bytes()
    manifest(source / "original", "demo", "2.0.0")
    manifest(source / "added", "added")
    git(source, "add", ".")
    git(source, "commit", "-m", "advance")
    git(source, "push", "origin", "main")
    tip = git(source, "rev-parse", "HEAD")
    registry.write_text("{ broken registry")
    assert "failed to load registry" in run(home, ["update", "demo"])
    assert git(artifact, "rev-parse", "HEAD") == old and registry.read_text() == "{ broken registry"
    registry.unlink()
    registry.mkdir()
    assert "failed to load registry" in run(home, ["update", "demo"])
    assert git(artifact, "rev-parse", "HEAD") == old and registry.is_dir()
    registry.rmdir()
    registry.write_bytes(before)
    text = run(home, ["update", "demo"], fault=True)
    assert "incomplete" in text and "test-injected registry save failure" in text
    assert git(artifact, "rev-parse", "HEAD") == tip and registry.read_bytes() == before
    assert ": updated (" in run(home, ["update", "demo"], success=True)
    entry = json.loads(registry.read_text())["repos"]["fixture"]
    assert entry["kind"]["commit"] == tip and set(entry["plugins"]) == {"demo", "added"}
    before = registry.read_bytes()
    assert "already up to date" in run(home, ["update", "demo"], fault=True, success=True)
    assert registry.read_bytes() == before
print("Standalone CLI: all 11 failure/confirmation/retry/no-op invocations passed", flush=True)
