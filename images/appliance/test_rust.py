import subprocess
import unittest


class TestRust(unittest.TestCase):

    def test_rustc_version(self):
        version = subprocess.run(
            ["rustc", "--version"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

        self.assertIn("nightly", version)
