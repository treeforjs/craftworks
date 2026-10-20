"""Offline checks for authenticated feed collection; no network or real token."""

import importlib.util
import io
import pathlib
import tempfile
import unittest
import urllib.request

spec = importlib.util.spec_from_file_location("feeds", pathlib.Path(__file__).with_name("prepare-release-feeds.py"))
feeds = importlib.util.module_from_spec(spec)
spec.loader.exec_module(feeds)


class FakeOpener:
    def __init__(self, body=b"[]"):
        self.body = body
        self.requests = []

    def open(self, request, timeout):
        self.requests.append(request)
        return io.BytesIO(self.body)


class FeedsTest(unittest.TestCase):
    def test_authentication_is_only_sent_to_fixed_public_api_feeds(self):
        opener = FakeOpener()
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            feeds.fetch_feeds(directory, "throwaway-fixture", opener)
            self.assertEqual({path.name for path in directory.iterdir()}, {f"{name}.json" for name in feeds.REPOSITORIES})
        self.assertEqual(len(opener.requests), 4)
        for request in opener.requests:
            self.assertTrue(request.full_url.startswith("https://api.github.com/repos/treeforjs/"))
            self.assertEqual(request.get_header("Authorization"), "Bearer throwaway-fixture")

    def test_invalid_or_oversized_feed_is_not_written(self):
        original = feeds.MAX_FEED_BYTES
        try:
            feeds.MAX_FEED_BYTES = 4
            for body in (b"{}", b"[1,2,3]"):
                with tempfile.TemporaryDirectory() as temporary:
                    directory = pathlib.Path(temporary)
                    with self.assertRaises(ValueError):
                        feeds.fetch_feeds(directory, "fixture", FakeOpener(body))
                    self.assertEqual(list(directory.iterdir()), [])
        finally:
            feeds.MAX_FEED_BYTES = original

    def test_credentials_cannot_follow_cross_origin_redirects(self):
        handler = feeds.SameOriginRedirect()
        request = urllib.request.Request("https://api.github.com/repos/treeforjs/craftworks/releases")
        for target in ("https://api.github.com.evil.invalid/", "https://github.com/", "http://api.github.com/"):
            with self.assertRaises(ValueError):
                handler.redirect_request(request, None, 302, "Found", {}, target)


if __name__ == "__main__":
    unittest.main()
