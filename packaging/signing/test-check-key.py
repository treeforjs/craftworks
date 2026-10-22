"""Exercise signing configuration checks with disposable keys only."""

import base64
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


CHECK = Path(__file__).with_name("check-key.py")


class SigningConfigurationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="craftworks-signing-test-")
        directory = Path(cls.temporary.name).resolve()
        cls.private_keys = []
        cls.public_keys = []
        for number in range(2):
            path = directory / f"test-{number}.pem"
            subprocess.run(
                ["openssl", "genpkey", "-algorithm", "ED25519", "-out", str(path)],
                check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            cls.private_keys.append(path.read_text(encoding="utf-8"))
            public = subprocess.check_output(
                ["openssl", "pkey", "-in", str(path), "-pubout", "-outform", "DER"],
                stderr=subprocess.DEVNULL,
            )
            cls.public_keys.append(public)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def check_configuration(self, expected_success, **overrides):
        environment = dict(os.environ)
        environment.update(
            CRAFTWORKS_UPDATE_PUBLIC_KEY="",
            CRAFT_UPDATE_PUBLIC_KEY=base64.b64encode(self.public_keys[0]).decode(),
            UPDATE_SIGNING_KEY=self.private_keys[0],
        )
        environment.update(overrides)
        result = subprocess.run(
            [sys.executable, str(CHECK)], env=environment, capture_output=True,
            text=True, timeout=20,
        )
        if any(private in result.stdout + result.stderr for private in self.private_keys):
            raise AssertionError("Private material appeared in signing-check output.")
        self.assertEqual(result.returncode == 0, expected_success)

    def test_matching_der_and_raw_public_keys(self):
        self.check_configuration(True)
        raw = base64.b64encode(self.public_keys[0][-32:]).decode()
        self.check_configuration(True, CRAFT_UPDATE_PUBLIC_KEY=raw)

    def test_app_specific_public_key_overrides_the_shared_key(self):
        other = base64.b64encode(self.public_keys[1]).decode()
        self.check_configuration(False, CRAFTWORKS_UPDATE_PUBLIC_KEY=other)

    def test_missing_placeholder_and_mismatched_private_keys_are_rejected(self):
        self.check_configuration(False, UPDATE_SIGNING_KEY="")
        self.check_configuration(False, UPDATE_SIGNING_KEY="Private Key")
        self.check_configuration(False, UPDATE_SIGNING_KEY=self.private_keys[1])

    def test_missing_malformed_and_wrong_algorithm_public_keys_are_rejected(self):
        self.check_configuration(False, CRAFT_UPDATE_PUBLIC_KEY="")
        self.check_configuration(False, CRAFT_UPDATE_PUBLIC_KEY="not a public key")
        wrong_prefix = base64.b64encode(bytes(44)).decode()
        self.check_configuration(False, CRAFT_UPDATE_PUBLIC_KEY=wrong_prefix)


if __name__ == "__main__":
    unittest.main()
