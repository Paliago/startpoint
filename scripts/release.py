"""Cross-platform release tooling. Python 3.11+; standard library only.

Build jobs never publish. The publish job validates both platform artifacts before
creating/resuming a draft and marks it public only after every upload succeeds.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import secrets
import shutil
import subprocess
import tempfile
import tomllib
import urllib.error
import urllib.parse
import urllib.request
import zipfile

APP_ID = "Paliago.Startpoint.App"
BUNDLE_ID = "com.paliago.startpoint"
PLATFORMS = {
    "win-x64": "win-x64-stable",
    "osx-arm64": "osx-arm64-stable",
}
ROOT = Path(__file__).resolve().parent.parent
WINDOWS_SIGNING = ("STARTPOINT_SIGNING_METADATA_JSON", "AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET")
MAC_SIGNING = (
    "MACOS_APP_CERTIFICATE_BASE64", "MACOS_INSTALLER_CERTIFICATE_BASE64",
    "MACOS_CERTIFICATE_PASSWORD", "MACOS_APP_SIGNING_IDENTITY", "MACOS_INSTALLER_SIGNING_IDENTITY",
    "APPLE_ID", "APPLE_APP_PASSWORD", "APPLE_TEAM_ID",
)


class ReleaseError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def version_tuple(version):
    require(re.fullmatch(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)", version), "Use a stable three-part version, e.g. 0.1.0.")
    return tuple(map(int, version.split(".")))


def valid_repository(repository):
    return bool(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository)) and all(p not in (".", "..") for p in repository.split("/"))


def safe_name(name):
    return bool(re.fullmatch(r"[A-Za-z0-9_.-]+", name)) and ".." not in name


def run(*args, secret=False, capture=False):
    # Never print commands: signing commands can contain credentials.
    result = subprocess.run([str(a) for a in args], cwd=ROOT, text=True, capture_output=secret or capture)
    require(result.returncode == 0, f"{Path(str(args[0])).name} failed (exit {result.returncode})." + (" Check signing credentials." if secret else ""))
    return result.stdout.strip() if capture else None


def git(*args):
    return run("git", *args, capture=True)


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def file_record(path):
    path = Path(path)
    return {"name": path.name, "size": path.stat().st_size, "sha256": sha256(path)}


def output(values):
    print(json.dumps(values, indent=2))
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a", encoding="utf-8") as stream:
            for key, value in values.items():
                stream.write(f"{key}={str(value).lower() if isinstance(value, bool) else value}\n")


def metadata(args):
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    version = cargo["workspace"]["package"]["version"]
    version_tuple(version)
    packages = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    for name in ("startpoint", "startpoint-core", "startpoint-sdk"):
        require(any(p["name"] == name and p["version"] == version for p in packages), f"Update Cargo.lock: {name} must be version {version}.")
    desktop = tomllib.loads((ROOT / "crates/desktop/Cargo.toml").read_text(encoding="utf-8"))
    require(desktop["dependencies"]["velopack"]["version"] == "=" + args.vpk_version, "Cargo and the packaging CLI must use the same pinned Velopack version.")
    commit = git("rev-parse", "HEAD")
    require(re.fullmatch(r"[0-9a-f]{40}", commit), "Unexpected source commit.")
    # fetch-depth: 0 gives us origin/main and the annotated tag contents.
    git("merge-base", "--is-ancestor", commit, "origin/main")
    publish = os.environ.get("PUBLISH_REQUESTED", "false") == "true"
    tag = os.environ.get("RELEASE_TAG", "")
    allow_unsigned = os.environ.get("ALLOW_UNSIGNED", "false") == "true" or not publish
    if tag:
        require(tag == "v" + version, f"Tag {tag} must match Cargo version v{version}.")
        require(git("rev-parse", f"refs/tags/{tag}^{{commit}}") == commit, "Build must be checked out at the requested tag.")
        annotation = git("for-each-ref", "--format=%(contents)", f"refs/tags/{tag}")
        allow_unsigned |= "[unsigned-test]" in annotation.splitlines()
    else:
        require(not publish, "Publishing requires an existing version tag. Preview main first, then push the tag.")
        tag = "v" + version
    repository = os.environ.get("STARTPOINT_RELEASE_REPOSITORY", "Paliago/startpoint")
    require(valid_repository(repository), "Release repository must be owner/name.")
    with urllib.request.urlopen(f"https://api.github.com/repos/{repository}", timeout=30) as response:
        require(not json.load(response)["private"], "Release destination must be public for installed update checks.")
    output({"version": version, "tag": tag, "commit": commit, "publish": publish, "policy": "testing" if allow_unsigned else "production"})


def signing_ready(platform, policy, environment=None):
    env = os.environ if environment is None else environment
    names = WINDOWS_SIGNING if platform == "win-x64" else MAC_SIGNING
    present = [name for name in names if env.get(name)]
    if len(present) == len(names):
        return True
    require(not present, "Incomplete signing configuration; missing: " + ", ".join(n for n in names if not env.get(n)))
    require(policy == "testing", f"{platform} signing is not configured. Configure production signing or explicitly opt into an unsigned testing release.")
    return False


def inspect_archive(path):
    with zipfile.ZipFile(path) as archive:
        for entry in archive.infolist():
            name = entry.filename.replace("\\", "/")
            require(not name.startswith("/") and ".." not in name.split("/"), "Unsafe archive entry.")
            require(not name.lower().endswith((".ps1", ".pdb", ".p12", ".pfx")), "Private tooling, symbols or signing certificates leaked into a package.")
        require(archive.testzip() is None, "Corrupt package archive.")


def validate_packages(directory, platform, version):
    directory = Path(directory)
    channel = PLATFORMS[platform]
    feed_name = f"releases.{channel}.json"
    feed = read_json(directory / feed_name)
    assets = feed.get("Assets", [])
    require(len(assets) == 1, "Expected one full package and no delta dependencies.")
    asset = assets[0]
    expected_name = f"{APP_ID}-{version}-{channel}-full.nupkg"
    require(asset.get("PackageId") == APP_ID and asset.get("Version") == version and asset.get("Type") == "Full", "Unexpected package ID, version or type.")
    require(asset.get("FileName") == expected_name, "Package filename must match its version and architecture channel.")
    package = directory / expected_name
    require(not package.is_symlink(), "Package must be a regular file.")
    require(package.stat().st_size == asset.get("Size") and sha256(package).lower() == str(asset.get("SHA256", "")).lower(), "Package hash/size does not match the feed.")
    inspect_archive(package)
    installer_suffix = "-Setup.exe" if platform == "win-x64" else "-Setup.pkg"
    installers = list(directory.glob(f"{APP_ID}-{channel}{installer_suffix}"))
    portables = list(directory.glob(f"{APP_ID}-{channel}-Portable.zip"))
    require(len(installers) == 1 and len(portables) == 1, f"Missing/ambiguous installer or portable package for {channel}.")
    inspect_archive(portables[0])
    for path in (installers[0], portables[0]):
        require(path.is_file() and not path.is_symlink() and path.stat().st_size > 0 and safe_name(path.name), "Invalid installer/portable file.")
    return [directory / feed_name, package, installers[0], portables[0]]


def mac_bundle(stage, binary, icon_source, version):
    bundle = stage / "Startpoint.app"
    macos = bundle / "Contents/MacOS"
    resources = bundle / "Contents/Resources"
    macos.mkdir(parents=True)
    resources.mkdir()
    shutil.copy2(binary, macos / "startpoint")
    (macos / "startpoint").chmod(0o755)
    iconset = stage / "Startpoint.iconset"
    iconset.mkdir()
    for size in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            pixels = size * scale
            suffix = "@2x" if scale == 2 else ""
            run("sips", "-z", pixels, pixels, icon_source, "--out", iconset / f"icon_{size}x{size}{suffix}.png", capture=True)
    run("iconutil", "-c", "icns", iconset, "-o", resources / "startpoint.icns")
    with (bundle / "Contents/Info.plist").open("wb") as stream:
        plistlib.dump({
            "CFBundleIdentifier": BUNDLE_ID, "CFBundleName": "Startpoint", "CFBundleDisplayName": "Startpoint",
            "CFBundleExecutable": "startpoint", "CFBundlePackageType": "APPL", "CFBundleShortVersionString": version,
            "CFBundleVersion": version, "CFBundleIconFile": "startpoint.icns", "NSPrincipalClass": "NSApplication",
            "NSHighResolutionCapable": True, "NSSupportsAutomaticGraphicsSwitching": True,
            "LSMinimumSystemVersion": "14.0",
            "NSAppleEventsUsageDescription": "Startpoint may request permission to finish installing an application update.",
        }, stream)
    return bundle


def mac_signing_setup(directory):
    password = secrets.token_urlsafe(32)
    keychain = directory / "signing.keychain-db"
    old_keychains = run("security", "list-keychains", "-d", "user", capture=True)
    run("security", "create-keychain", "-p", password, keychain, secret=True)
    run("security", "set-keychain-settings", "-lut", "21600", keychain, secret=True)
    run("security", "unlock-keychain", "-p", password, keychain, secret=True)
    for name, filename in (("MACOS_APP_CERTIFICATE_BASE64", "app.p12"), ("MACOS_INSTALLER_CERTIFICATE_BASE64", "installer.p12")):
        certificate = directory / filename
        certificate.write_bytes(base64.b64decode("".join(os.environ[name].split()), validate=True))
        certificate.chmod(0o600)
        run("security", "import", certificate, "-P", os.environ["MACOS_CERTIFICATE_PASSWORD"], "-A", "-t", "cert", "-f", "pkcs12", "-k", keychain, secret=True)
    run("security", "list-keychains", "-d", "user", "-s", keychain, *re.findall(r'"([^"]+)"', old_keychains), secret=True)
    run("security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s", "-k", password, keychain, secret=True)
    run("xcrun", "notarytool", "store-credentials", "startpoint-release", "--keychain", keychain,
        "--apple-id", os.environ["APPLE_ID"], "--team-id", os.environ["APPLE_TEAM_ID"], "--password", os.environ["APPLE_APP_PASSWORD"], secret=True)
    return keychain


def package(args):
    version_tuple(args.version)
    require(re.fullmatch(r"[0-9a-f]{40}", args.commit), "Invalid source commit.")
    signed = signing_ready(args.platform, args.policy)
    output_dir = Path(args.output).resolve()
    require(not output_dir.exists() or not any(output_dir.iterdir()), "Refusing to overwrite existing release packages.")
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="startpoint-package-") as temp:
        stage = Path(temp)
        binary = ROOT / "target/release" / ("startpoint.exe" if args.platform == "win-x64" else "startpoint")
        require(binary.is_file(), "Build the release executable with --features self-update first.")
        keychain = None
        try:
            if args.platform == "win-x64":
                pack_dir = stage / "app"
                pack_dir.mkdir()
                shutil.copy2(binary, pack_dir / "startpoint.exe")
                icon = ROOT / "crates/desktop/assets/icons/startpoint.ico"
                extra = ["--mainExe", "startpoint.exe", "--icon", icon]
                if signed:
                    metadata_file = stage / "signing.json"
                    metadata_file.write_text(os.environ["STARTPOINT_SIGNING_METADATA_JSON"], encoding="utf-8")
                    extra += ["--azureTrustedSignFile", metadata_file]
            else:
                require(os.uname().sysname == "Darwin", "macOS packages must be built on a Mac.")
                run("lipo", "-verify_arch", "arm64", binary)
                pack_dir = mac_bundle(stage, binary, ROOT / "crates/desktop/assets/icons/startpoint.png", args.version)
                # Native Rust needs no JIT/unsigned-memory/App Sandbox entitlements.
                entitlements = stage / "entitlements.plist"
                with entitlements.open("wb") as stream:
                    plistlib.dump({}, stream)
                extra = ["--mainExe", "startpoint", "--signEntitlements", entitlements]
                if signed:
                    keychain = stage / "signing.keychain-db"
                    mac_signing_setup(stage)
                    extra += ["--signAppIdentity", os.environ["MACOS_APP_SIGNING_IDENTITY"],
                              "--signInstallIdentity", os.environ["MACOS_INSTALLER_SIGNING_IDENTITY"],
                              "--notaryProfile", "startpoint-release", "--keychain", keychain]
                else:
                    # Ad-hoc signing is needed for valid Apple Silicon bundles,
                    # but does NOT provide Developer ID trust or notarization.
                    extra += ["--signAppIdentity=-"]
            vpk = Path(args.vpk).resolve()
            if args.platform == "win-x64" and vpk.suffix != ".exe" and vpk.with_suffix(".exe").exists():
                vpk = vpk.with_suffix(".exe")
            run(vpk, "pack", "--packId", APP_ID, "--packTitle", "Startpoint", "--packAuthors", "Paliago",
                "--packVersion", args.version, "--packDir", pack_dir, "--runtime", args.platform,
                "--channel", PLATFORMS[args.platform], "--delta", "None", "--outputDir", output_dir, *extra)
            files = validate_packages(output_dir, args.platform, args.version)
            if args.platform == "win-x64" and signed:
                extracted = stage / "verify"
                with zipfile.ZipFile(next(p for p in files if p.suffix == ".zip")) as archive:
                    archive.extractall(extracted)
                # Avoid interpolation of filesystem paths into PowerShell source.
                os.environ["STARTPOINT_VERIFY_SETUP"] = str(next(p for p in files if p.suffix == ".exe"))
                os.environ["STARTPOINT_VERIFY_EXE"] = str(extracted / "current/startpoint.exe")
                os.environ["STARTPOINT_VERIFY_UPDATER"] = str(extracted / "Update.exe")
                run("powershell", "-NoProfile", "-NonInteractive", "-Command",
                    "$ErrorActionPreference='Stop'; foreach ($path in @($env:STARTPOINT_VERIFY_EXE, $env:STARTPOINT_VERIFY_SETUP, $env:STARTPOINT_VERIFY_UPDATER)) { "
                    "if ((Get-AuthenticodeSignature -LiteralPath $path).Status -ne 'Valid') { throw 'Invalid Windows signature' } }")
            if args.platform != "win-x64":
                extracted = stage / "verify"
                run("ditto", "-x", "-k", next(p for p in files if p.suffix == ".zip"), extracted)
                bundle = extracted / "Startpoint.app"
                run("codesign", "--verify", "--deep", "--strict", bundle)
                if signed:
                    run("xcrun", "stapler", "validate", bundle)
                    installer = next(p for p in files if p.suffix == ".pkg")
                    run("xcrun", "stapler", "validate", installer)
                    run("spctl", "--assess", "--type", "install", installer)
            write_json(output_dir / f"build.{PLATFORMS[args.platform]}.json", {
                "schema": 1, "version": args.version, "commit": args.commit, "platform": args.platform,
                "channel": PLATFORMS[args.platform], "signed": signed, "files": [file_record(p) for p in files],
            })
        finally:
            if keychain is not None and keychain.exists():
                run("security", "delete-keychain", keychain, secret=True)


def assemble_packages(directory, output_dir, version, commit, policy):
    version_tuple(version)
    directory, output_dir = Path(directory), Path(output_dir)
    require(not output_dir.exists() or not any(output_dir.iterdir()), "Assembled output must be empty.")
    manifests = list(directory.rglob("build.*.json"))
    require(len(manifests) == len(PLATFORMS), "Both platform artifacts must be present before publishing.")
    builds, names = {}, set()
    verified = []
    for path in manifests:
        manifest = read_json(path)
        platform = manifest.get("platform")
        require(platform in PLATFORMS and platform not in builds, "Unexpected/duplicate platform artifact.")
        require(manifest.get("schema") == 1 and manifest.get("channel") == PLATFORMS[platform], "Invalid build manifest.")
        require(manifest.get("version") == version and manifest.get("commit") == commit, "Artifacts must all come from the same tagged source commit and version.")
        require(isinstance(manifest.get("signed"), bool), "Missing signing status.")
        require(policy == "testing" or manifest["signed"], "Production release cannot contain unsigned packages.")
        files = validate_packages(path.parent, platform, version)
        require(manifest.get("files") == [file_record(p) for p in files], "Build artifact contents changed after verification.")
        for file in files:
            require(file.name not in names, "Release asset name collision across platforms.")
            names.add(file.name)
            verified.append(file)
        builds[platform] = manifest
    output_dir.mkdir(parents=True, exist_ok=True)
    for path in verified:
        shutil.copy2(path, output_dir / path.name)
    release_manifest = {"schema": 1, "version": version, "commit": commit, "builds": builds}
    write_json(output_dir / "release-manifest.json", release_manifest)
    records = [file_record(p) for p in sorted(output_dir.iterdir())]
    (output_dir / "SHA256SUMS").write_text("".join(f"{r['sha256']}  {r['name']}\n" for r in records), encoding="utf-8")
    return release_manifest


class GitHub:
    def __init__(self, repository, token):
        require(valid_repository(repository), "Invalid release repository.")
        require(token, "GITHUB_TOKEN / STARTPOINT_RELEASE_TOKEN is required to publish.")
        self.repository, self.token = repository, token

    def request(self, method, path, data=None, upload=None):
        if upload is not None:
            url = f"https://uploads.github.com/repos/{self.repository}/releases/{path}/assets?" + urllib.parse.urlencode({"name": upload.name})
            body = upload.read_bytes()
            content_type = "application/octet-stream"
        else:
            url = f"https://api.github.com/repos/{self.repository}/{path}"
            body = json.dumps(data).encode() if data is not None else None
            content_type = "application/json"
        request = urllib.request.Request(url, data=body, method=method, headers={
            "Authorization": "Bearer " + self.token, "Accept": "application/vnd.github+json",
            "Content-Type": content_type, "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "Startpoint-release",
        })
        try:
            with urllib.request.urlopen(request, timeout=300) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            if error.code == 404 and method == "GET":
                return None
            raise ReleaseError(f"GitHub {method} {path} failed (HTTP {error.code}); any draft remains unpublished.") from None


def release_notes(manifest, source_repository):
    version, commit = manifest["version"], manifest["commit"]
    signed = all(build["signed"] for build in manifest["builds"].values())
    warning = "" if signed else (
        "**Unsigned testing release — not a signed/notarized production distribution.**\n\n"
        "Windows may show SmartScreen warnings. macOS requires an explicit Gatekeeper approval; "
        "see [installation instructions](https://github.com/" + source_repository + "/blob/" + commit + "/docs/releases.md#install-and-test-010).\n\n"
    )
    return (f"<!-- startpoint-release:{commit} -->\n# Startpoint {version}\n\n" + warning +
            "One release includes **Windows x64** and **macOS Apple Silicon (arm64)**. Intel Macs are not supported.\n\n"
            "- Windows: download the `win-x64-stable-Setup.exe`, or extract the entire portable ZIP.\n"
            "- macOS: download your architecture's `.pkg`, or extract its portable ZIP and move `Startpoint.app` into `~/Applications`. macOS 14+ is required.\n"
            "- Installed apps check in the background; use **Download update → Restart to update**.\n"
            "- `SHA256SUMS` and `release-manifest.json` record package hashes and source provenance.\n\n"
            f"Source: https://github.com/{source_repository}/commit/{commit}\n")


def publish(args):
    manifest = read_json(Path(args.output) / "release-manifest.json")
    require(manifest["version"] == args.version and manifest["commit"] == args.commit, "Wrong assembled release.")
    version_tuple(args.version)
    require(valid_repository(args.source_repository), "Invalid source repository.")
    tag = "v" + args.version
    api = GitHub(args.repository, os.environ.get("GH_TOKEN"))
    existing = api.request("GET", "releases/tags/" + tag)
    require(existing is None or existing["draft"], "This version is already public; release assets/tags are immutable.")
    latest = api.request("GET", "releases/latest")
    if latest:
        require(latest["tag_name"].startswith("v"), "Latest release does not have a version tag.")
        require(version_tuple(latest["tag_name"][1:]) < version_tuple(args.version), "Refusing to move the stable release feed backwards.")
    body = release_notes(manifest, args.source_repository)
    if existing:
        require(f"<!-- startpoint-release:{args.commit} -->" in existing.get("body", ""), "Existing draft belongs to a different source commit; inspect it manually.")
    else:
        # In the default source repository, the version tag already exists.
        existing = api.request("POST", "releases", {
            "tag_name": tag, "name": f"Startpoint {args.version}", "body": body, "draft": True, "prerelease": False,
        })
    release_id = existing["id"]
    remote = {a["name"]: a for a in existing.get("assets", [])}
    files = sorted(Path(args.output).iterdir())
    require(all(p.is_file() and not p.is_symlink() and safe_name(p.name) for p in files), "Invalid assembled release asset.")
    require(not (set(remote) - {p.name for p in files}), "Unexpected assets in draft; refusing to publish it.")
    for path in files:
        expected = file_record(path)
        if asset := remote.get(path.name):
            digest = asset.get("digest")
            if not digest:
                # Public asset reads deliberately do not carry the publishing token.
                with urllib.request.urlopen(asset["browser_download_url"], timeout=300) as response:
                    digest = "sha256:" + hashlib.sha256(response.read()).hexdigest()
            require(asset["size"] == expected["size"] and digest.lower() == "sha256:" + expected["sha256"], "Draft asset differs from rebuilt output. Never overwrite it; inspect the unpublished draft.")
        else:
            uploaded = api.request("POST", str(release_id), upload=path)
            require(uploaded["size"] == expected["size"] and uploaded.get("digest", "").lower() == "sha256:" + expected["sha256"], "Uploaded asset hash/size mismatch; draft remains unpublished.")
    public = api.request("PATCH", f"releases/{release_id}", {"draft": False, "make_latest": "true", "body": body})
    print("Published " + public["html_url"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    meta = commands.add_parser("metadata")
    meta.add_argument("--vpk-version", required=True)
    ready = commands.add_parser("signing")
    ready.add_argument("--platform", choices=PLATFORMS, required=True)
    ready.add_argument("--policy", choices=("testing", "production"), required=True)
    pack = commands.add_parser("package")
    pack.add_argument("--platform", choices=PLATFORMS, required=True)
    pack.add_argument("--vpk", required=True)
    pack.add_argument("--policy", choices=("testing", "production"), required=True)
    assemble = commands.add_parser("assemble")
    assemble.add_argument("--packages", required=True)
    assemble.add_argument("--policy", choices=("testing", "production"), required=True)
    publication = commands.add_parser("publish")
    publication.add_argument("--repository", required=True)
    publication.add_argument("--source-repository", required=True)
    for command in (pack, assemble, publication):
        command.add_argument("--version", required=True)
        command.add_argument("--commit", required=True)
        command.add_argument("--output", required=True)
    args = parser.parse_args()
    try:
        if args.command == "metadata":
            metadata(args)
        elif args.command == "signing":
            output({"signed": signing_ready(args.platform, args.policy)})
        elif args.command == "package":
            package(args)
        elif args.command == "assemble":
            assemble_packages(args.packages, args.output, args.version, args.commit, args.policy)
        else:
            publish(args)
    except (ReleaseError, OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Release error: {error}\n")


if __name__ == "__main__":
    main()
