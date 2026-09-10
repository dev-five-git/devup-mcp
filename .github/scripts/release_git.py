#!/usr/bin/env python3
"""CI-only adapter for changepacks/action's automatic version commit.

The action has no post-update hook and commits with --no-verify. Run Cargo
before Git constructs its commit tree; a prepare-commit-msg hook is too late.
Every other Git operation is delegated unchanged to the captured real binary.
"""
import os
from pathlib import Path
import subprocess
import sys


def main():
    real_git = os.environ["DEVUP_RELEASE_REAL_GIT"]
    if Path(real_git).resolve() == Path(__file__).resolve():
        raise RuntimeError("release Git adapter cannot delegate to itself")
    args = sys.argv[1:]
    if os.environ.get("DEVUP_RELEASE_LOCK_SYNC") == "true":
        if args == ["commit", "-m", "Update Versions", "--no-verify"]:
            branch = subprocess.check_output([real_git, "branch", "--show-current"], text=True).strip()
            if branch == "changepacks/main":
                # Updates workspace package versions while retaining locked
                # external dependencies. Stage before the real git commit.
                subprocess.run(["cargo", "update", "--workspace"], check=True)
                subprocess.run([real_git, "add", "--", "Cargo.lock"], check=True)
        elif args == ["push", "--force", "origin", "changepacks/main"]:
            # Fail closed if upstream changes its commit command and stops
            # matching the adapter. Never push a stale lockfile version PR.
            subprocess.run(["cargo", "metadata", "--locked", "--format-version", "1"], stdout=subprocess.DEVNULL, check=True)
            subprocess.run([real_git, "diff", "--exit-code", "HEAD", "--", "Cargo.toml", "Cargo.lock"], check=True)
    return subprocess.call([real_git, *args])


if __name__ == "__main__":
    sys.exit(main())
