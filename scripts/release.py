#!/usr/bin/env python3
"""Dependency-free release validation and allowlisted source export (Python 3.9+)."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
PUBLIC_FILES = [
    ".gitignore", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "build.rs",
    "README.md", "LICENSE", "CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md",
    "docs/install.md", "docs/releasing.md", "docs/architecture.md",
    "docs/images/ui-v0.2.1.png", "scripts/package-macos.sh", "scripts/release.py",
    "scripts/test_release.py", ".github/workflows/ci.yml",
    ".github/workflows/release.yml", ".github/dependabot.yml",
    ".github/ISSUE_TEMPLATE/bug_report.yml", ".github/pull_request_template.md",
    "src/macos_dock.m", "examples/icon_preview.rs",
    "third-party/manifest.json",
]


def version(root=ROOT):
    manifest = (root / "Cargo.toml").read_text()
    match = re.search(r'^version = "(\d+\.\d+\.\d+)"$', manifest, re.M)
    build = re.search(r'^build-number = ([1-9]\d*)$', manifest, re.M)
    if not match or not build:
        raise ValueError("Cargo.toml needs a stable SemVer and positive release build-number")
    return match[1], int(build[1])


def sources(root=ROOT):
    files = [root / name for name in PUBLIC_FILES]
    files += sorted((root / "src").rglob("*.rs"))
    overrides = json.loads((root / "third-party/manifest.json").read_text())
    files += sorted({root / item["path"] for entry in overrides.values() for item in entry["files"]})
    for path in files:
        if path.is_symlink() or not path.is_file() or root.resolve() not in path.resolve().parents:
            raise ValueError("Missing or unsafe release source: " + str(path.relative_to(root)))
    return files


def check(root=ROOT, tag=None):
    current, build = version(root)
    lock = (root / "Cargo.lock").read_text()
    if not re.search(r'name = "codex-account-hub"\nversion = "' + re.escape(current) + r'"', lock):
        raise ValueError("Cargo.toml and Cargo.lock versions differ")
    if tag is not None and tag != "v" + current:
        raise ValueError("Release tag must be v" + current)
    if "## [" + current + "]" not in (root / "CHANGELOG.md").read_text():
        raise ValueError("Add a CHANGELOG.md entry for " + current)
    files = sources(root)
    for path in files:
        if path.suffix in (".md", ".yml", ".toml"):
            text = path.read_text()
            if re.search(r"/Users/[^/\s]+/|/home/[^/\s]+/", text):
                raise ValueError("Personal absolute path in " + str(path.relative_to(root)))
        if path.suffix == ".yml" and path.parent.name == "workflows":
            for action in re.findall(r"uses:\s+([^\s#]+)", path.read_text()):
                if not re.fullmatch(r"[\w.-]+/[\w.-]+@[0-9a-f]{40}", action):
                    raise ValueError("Action must be pinned to a full commit: " + action)
    return current, build, files


def export_source(root=ROOT):
    current, _, files = check(root)
    (root / "dist").mkdir(exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix="source-" + current + "-", dir=root / "dist"))
    stage = output / "codex-account-hub"
    stage.mkdir()
    for source in files:
        destination = stage / source.relative_to(root)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
    archive = output / ("Codex-Account-Hub-" + current + "-source.zip")
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zipped:
        for source in files:
            relative = source.relative_to(root)
            zipped.write(stage / relative, Path("codex-account-hub") / relative)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_suffix(".zip.sha256").write_text(digest + "  " + archive.name + "\n")
    print("Clean source directory:", stage)
    print("Source archive:", archive)


def dependency_inventory(metadata, destination):
    """Collect declared licenses and packaged notices; flag omissions for review."""
    data = json.loads(Path(metadata).read_text())
    lines = ["# Dependency license inventory", "", "Generated from Cargo's resolved metadata.",
             "Review distribution obligations and include required notices before public release.", "",
             "| Package | Version | Declared license | Source |", "| --- | --- | --- | --- |"]
    notices = ["THIRD-PARTY NOTICES", "", "Resolved dependency license texts, as packaged by their authors.", ""]
    missing = []
    review = []
    overrides = json.loads((ROOT / "third-party/manifest.json").read_text())
    for package in sorted(data["packages"], key=lambda item: (item["name"], item["version"])):
        if package["name"] == "codex-account-hub":
            continue
        source = package.get("repository") or "https://crates.io/crates/" + package["name"]
        lines.append("| {} | {} | {} | {} |".format(package["name"], package["version"],
                     (package.get("license") or "REVIEW REQUIRED").replace("|", "\\|"), source))
        directory = Path(package["manifest_path"]).parent.resolve()
        candidates = set()
        for path in directory.rglob("*"):
            if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "OFL", "UFL")):
                candidates.add(path)
        if package.get("license_file"):
            candidates.add(directory / package["license_file"])
        texts = [path for path in sorted(candidates) if path.is_file() and
                 not path.is_symlink() and directory in path.resolve().parents]
        name = package["name"] + " " + package["version"]
        entry = overrides.get(package["name"] + "@" + package["version"])
        vcs = directory / ".cargo_vcs_info.json"
        commit = json.loads(vcs.read_text()).get("git", {}).get("sha1") if vcs.exists() else None
        if entry and entry["commit"] == commit:
            for item in entry["files"]:
                local = ROOT / item["path"]
                if ROOT.resolve() not in local.resolve().parents or local.is_symlink():
                    raise ValueError("Unsafe dependency notice path")
                notices.extend(["=" * 72, name + " / upstream notice", item["source"],
                                "=" * 72, local.read_text(), ""])
            if entry.get("review_required"):
                review.append(name)
        elif not texts:
            missing.append(name)
        for path in texts:
            notices.extend(["=" * 72, name + " / " + str(path.relative_to(directory)),
                            "=" * 72, path.read_text(errors="replace"), ""])
    if missing:
        lines.extend(["", "## License text review required", "",
                      "No bundled license text was found for these packages. Resolve before public binary distribution:", ""])
        lines.extend("- " + name for name in missing)
    if review:
        lines.extend(["", "## Upstream licensing clarification required", "",
                      "These upstream notices discuss licensing but are not complete license texts. Review before public distribution:", ""])
        lines.extend("- " + name for name in review)
    Path(destination).with_name("THIRD_PARTY_NOTICES.txt").write_text("\n".join(notices))
    Path(destination).write_text("\n".join(lines) + "\n")
    print("Dependency notices collected; {} missing texts, {} upstream clarifications require review.".format(len(missing), len(review)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("check")
    validate.add_argument("--tag")
    commands.add_parser("export")
    commands.add_parser("version")
    inventory = commands.add_parser("licenses")
    inventory.add_argument("metadata")
    inventory.add_argument("destination")
    args = parser.parse_args()
    try:
        if args.command == "version":
            print(version()[0])
        elif args.command == "check":
            current, build, files = check(tag=args.tag)
            print("Release checks passed: v{} (build {}), {} allowlisted sources".format(current, build, len(files)))
        elif args.command == "export":
            export_source()
        elif args.command == "licenses":
            dependency_inventory(args.metadata, args.destination)
    except (ValueError, OSError) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
