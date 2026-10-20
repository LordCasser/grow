from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BUILD_RS = ROOT / "crates/codegen/cli/build.rs"
OHOS_SCRIPT = ROOT / "scripts/build-ohos.sh"
COMMIT_A = "ABCDEF0123456789abcdef0123456789abcdef01"
COMMIT_B = "12345678abcdef0123456789abcdef0123456789"


def write(path: Path, text: str, *, executable: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    if executable:
        path.chmod(0o755)


@unittest.skipUnless(os.name == "posix", "Cargo Git-shim fixture depends on POSIX PATH and /tmp semantics")
class ReleaseBuildIdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.temp = tempfile.TemporaryDirectory(prefix="grow-release-identity-", dir="/tmp")
        cls.work = Path(cls.temp.name)
        cls.fixture = cls.work / "fixture"
        cls.fixture.mkdir()
        cls.target = cls.work / "cargo-target"
        cls.cargo_home = cls.work / "cargo-home"
        cls.cargo_home.mkdir()
        cls.cargo = shutil.which("cargo")
        cls.rustc = shutil.which("rustc")
        cls.git = shutil.which("git")
        cls.bash = shutil.which("bash")
        if not all((cls.cargo, cls.rustc, cls.git, cls.bash)):
            raise unittest.SkipTest("cargo, rustc, git, and bash are required")

        # Cargo compiles the repository's production build script directly.
        write(
            cls.fixture / "Cargo.toml",
            """[package]
name = "release-identity-fixture"
version = "1.0.0"
edition = "2021"
build = "{build_rs}"

[workspace]
""".format(build_rs=BUILD_RS),
        )
        write(cls.fixture / "src/main.rs", 'fn main() { println!("{}", env!("VERSION_WITH_COMMIT")); }\n')

        # A temporary PATH shim makes Git unavailable for the explicit-input
        # cases while leaving the selected host Rust toolchain accessible.
        cls.no_git_bin = cls.work / "no-git-bin"
        write(cls.no_git_bin / "git", "#!/bin/sh\nexit 127\n", executable=True)

        # The fallback case uses an actual local repository and commit.
        subprocess.run([cls.git, "init", "-q", str(cls.fixture)], check=True)
        subprocess.run([cls.git, "-C", str(cls.fixture), "add", "Cargo.toml", "src/main.rs"], check=True)
        subprocess.run(
            [
                cls.git,
                "-C",
                str(cls.fixture),
                "-c",
                "user.name=Release Identity Test",
                "-c",
                "user.email=release-identity@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-q",
                "-m",
                "fixture",
            ],
            check=True,
        )

    @classmethod
    def tearDownClass(cls) -> None:
        cls.temp.cleanup()

    def cargo_env(self, *, git_unavailable: bool = False) -> dict[str, str]:
        env = os.environ.copy()
        env.update(
            {
                "CARGO_HOME": str(self.cargo_home),
                "CARGO_TARGET_DIR": str(self.target),
                "CARGO_NET_OFFLINE": "true",
                "RUSTC": self.rustc or "rustc",
                "GROW_VERSION": "4.5.6",
            }
        )
        env.pop("RELEASE_COMMIT", None)
        if git_unavailable:
            env["PATH"] = f"{self.no_git_bin}{os.pathsep}{env['PATH']}"
        return env

    def build_and_run(self, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
        build = subprocess.run(
            [self.cargo or "cargo", "build", "--offline", "--quiet"],
            cwd=self.fixture,
            env=env,
            text=True,
            capture_output=True,
        )
        if build.returncode != 0:
            return build
        run = subprocess.run(
            [str(self.target / "debug/release-identity-fixture")],
            cwd=self.fixture,
            env=env,
            text=True,
            capture_output=True,
        )
        return subprocess.CompletedProcess(run.args, run.returncode, run.stdout, run.stderr)

    def test_explicit_hash_works_without_git_and_rebuilds_on_same_target(self) -> None:
        env = self.cargo_env(git_unavailable=True)
        env["RELEASE_COMMIT"] = COMMIT_A
        first = self.build_and_run(env)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(first.stdout.strip(), "4.5.6 (abcdef0)")

        # Reuse the same Cargo target and change only the explicit identity.
        env["RELEASE_COMMIT"] = COMMIT_B
        second = self.build_and_run(env)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(second.stdout.strip(), "4.5.6 (1234567)")

    def test_empty_malformed_and_non_unicode_explicit_values_fail(self) -> None:
        for value in ("", "abc", "g" * 40):
            with self.subTest(value=value):
                env = self.cargo_env(git_unavailable=True)
                env["RELEASE_COMMIT"] = value
                result = subprocess.run(
                    [self.cargo or "cargo", "build", "--offline", "--quiet"],
                    cwd=self.fixture,
                    env=env,
                    text=True,
                    capture_output=True,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("RELEASE_COMMIT", result.stderr)

        if os.name == "posix":
            env = self.cargo_env(git_unavailable=True)
            byte_env = {os.fsencode(key): os.fsencode(value) for key, value in env.items()}
            byte_env[b"RELEASE_COMMIT"] = b"\xff" * 40
            result = subprocess.run(
                [self.cargo or "cargo", "build", "--offline", "--quiet"],
                cwd=self.fixture,
                env=byte_env,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("RELEASE_COMMIT must be Unicode", result.stderr)

    def test_absent_explicit_value_uses_git_fallback(self) -> None:
        env = self.cargo_env()
        expected_commit = subprocess.check_output(
            [self.git or "git", "-C", str(self.fixture), "rev-parse", "--short", "HEAD"],
            text=True,
        ).strip()
        result = self.build_and_run(env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), f"4.5.6 ({expected_commit})")


@unittest.skipUnless(os.name == "posix", "OHOS smoke integration requires POSIX")
class OhosSmokeIdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.temp = tempfile.TemporaryDirectory(prefix="grow-ohos-smoke-identity-")
        cls.work = Path(cls.temp.name)
        cls.bash = shutil.which("bash")
        if cls.bash is None:
            raise unittest.SkipTest("bash is required")

    @classmethod
    def tearDownClass(cls) -> None:
        cls.temp.cleanup()

    def run_ohos_smoke(
        self,
        *,
        release_commit: str | None,
        version_output: str,
        grow_version: str | None = "2.4.1",
    ) -> subprocess.CompletedProcess[str]:
        if os.uname().machine != "aarch64" or not Path("/lib/ld-musl-aarch64.so.1").exists():
            raise unittest.SkipTest("OHOS --smoke integration requires the aarch64 musl container")
        smoke_root = self.work / "ohos-smoke"
        sdk = smoke_root / "sdk"
        target = smoke_root / "target"
        home = smoke_root / "home"
        for path in (
            sdk / "native/llvm/bin",
            sdk / "native/build-tools/cmake/bin",
            sdk / "native/sysroot/usr",
            target / "release",
            home,
        ):
            path.mkdir(parents=True, exist_ok=True)
        write(
            target / "release/grow",
            "#!/bin/sh\n"
            'case "$1" in\n'
            '  --version) printf "%s\\n" "$SMOKE_VERSION_OUTPUT" ;;\n'
            '  doctor) exit 0 ;;\n'
            '  *) exit 0 ;;\n'
            'esac\n',
            executable=True,
        )

        env = os.environ.copy()
        env.update(
            {
                "OHOS_SDK_ROOT": str(sdk),
                "CARGO_TARGET_DIR": str(target),
                "CARGO_PROFILE": "release",
                "HOME": str(home),
                "SMOKE_VERSION_OUTPUT": version_output,
            }
        )
        env.pop("RELEASE_COMMIT", None)
        env.pop("GROW_VERSION", None)
        if release_commit is not None:
            env["RELEASE_COMMIT"] = release_commit
        if grow_version is not None:
            env["GROW_VERSION"] = grow_version
        return subprocess.run(
            [self.bash or "bash", str(OHOS_SCRIPT), "--smoke"],
            cwd=ROOT,
            env=env,
            text=True,
            capture_output=True,
        )

    def test_ohos_smoke_checks_full_identity_only_when_release_commit_is_supplied(self) -> None:
        expected = self.run_ohos_smoke(
            release_commit=COMMIT_A,
            version_output="grow 2.4.1 (abcdef0)",
        )
        self.assertEqual(expected.returncode, 0, expected.stderr)

        wrong = self.run_ohos_smoke(
            release_commit=COMMIT_A,
            version_output="grow 2.4.1 (unknown)",
        )
        self.assertNotEqual(wrong.returncode, 0)
        self.assertIn("unexpected version identity", wrong.stderr)

        malformed = self.run_ohos_smoke(release_commit="", version_output="grow 2.4.1 (unknown)")
        self.assertNotEqual(malformed.returncode, 0)
        self.assertIn("RELEASE_COMMIT", malformed.stderr)

        missing_version = self.run_ohos_smoke(
            release_commit=COMMIT_A,
            version_output="grow 2.4.1 (abcdef0)",
            grow_version=None,
        )
        self.assertNotEqual(missing_version.returncode, 0)
        self.assertIn("GROW_VERSION is required", missing_version.stderr)

        manual = self.run_ohos_smoke(
            release_commit=None,
            version_output="grow 2.4.1 (unknown)",
        )
        self.assertEqual(manual.returncode, 0, manual.stderr)


if __name__ == "__main__":
    unittest.main()
