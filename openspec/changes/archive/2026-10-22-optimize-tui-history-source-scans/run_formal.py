"""Run measured groups sequentially after compilation and regression checks."""
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = next(p for p in HERE.parents if (p / "Cargo.toml").is_file())
BINS = (ROOT / "target/tui-scan-release/binaries").resolve()
PTY = ROOT / "openspec/changes/archive/2026-10-22-defer-historical-markdown-presentation/run_pty_pairs.py"
base = [sys.executable, str(PTY), str(BINS / "after-pty_e2e_persistence"), str(BINS / "before-grow"), str(BINS / "after-grow"), "--runs", "3", "--turns", "128,512,2048", "--slow-turns", "512,2048"]
for command in [base + ["--output", str(HERE / "pty-default-off"), "--instrumentation-off"],
                base + ["--output", str(HERE / "pty-log")],
                [sys.executable, str(HERE / "run_micro_pairs.py")],
                [sys.executable, str(HERE / "run_semantic_pairs.py")],
                [sys.executable, str(HERE / "run_rpc_pairs.py")]]:
    subprocess.run(command, check=True, cwd=ROOT / "crates/codegen/pager" if command[1] == str(PTY) else ROOT)
