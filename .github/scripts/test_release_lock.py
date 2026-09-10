"""Exercise the actual automatic commit with a real temporary Git/Cargo workspace."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
WRAPPER = ROOT / ".github/scripts/release_git.py"


class ReleaseLockTest(unittest.TestCase):
    def test_version_commit_contains_updated_lock_without_dependency_churn(self):
        with tempfile.TemporaryDirectory(prefix="devup-r5-release-") as directory:
            repo = Path(directory)
            for name in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]:
                shutil.copyfile(ROOT / name, repo / name)
            manifest = tomllib.loads((repo / "Cargo.toml").read_text(encoding="utf-8"))
            for member in manifest["workspace"]["members"]:
                crate = repo / member
                (crate / "src").mkdir(parents=True)
                shutil.copyfile(ROOT / member / "Cargo.toml", crate / "Cargo.toml")
                (crate / "src/lib.rs").write_text("")
            git = shutil.which("git")
            self.assertIsNotNone(git)
            env = os.environ.copy()
            # No compilation or alternate target; use only cached registry data.
            env["CARGO_NET_OFFLINE"] = "true"
            env["DEVUP_RELEASE_REAL_GIT"] = git
            env["DEVUP_RELEASE_LOCK_SYNC"] = "true"
            def run(*args, check=True):
                return subprocess.run(args, cwd=repo, env=env, text=True, encoding="utf-8", capture_output=True, check=check)
            run(git, "init", "-b", "main")
            run(git, "config", "user.name", "release test")
            run(git, "config", "user.email", "release-test@example.invalid")
            run(git, "config", "commit.gpgsign", "false")
            run(git, "add", ".")
            run(git, "commit", "-m", "baseline")
            baseline = (repo / "Cargo.lock").read_bytes()
            run("cargo", "metadata", "--locked", "--format-version", "1")
            run(git, "checkout", "-b", "changepacks/main")
            text = (repo / "Cargo.toml").read_text(encoding="utf-8")
            current = manifest["workspace"]["package"]["version"]
            major, minor, patch = map(int, current.split("."))
            following = f"{major}.{minor}.{patch + 1}"
            (repo / "Cargo.toml").write_text(text.replace(f'version = "{current}"', f'version = "{following}"', 1), encoding="utf-8")
            # CI must fail without rewriting the stale lock before any build.
            self.assertNotEqual(run("cargo", "metadata", "--locked", "--format-version", "1", check=False).returncode, 0)
            self.assertEqual((repo / "Cargo.lock").read_bytes(), baseline)
            run(git, "add", ".")
            result = run(sys.executable, str(WRAPPER), "commit", "-m", "Update Versions", "--no-verify", check=False)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            committed = tomllib.loads(run(git, "show", "HEAD:Cargo.lock").stdout)
            original = tomllib.loads(baseline.decode())
            names = {tomllib.loads((repo / m / "Cargo.toml").read_text(encoding="utf-8"))["package"]["name"] for m in manifest["workspace"]["members"]}
            local = [p for p in committed["package"] if p["name"] in names and "source" not in p]
            self.assertEqual(len(local), 4)
            self.assertEqual({p["version"] for p in local}, {following})
            self.assertEqual([p for p in committed["package"] if "source" in p], [p for p in original["package"] if "source" in p])
            run("cargo", "metadata", "--locked", "--format-version", "1")
            self.assertEqual(run(git, "status", "--porcelain").stdout, "")
            # Ordinary Git invocations are passed through unchanged.
            self.assertEqual(run(sys.executable, str(WRAPPER), "rev-parse", "HEAD").stdout, run(git, "rev-parse", "HEAD").stdout)


if __name__ == "__main__":
    unittest.main()
