import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import asc_profiles


class ApiUrlTests(unittest.TestCase):
    def test_rejects_external_pagination_url(self):
        with self.assertRaises(ValueError):
            asc_profiles.api_url("https://example.com/steal")

    def test_allows_apple_pagination_url(self):
        self.assertEqual(
            asc_profiles.api_url("https://api.appstoreconnect.apple.com/v1/devices"),
            "https://api.appstoreconnect.apple.com/v1/devices",
        )

    def test_allows_relative_api_path(self):
        self.assertEqual(asc_profiles.api_url("/devices"), asc_profiles.API + "/devices")


if __name__ == "__main__":
    unittest.main()
