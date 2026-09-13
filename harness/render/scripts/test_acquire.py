import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import acquire


class AcquisitionFailures(unittest.TestCase):
    def test_refused_frame_is_recorded_and_later_frame_is_acquired(self):
        class FakeServer:
            def export(self, arguments, allow_error=False):
                if "node-id=1-1" in arguments["url"]:
                    return {"code": "DEVUP_FIXTURE_REFUSED", "message": "capture refused"}
                paths = arguments.get("outputPaths", {})
                for kind, path in paths.items():
                    destination = Path(acquire.HARNESS, path)
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.write_text("{}" if kind == "rawSnapshot" else "new", encoding="utf-8")
                return {"status": "complete", "quality": {}, "outputPaths": paths}

        with tempfile.TemporaryDirectory() as directory, patch.object(acquire, "HARNESS", directory):
            manifest = {"targets": [], "assets": {}}
            with contextlib.redirect_stdout(io.StringIO()):
                acquire.acquire(FakeServer(), "example", {"output": "tsx", "frames": ["1:1", "1:2"]}, manifest)
            self.assertEqual([t["frame"] for t in manifest["targets"]], ["1:2"])
            self.assertEqual(manifest["skipped"][0]["frame"], "1:1")
            self.assertIn("capture refused", json.dumps(manifest["skipped"][0]))

    def test_failed_export_cannot_reuse_old_files(self):
        class FakeServer:
            def export(self, arguments, allow_error=False):
                return {"status": "failed", "message": "no output this time"}

        with tempfile.TemporaryDirectory() as directory, patch.object(acquire, "HARNESS", directory):
            for relative in ["out/example-1-1-1-1.snapshot.json", "out/example-1-1.reference.png", "themes/example-1-1.json", "src/screens/example-1-1.tsx"]:
                path = Path(directory, relative)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("{}", encoding="utf-8")
            manifest = {"targets": [], "assets": {}}
            with contextlib.redirect_stdout(io.StringIO()):
                acquire.acquire(FakeServer(), "example", {"output": "tsx", "frames": ["1:1"]}, manifest)
            self.assertEqual(manifest["targets"], [])
            self.assertEqual(len(manifest["skipped"]), 1)


if __name__ == "__main__":
    unittest.main()
