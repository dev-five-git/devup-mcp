"""Release lifecycle regressions using real Git commits and an isolated API."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
from urllib.parse import unquote
import unittest

SPEC = importlib.util.spec_from_file_location("delivery", Path(__file__).with_name("release.py"))
delivery = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(delivery)


class FakeGitHub:
    def __init__(self, head):
        self.head = head
        self.tags, self.releases, self.uploaded, self.calls = {}, [], {}, []

    def request(self, method, path, body=None):
        self.calls.append((method, path, copy.deepcopy(body)))
        if method == "GET" and path == "git/ref/heads/main":
            return {"object": {"sha": self.head}}
        if method == "GET" and path.startswith("git/ref/tags/"):
            tag = unquote(path.split("git/ref/tags/", 1)[1])
            if tag not in self.tags:
                raise delivery.ApiError(404, path)
            return {"object": {"type": "commit", "sha": self.tags[tag]}}
        if method == "POST" and path == "git/refs":
            self.tags[body["ref"].removeprefix("refs/tags/")] = body["sha"]
            return {}
        if method == "GET" and path.startswith("releases?"):
            return copy.deepcopy(self.releases)
        if method == "POST" and path == "releases/generate-notes":
            return {"body": "Generated release notes"}
        if method == "POST" and path == "releases":
            release = {**body, "id": len(self.releases) + 1}
            release["upload_url"] = f"https://uploads.github.com/repos/example/repo/releases/{release['id']}/assets{{?name,label}}"
            self.releases.append(release)
            return copy.deepcopy(release)
        if method == "DELETE" and path.startswith("releases/assets/"):
            asset_id = int(path.split("/")[-1])
            for items in self.uploaded.values():
                items[:] = [item for item in items if item.get("id") != asset_id]
            return None
        if path.startswith("releases/"):
            release_id = int(path.split("/")[1])
            release = next(item for item in self.releases if item["id"] == release_id)
            if method == "GET" and "/assets?" in path:
                return copy.deepcopy(self.uploaded.get(release_id, []))
            if method == "GET":
                return copy.deepcopy(release)
            if method == "PATCH":
                release.update(body)
                return copy.deepcopy(release)
        raise AssertionError((method, path, body))

    def upload(self, release, file):
        data = file.read_bytes()
        item = {"name": file.name, "size": len(data), "state": "uploaded", "digest": "sha256:" + hashlib.sha256(data).hexdigest()}
        self.uploaded.setdefault(release["id"], []).append(item)
        return item


class DeliveryTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="devup-release-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / ".changepacks").mkdir()
        (self.root / delivery.SERVER).parent.mkdir(parents=True)
        (self.root / delivery.SERVER).write_text('[package]\nname="devup-mcp"\nversion.workspace=true\n', encoding="utf-8")
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion="0.20.0"\n', encoding="utf-8")
        self.command("init", "-b", "main")
        self.command("config", "user.name", "release test")
        self.command("config", "user.email", "release@example.invalid")
        self.command("config", "commit.gpgsign", "false")
        self.head = self.commit("version")
        self.api = FakeGitHub(self.head)

    def command(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True, encoding="utf-8", stderr=subprocess.PIPE).strip()

    def commit(self, message):
        self.command("add", ".")
        self.command("commit", "-m", message)
        return self.command("rev-parse", "HEAD")

    def prepare(self):
        return delivery.prepare(self.root, self.api, self.head)

    def pending(self):
        return self.prepare()["pending_releases"][delivery.SERVER]

    def make_assets(self, saved):
        for name in delivery.expected_assets(saved["version"]):
            file = self.root / name
            file.write_bytes((name + " contents").encode())
            delivery.upload(self.api, saved, file)

    def test_actual_workspace_only_release_gets_canonical_draft_and_crate_upload_key(self):
        self.api.releases.append({"id": 433388180, "tag_name": "devup-mcp(Cargo.toml)@0.20.0", "draft": False, "body": "All 22 feedback items and 14 follow-ups"})
        result = self.prepare()
        saved = result["pending_releases"][delivery.SERVER]
        release = self.api.releases[-1]
        self.assertTrue(release["draft"])
        self.assertEqual(release["tag_name"], delivery.PREFIX + "0.20.0")
        self.assertEqual(release["body"], self.api.releases[0]["body"])
        self.assertEqual(saved["sourceSha"], self.head)
        self.assertEqual(result["release_sha"], self.head)
        self.assertEqual(list(result["release_assets_urls"]), [delivery.SERVER])
        self.assertEqual(self.api.releases[0]["draft"], False, "existing workspace release is preserved")

    def test_pending_version_change_never_releases_unbumped_code(self):
        (self.root / ".changepacks/changepack_log_pending.json").write_text("{}")
        self.assertEqual(self.prepare()["pending_releases"], {})
        self.assertEqual(self.api.calls, [])

    def test_superseded_workflow_does_not_create_release_or_tag(self):
        self.api.head = "a" * 40
        self.assertEqual(self.prepare()["pending_releases"], {})
        self.assertEqual(self.api.tags, {})
        self.assertEqual(self.api.releases, [])

    def test_draft_retry_uses_original_commit_after_main_moves(self):
        first = self.pending()
        (self.root / "README.md").write_text("later unrelated documentation")
        self.api.head = self.head = self.commit("docs")
        retry = self.prepare()
        self.assertEqual(retry["release_sha"], first["sourceSha"])
        self.assertEqual(retry["pending_releases"][delivery.SERVER]["releaseId"], first["releaseId"])
        self.assertEqual(len(self.api.releases), 1)

    def test_tag_for_wrong_workspace_version_is_refused(self):
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion="0.21.0"\n')
        self.api.head = self.head = self.commit("new version")
        self.api.tags[delivery.PREFIX + "0.21.0"] = self.command("rev-parse", "HEAD^")
        with self.assertRaisesRegex(ValueError, "different workspace version"):
            self.prepare()
        self.assertEqual(self.api.releases, [])

    def test_public_complete_release_is_a_noop(self):
        saved = self.pending()
        self.make_assets(saved)
        delivery.finalize(self.api, saved)
        self.assertEqual(self.prepare()["pending_releases"], {})
        self.assertEqual(len(self.api.releases), 1)

    def test_public_incomplete_release_cannot_be_mistaken_for_success(self):
        self.pending()
        self.api.releases[0]["draft"] = False
        with self.assertRaisesRegex(ValueError, "missing a complete asset"):
            self.prepare()

    def test_identical_upload_retry_is_idempotent_but_changed_bytes_are_refused(self):
        saved = self.pending()
        file = self.root / "devup-mcp-linux-x86_64"
        file.write_bytes(b"binary")
        delivery.upload(self.api, saved, file)
        delivery.upload(self.api, saved, file)
        self.assertEqual(len(self.api.uploaded[saved["releaseId"]]), 1)
        file.write_bytes(b"edited binary")
        with self.assertRaisesRegex(ValueError, "differs"):
            delivery.upload(self.api, saved, file)

    def test_wrong_release_source_never_uploads_or_publishes(self):
        saved = self.pending()
        self.api.releases[0]["target_commitish"] = "b" * 40
        with self.assertRaisesRegex(ValueError, "identity changed"):
            delivery.finalize(self.api, saved)
        self.assertTrue(self.api.releases[0]["draft"])

    def test_interrupted_empty_draft_upload_is_recovered(self):
        saved = self.pending()
        self.api.uploaded[saved["releaseId"]] = [{"id": 42, "name": "devup-mcp-linux-x86_64", "size": 0, "state": "starter"}]
        file = self.root / "devup-mcp-linux-x86_64"
        file.write_bytes(b"complete binary")
        delivery.upload(self.api, saved, file)
        self.assertEqual(len(self.api.uploaded[saved["releaseId"]]), 1)
        self.assertEqual(self.api.uploaded[saved["releaseId"]][0]["state"], "uploaded")

    def test_corrupt_upload_receipt_keeps_release_private(self):
        saved = self.pending()
        file = self.root / "devup-mcp-linux-x86_64"
        file.write_bytes(b"binary")
        original_upload = self.api.upload
        def corrupt(release, file):
            uploaded = original_upload(release, file)
            uploaded["digest"] = "sha256:" + "0" * 64
            return uploaded
        self.api.upload = corrupt
        with self.assertRaisesRegex(ValueError, "size/digest"):
            delivery.upload(self.api, saved, file)
        self.assertTrue(self.api.releases[0]["draft"])

    def test_tag_moved_during_build_cannot_publish_old_binaries(self):
        saved = self.pending()
        self.make_assets(saved)
        self.api.tags[saved["tagName"]] = "a" * 40
        with self.assertRaisesRegex(ValueError, "tag changed"):
            delivery.finalize(self.api, saved)
        self.assertTrue(self.api.releases[0]["draft"])

    def test_uncommitted_version_cannot_create_a_tag(self):
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion="0.21.0"\n')
        with self.assertRaisesRegex(ValueError, "uncommitted"):
            self.prepare()
        self.assertEqual(self.api.tags, {})

    def test_finalize_requires_all_three_binaries_and_bundle_then_marks_latest(self):
        saved = self.pending()
        with self.assertRaisesRegex(ValueError, "missing a complete asset"):
            delivery.finalize(self.api, saved)
        self.assertTrue(self.api.releases[0]["draft"])
        self.make_assets(saved)
        delivery.finalize(self.api, saved)
        self.assertFalse(self.api.releases[0]["draft"])
        self.assertEqual(self.api.releases[0]["make_latest"], "true")
        calls = len(self.api.calls)
        delivery.finalize(self.api, saved)
        self.assertFalse(any(method == "PATCH" for method, _, _ in self.api.calls[calls:]))

    def test_unexpected_or_empty_asset_is_refused(self):
        saved = self.pending()
        file = self.root / "not-an-mcp-asset"
        file.write_bytes(b"unexpected")
        with self.assertRaisesRegex(ValueError, "Unexpected"):
            delivery.upload(self.api, saved, file)
        file = self.root / "devup-mcp-linux-x86_64"
        file.touch()
        with self.assertRaisesRegex(ValueError, "empty"):
            delivery.upload(self.api, saved, file)


if __name__ == "__main__":
    unittest.main()
