"""Fetch public metadata with CI's token; downloads still use the real HTTPS source."""

import json
import os
from pathlib import Path
import urllib.request

MAX_FEED_BYTES = 4 << 20
REPOSITORIES = ("craftworks", "pdfcraft", "vectorcraft", "photocraft")


class SameOriginRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        if not newurl.startswith("https://api.github.com/"):
            raise ValueError("Refusing to forward release-feed credentials to another origin")
        return super().redirect_request(request, fp, code, msg, headers, newurl)


def fetch_feeds(directory, token, opener):
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    for repository in REPOSITORIES:
        per_page = 20 if repository == "craftworks" else 100
        request = urllib.request.Request(
            f"https://api.github.com/repos/treeforjs/{repository}/releases?per_page={per_page}",
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {token}",
                "User-Agent": "Craftworks-release-check",
            },
        )
        with opener.open(request, timeout=30) as response:
            body = response.read(MAX_FEED_BYTES + 1)
        if len(body) > MAX_FEED_BYTES or not isinstance(json.loads(body), list):
            raise ValueError("Release feed has an invalid type or size")
        (directory / f"{repository}.json").write_bytes(body)


if __name__ == "__main__":
    token = os.environ.get("GH_TOKEN", "").strip()
    if not token:
        raise SystemExit("Set GH_TOKEN for authenticated CI release discovery")
    directory = Path(os.environ["CRAFT_RELEASE_FEEDS"])
    if not directory.is_absolute():
        raise SystemExit("CRAFT_RELEASE_FEEDS must be an absolute directory")
    fetch_feeds(directory, token, urllib.request.build_opener(SameOriginRedirect()))
