#!/usr/bin/env python3
"""Narrow old/new application control, reusing an existing Shell rlib and dependencies.

This compiles the complete original plugin.rs, not the original Shell/ACP/CLI.
Run after the fixed Shell integration tests build:
  python run-baseline-control.py --shell-rlib /path/to/debug/deps/libshell-HASH.rlib
Use the matching rustc on PATH. GNU-compatible lld must be available.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

BASELINE = "8fbc52185ce30e38d97745dd6597242da1135c31"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--shell-rlib", type=Path, required=True)
args = parser.parse_args()
rlib = args.shell_rlib.resolve()
root = rlib.parent.parent
fingerprint = root / ".fingerprint" / rlib.stem.removeprefix("lib")
metadata = json.loads((fingerprint / "lib-shell.json").read_text())
needed = {"agent", "plugin_marketplace", "dirs", "chrono", "tracing", "toml_edit", "toml", "config"}
command = ["rustc", "--edition=2024", "-C", "debuginfo=0", "-C", "link-arg=-fuse-ld=lld",
           "-L", f"dependency={root / 'deps'}", "--extern", f"shell={rlib}"]
for _, name, _, expected in metadata["deps"]:
    if name not in needed:
        continue
    for item in (root / ".fingerprint").glob(f"*/lib-{name}"):
        actual = int.from_bytes(bytes.fromhex(item.read_text().strip()), "little")
        candidate = root / "deps" / f"lib{name}-{item.parent.name.rsplit('-', 1)[1]}.rlib"
        if actual == expected and candidate.is_file():
            command += ["--extern", f"{name}={candidate}"]
            print(f"dependency {name}: {candidate}", flush=True)
            break
    else:
        raise RuntimeError(f"Matching built dependency unavailable: {name}")
for path in (root / "build").glob("*/out"):
    if any(path.glob("*.a")):
        command += ["-L", f"native={path}"]
with tempfile.TemporaryDirectory(prefix="grow-plugin-baseline-") as build:
    build = Path(build)
    original = subprocess.check_output(["git", "show", f"{BASELINE}:crates/codegen/shell/src/plugin.rs"])
    (build / "plugin.rs").write_bytes(original)
    (build / "control.rs").write_bytes(Path(__file__).with_name("baseline-control.rs").read_bytes())
    binary = build / "control"
    subprocess.run(command + [str(build / "control.rs"), "-o", str(binary)], check=True)
    for phase in ["uninstall-save", "uninstall-remove", "update-save"]:
        for mode in ["old", "new"]:
            with tempfile.TemporaryDirectory(prefix="grow-plugin-control-") as home:
                home = Path(home)
                (home / ".grow").mkdir()
                env = os.environ.copy()
                env.update(HOME=str(home), GROW_HOME=str(home / ".grow"),
                           GROW_MARKETPLACE_REQUIRE_SHA="0", GIT_CONFIG_NOSYSTEM="1",
                           GIT_CONFIG_GLOBAL=str(home / "absent-gitconfig"))
                env.pop("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", None)
                output = subprocess.run([str(binary), mode, phase], cwd=home, env=env,
                                        capture_output=True, text=True, timeout=30)
                print(f"=== {mode} {phase}: exit {output.returncode}", flush=True)
                print(output.stdout, end="", flush=True)
                print(output.stderr, end="", flush=True)
                assert output.returncode == (101 if mode == "old" else 0)
                if mode == "old":
                    assert "operation falsely reported success" in output.stderr
                    assert "result=Ok(())" in output.stdout
    print("3 baseline controls failed as expected; all 3 fixed controls passed")
