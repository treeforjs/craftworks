"""Validate the release signing configuration without printing private material."""

import base64
import binascii
import os
from pathlib import Path
import subprocess
import sys
import tempfile


SPKI_PREFIX = bytes.fromhex("302a300506032b6570032100")


def main():
    expected = os.environ.get("CRAFTWORKS_UPDATE_PUBLIC_KEY") or os.environ.get(
        "CRAFT_UPDATE_PUBLIC_KEY", ""
    )
    try:
        public_key = base64.b64decode(expected.strip(), validate=True)
    except (ValueError, binascii.Error):
        public_key = b""
    if len(public_key) == 32:
        public_key = SPKI_PREFIX + public_key
    if len(public_key) != 44 or not public_key.startswith(SPKI_PREFIX):
        raise ValueError("Set CRAFT_UPDATE_PUBLIC_KEY to the base64 Ed25519 public key.")

    private_key = os.environ.get("UPDATE_SIGNING_KEY", "")
    if not private_key.strip():
        raise ValueError("Set the release environment secret UPDATE_SIGNING_KEY to the complete private PEM.")

    with tempfile.TemporaryDirectory(prefix="craftworks-signing-check-") as directory:
        key_path = Path(directory) / "key.pem"
        key_path.write_text(private_key, encoding="utf-8")
        key_path.chmod(0o600)
        try:
            result = subprocess.run(
                ["openssl", "pkey", "-in", str(key_path), "-passin", "pass:", "-pubout", "-outform", "DER"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                check=True,
                timeout=15,
            )
        except (OSError, subprocess.SubprocessError) as error:
            raise ValueError("UPDATE_SIGNING_KEY must contain an unencrypted Ed25519 private PEM.") from error
        if result.stdout != public_key:
            raise ValueError("UPDATE_SIGNING_KEY does not match the configured Craftworks public key.")
    print("Release signing key matches the trusted public key.")


if __name__ == "__main__":
    try:
        main()
    except ValueError as error:
        print(f"::error::{error}", file=sys.stderr)
        sys.exit(1)
