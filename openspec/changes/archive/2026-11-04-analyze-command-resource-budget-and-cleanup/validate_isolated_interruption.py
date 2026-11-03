"""Bounded evidence for the disposable-copy protocol; no user workspace mutation."""
import hashlib
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

with tempfile.TemporaryDirectory(prefix="grow-mutation-baseline-") as baseline_dir:
    baseline = pathlib.Path(baseline_dir)
    source = baseline / "source.txt"
    source.write_text("original\n")
    original = hashlib.sha256(source.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="grow-mutation-trial-") as trial_dir:
        trial = pathlib.Path(trial_dir) / "copy"
        shutil.copytree(baseline, trial)
        worker = subprocess.Popen([
            sys.executable, "-c",
            "import pathlib,time; pathlib.Path('source.txt').write_text('mutation\\n'); "
            "pathlib.Path('ready').touch(); time.sleep(8)",
        ], cwd=trial)
        try:
            deadline = time.monotonic() + 2
            while not (trial / "ready").exists():
                if worker.poll() is not None or time.monotonic() >= deadline:
                    raise AssertionError("mutation worker failed to reach interruption barrier")
                time.sleep(0.01)
            worker.terminate()
            worker.wait(timeout=2)
            assert worker.returncode != 0, "interruption is incomplete evidence, never a passed mutant"
            assert (trial / "source.txt").read_text() != source.read_text()
            assert hashlib.sha256(source.read_bytes()).hexdigest() == original
        finally:
            if worker.poll() is None:
                worker.kill()
                worker.wait(timeout=2)
    assert not pathlib.Path(trial_dir).exists(), "disposable trial removed after interruption"
    assert hashlib.sha256(source.read_bytes()).hexdigest() == original
print("PASS: interrupted trial is incomplete; baseline unchanged; disposable copy removed")
