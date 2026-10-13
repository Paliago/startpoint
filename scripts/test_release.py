"""Offline tests for package validation and fail-closed publication."""
import argparse
import json
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import release

COMMIT = "a" * 40


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.packages = self.root / "packages"
        self.output = self.root / "assembled"

    def fixture(self, platform, signed=False):
        channel = release.PLATFORMS[platform]
        directory = self.packages / platform
        directory.mkdir(parents=True)
        package = directory / f"{release.APP_ID}-0.1.0-{channel}-full.nupkg"
        with zipfile.ZipFile(package, "w") as archive:
            archive.writestr("lib/app/startpoint", b"fixture binary")
        portable = directory / f"{release.APP_ID}-{channel}-Portable.zip"
        with zipfile.ZipFile(portable, "w") as archive:
            archive.writestr("Startpoint.app/Contents/MacOS/startpoint", b"fixture binary")
        installer = directory / f"{release.APP_ID}-{channel}-Setup{'.exe' if platform == 'win-x64' else '.pkg'}"
        installer.write_bytes(b"fixture installer")
        feed = directory / f"releases.{channel}.json"
        release.write_json(feed, {"Assets": [{
            "PackageId": release.APP_ID, "Version": "0.1.0", "Type": "Full",
            "FileName": package.name, "Size": package.stat().st_size, "SHA256": release.sha256(package),
        }]})
        release.write_json(directory / f"build.{channel}.json", {
            "schema": 1, "version": "0.1.0", "commit": COMMIT, "platform": platform,
            "channel": channel, "signed": signed,
            "files": [release.file_record(p) for p in (feed, package, installer, portable)],
        })
        return directory

    def all_platforms(self, signed=False):
        return [self.fixture(platform, signed) for platform in release.PLATFORMS]

    def assemble(self, policy="testing"):
        return release.assemble_packages(self.packages, self.output, "0.1.0", COMMIT, policy)

    def publish_args(self):
        return argparse.Namespace(output=str(self.output), version="0.1.0", commit=COMMIT,
                                  repository="owner/app", source_repository="owner/app")

    def test_one_release_contains_every_channel_and_checksums(self):
        self.all_platforms()
        manifest = self.assemble()
        self.assertEqual(set(manifest["builds"]), set(release.PLATFORMS))
        self.assertEqual(len(list(self.output.iterdir())), 4 * len(release.PLATFORMS) + 2)
        sums = (self.output / "SHA256SUMS").read_text()
        for platform, channel in release.PLATFORMS.items():
            feed = self.output / f"releases.{channel}.json"
            self.assertIn(release.sha256(feed), sums)

    def test_missing_platform_cannot_assemble(self):
        self.fixture("win-x64")
        with self.assertRaises(release.ReleaseError):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_mixed_source_commit_cannot_assemble(self):
        directories = self.all_platforms()
        path = next(directories[-1].glob("build.*.json"))
        manifest = release.read_json(path)
        manifest["commit"] = "b" * 40
        release.write_json(path, manifest)
        with self.assertRaises(release.ReleaseError):
            self.assemble()

    def test_changed_package_cannot_assemble(self):
        directories = self.all_platforms()
        next(directories[0].glob("*.nupkg")).write_bytes(b"corrupted")
        with self.assertRaises(release.ReleaseError):
            self.assemble()

    def test_changed_installer_cannot_assemble(self):
        directories = self.all_platforms()
        next(directories[0].glob("*.exe")).write_bytes(b"replaced")
        with self.assertRaises(release.ReleaseError):
            self.assemble()

    def test_wrong_feed_channel_is_rejected(self):
        directory = self.fixture("osx-arm64")
        path = next(directory.glob("releases.*.json"))
        feed = release.read_json(path)
        feed["Assets"][0]["FileName"] = feed["Assets"][0]["FileName"].replace("osx-arm64", "win-x64")
        release.write_json(path, feed)
        with self.assertRaises(release.ReleaseError):
            release.validate_packages(directory, "osx-arm64", "0.1.0")

    def test_production_rejects_unsigned_artifacts(self):
        self.all_platforms()
        with self.assertRaises(release.ReleaseError):
            self.assemble("production")

    def test_production_accepts_signed_artifacts(self):
        self.all_platforms(signed=True)
        self.assemble("production")

    def test_missing_or_partial_signing_fails_closed(self):
        for platform in release.PLATFORMS:
            self.assertFalse(release.signing_ready(platform, "testing", {}))
            with self.assertRaises(release.ReleaseError):
                release.signing_ready(platform, "production", {})
            names = release.WINDOWS_SIGNING if platform == "win-x64" else release.MAC_SIGNING
            with self.assertRaises(release.ReleaseError):
                release.signing_ready(platform, "testing", {names[0]: "configured"})
            self.assertTrue(release.signing_ready(platform, "production", dict.fromkeys(names, "configured")))

    def test_unsafe_archive_and_private_files_are_rejected(self):
        for name in ("../escape", "/absolute", "secret.p12", "local.ps1", "startpoint.pdb"):
            archive = self.root / "unsafe.zip"
            with zipfile.ZipFile(archive, "w") as stream:
                stream.writestr(name, b"fixture")
            with self.assertRaises(release.ReleaseError):
                release.inspect_archive(archive)

    def test_only_stable_versions_and_safe_repositories(self):
        self.assertEqual(release.version_tuple("0.1.0"), (0, 1, 0))
        for version in ("01.1.0", "0.1", "v0.1.0", "0.1.0-beta", "0.1.0+test"):
            with self.assertRaises(release.ReleaseError):
                release.version_tuple(version)
        for repository in ("../app", "owner/..", "owner/app/extra", "https://github.com/owner/app"):
            self.assertFalse(release.valid_repository(repository))

    def test_only_windows_and_apple_silicon_are_release_targets(self):
        self.assertEqual(release.PLATFORMS, {
            "win-x64": "win-x64-stable",
            "osx-arm64": "osx-arm64-stable",
        })

    def test_mac_packaging_uses_valid_architecture_and_adhoc_options(self):
        fixture = self.fixture("osx-arm64")
        binary = self.root / "target/release/startpoint"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"fixture binary")
        args = argparse.Namespace(platform="osx-arm64", version="0.1.0", commit=COMMIT,
                                  policy="testing", vpk=str(self.root / "vpk"), output=str(self.output))

        def command(*arguments, **kwargs):
            if Path(str(arguments[0])).name == "vpk":
                entitlements = Path(arguments[arguments.index("--signEntitlements") + 1])
                self.assertEqual(entitlements.suffix, ".entitlements")
                with entitlements.open("rb") as stream:
                    self.assertEqual(plistlib.load(stream), {})
                self.assertIn("--signAppIdentity=-", arguments)
                for option in ("--signInstallIdentity", "--notaryProfile", "--keychain"):
                    self.assertNotIn(option, arguments)
                for path in release.validate_packages(fixture, "osx-arm64", "0.1.0"):
                    (self.output / path.name).write_bytes(path.read_bytes())

        with patch.object(release, "ROOT", self.root), \
                patch.object(release.os, "uname", create=True, return_value=argparse.Namespace(sysname="Darwin")), \
                patch.object(release, "signing_ready", return_value=False), \
                patch.object(release, "mac_bundle", return_value=self.root / "Startpoint.app"), \
                patch.object(release, "run", side_effect=command) as run:
            release.package(args)
            self.assertEqual(run.call_args_list[0].args, ("lipo", binary, "-verify_arch", "arm64"))
        manifest = release.read_json(self.output / "build.osx-arm64-stable.json")
        self.assertEqual(manifest["platform"], "osx-arm64")
        self.assertEqual(manifest["commit"], COMMIT)
        self.assertFalse(manifest["signed"])

    def test_failed_mac_architecture_check_stops_before_bundling(self):
        binary = self.root / "target/release/startpoint"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"wrong architecture fixture")
        args = argparse.Namespace(platform="osx-arm64", version="0.1.0", commit=COMMIT,
                                  policy="testing", vpk=str(self.root / "vpk"), output=str(self.output))
        with patch.object(release, "ROOT", self.root), \
                patch.object(release.os, "uname", create=True, return_value=argparse.Namespace(sysname="Darwin")), \
                patch.object(release, "signing_ready", return_value=False), \
                patch.object(release, "mac_bundle") as bundle, \
                patch.object(release, "run", side_effect=release.ReleaseError("lipo failed")):
            with self.assertRaisesRegex(release.ReleaseError, "lipo failed"):
                release.package(args)
            bundle.assert_not_called()
        self.assertFalse((self.output / "build.osx-arm64-stable.json").exists())

    def test_upload_failure_keeps_release_draft(self):
        self.all_platforms()
        self.assemble()
        calls = []

        def request(method, path, data=None, upload=None):
            calls.append((method, path, data))
            if method == "GET":
                return None
            if path == "releases":
                self.assertTrue(data["draft"])
                return {"id": 123, "assets": []}
            raise release.ReleaseError("interrupted upload")

        with patch.object(release, "GitHub") as api:
            api.return_value.request.side_effect = request
            with self.assertRaises(release.ReleaseError):
                release.publish(self.publish_args())
        self.assertFalse(any(method == "PATCH" for method, _, _ in calls))

    def test_already_public_version_is_immutable(self):
        self.all_platforms()
        self.assemble()
        with patch.object(release, "GitHub") as api:
            api.return_value.request.return_value = {"draft": False}
            with self.assertRaises(release.ReleaseError):
                release.publish(self.publish_args())
            api.return_value.request.assert_called_once()

    def test_publish_cannot_move_latest_backwards(self):
        self.all_platforms()
        self.assemble()
        with patch.object(release, "GitHub") as api:
            api.return_value.request.side_effect = [None, {"tag_name": "v0.2.0"}]
            with self.assertRaises(release.ReleaseError):
                release.publish(self.publish_args())
            self.assertEqual(api.return_value.request.call_count, 2)

    def test_publication_happens_after_all_verified_uploads(self):
        self.all_platforms()
        self.assemble()
        calls = []

        def request(method, path, data=None, upload=None):
            calls.append((method, path))
            if method == "GET":
                return None
            if path == "releases":
                return {"id": 123, "assets": []}
            if upload:
                return {"size": upload.stat().st_size, "digest": "sha256:" + release.sha256(upload)}
            self.assertEqual(data["draft"], False)
            return {"html_url": "https://github.com/owner/app/releases/tag/v0.1.0"}

        with patch.object(release, "GitHub") as api:
            api.return_value.request.side_effect = request
            release.publish(self.publish_args())
        self.assertEqual(calls[-1], ("PATCH", "releases/123"))
        self.assertEqual(sum(path == "123" for _, path in calls), 4 * len(release.PLATFORMS) + 2)


if __name__ == "__main__":
    unittest.main()
