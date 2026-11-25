import io
import json
import os
import sys
import unittest
from contextlib import redirect_stdout
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import podman_check


def fake_which(name):
    if name == "apt-get":
        return "/usr/bin/apt-get"
    return None


class DryRunTest(unittest.TestCase):
    def test_dry_run_reports_without_executing(self):
        buffer = io.StringIO()
        with mock.patch.object(podman_check.shutil, "which", side_effect=fake_which):
            with redirect_stdout(buffer):
                code = podman_check.main(["--dry-run", "--json"])
        self.assertEqual(code, 0)
        report = json.loads(buffer.getvalue())
        self.assertEqual(report["result"], "dry-run")
        statuses = {step["status"] for step in report["steps"]}
        self.assertIn("skip", statuses)
        self.assertNotIn("fail", statuses)

    def test_no_install_fails_when_podman_missing(self):
        buffer = io.StringIO()
        with mock.patch.object(podman_check.shutil, "which", side_effect=fake_which):
            with mock.patch.object(podman_check.Checker, "podman_version", return_value=None):
                with redirect_stdout(buffer):
                    code = podman_check.main(["--no-install", "--json"])
        self.assertEqual(code, 1)
        report = json.loads(buffer.getvalue())
        self.assertEqual(report["result"], "fail")

    def test_uses_given_image(self):
        buffer = io.StringIO()
        with mock.patch.object(podman_check.shutil, "which", side_effect=fake_which):
            with redirect_stdout(buffer):
                podman_check.main(["--dry-run", "--json", "--image", "alpine:edge"])
        report = json.loads(buffer.getvalue())
        self.assertEqual(report["image"], "alpine:edge")
        run = [s for s in report["steps"] if s["name"] == "container run"][0]
        self.assertEqual(run["command"][-1], "alpine:edge")


if __name__ == "__main__":
    unittest.main()
