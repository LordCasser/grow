"""Link the audit probe to cached libraries; do not rebuild workspace crates."""
from pathlib import Path
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
# Archived changes have one additional directory component.
if not (ROOT / "Cargo.toml").is_file():
    ROOT = ROOT.parent
DEPS = ROOT / "target/debug/deps"
FINGERPRINTS = ROOT / "target/debug/.fingerprint"
samplers = list(DEPS.glob("libsampler-*.rlib"))
assert len(samplers) == 1, "Expected exactly one cached sampler; select the build explicitly."
sampler = samplers[0]
sampler_hash = sampler.stem.removeprefix("libsampler-")
metadata = json.loads((FINGERPRINTS / f"sampler-{sampler_hash}/lib-sampler.json").read_text())
libraries = {"sampler": sampler}
for _, name, _, checksum in metadata["deps"]:
    if name not in {"tokio", "futures_util", "serde_json", "sampling_types"}:
        continue
    matches = []
    for fingerprint in FINGERPRINTS.glob(f"*/lib-{name}"):
        if int.from_bytes(bytes.fromhex(fingerprint.read_text().strip()), "little") == checksum:
            crate_hash = fingerprint.parent.name.rsplit("-", 1)[1]
            library = DEPS / f"lib{name}-{crate_hash}.rlib"
            if library.is_file():
                matches.append(library)
    assert len(matches) == 1, f"Expected one matching cached dependency: {name}"
    libraries[name] = matches[0]
assert len(libraries) == 5
with tempfile.TemporaryDirectory(prefix="grow-completion-state-probe-") as directory:
    binary = Path(directory) / "probe"
    args = ["rustc", "--edition=2024", "-C", "debuginfo=0", "-C", "strip=debuginfo",
            "-L", f"dependency={DEPS}", str(Path(__file__).with_name("probe.rs")), "-o", str(binary)]
    for name, library in libraries.items():
        args += ["--extern", f"{name}={library}"]
    for native in (ROOT / "target/debug/build").glob("*/out"):
        args += ["-L", f"native={native}"]
    subprocess.run(args, cwd=ROOT, check=True)
    print(f"Temporary probe executable: {binary.stat().st_size} bytes; removed on exit.", flush=True)
    subprocess.run([str(binary)], cwd=ROOT, check=True)
