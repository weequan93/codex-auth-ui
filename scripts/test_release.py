import importlib.util
from pathlib import Path
import tempfile
import unittest
import shutil
import zipfile

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def fixture(self, directory):
        root = Path(directory)
        for source in release.sources():
            target = root / source.relative_to(release.ROOT)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
        return root

    def test_current_release_and_tag_match(self):
        current, _, _ = release.check()
        release.check(tag="v" + current)
        with self.assertRaises(ValueError):
            release.check(tag="v999.0.0")

    def test_dependency_notices_include_nested_font_licenses(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            crate = root / "sample-crate"
            (crate / "fonts").mkdir(parents=True)
            (crate / "Cargo.toml").write_text("")
            (crate / "fonts" / "OFL.txt").write_text("Synthetic font license notice")
            metadata = root / "metadata.json"
            metadata.write_text(json.dumps({"packages": [{
                "name": "sample-release-fixture", "version": "1.0.0", "license": "OFL-1.1",
                "manifest_path": str(crate / "Cargo.toml"),
            }]}))
            release.dependency_inventory(metadata, root / "DEPENDENCIES.md")
            self.assertIn("Synthetic font license notice", (root / "THIRD_PARTY_NOTICES.txt").read_text())
            self.assertNotIn("License text review required", (root / "DEPENDENCIES.md").read_text())

    def test_export_excludes_credentials_backups_and_unrelated_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture(directory)
            (root / "auth.json").write_text("not-a-real-secret")
            (root / "router-testnet-sample.json").write_text("unrelated")
            (root / "dist").mkdir()
            (root / "dist" / "private-backup").write_text("not-public")
            release.export_source(root)
            archive = next((root / "dist").glob("source-*/*.zip"))
            with zipfile.ZipFile(archive) as zipped:
                names = zipped.namelist()
                self.assertTrue(any(name.endswith("src/main.rs") for name in names))
                self.assertFalse(any("auth.json" in name or "router-testnet" in name or "/dist/" in name for name in names))

    def test_mismatched_version_and_symlink_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture(directory)
            lock = root / "Cargo.lock"
            lock.write_text(lock.read_text().replace('name = "codex-account-hub"', 'name = "wrong-name"'))
            with self.assertRaises(ValueError):
                release.check(root)
        if not hasattr(Path, "symlink_to"):
            return
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture(directory)
            target = root / "README.md"
            target.unlink()
            try:
                target.symlink_to(root / "LICENSE")
            except OSError:
                self.skipTest("Symlinks unavailable on this host")
            with self.assertRaises(ValueError):
                release.sources(root)


if __name__ == "__main__":
    unittest.main()
