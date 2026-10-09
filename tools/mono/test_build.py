"""Regressions for layered patches that overlap earlier source hunks."""
import difflib
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from build import apply_patch_stack, prepare, build_runtime


class PatchStackTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="wire-mono-patch-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.mono = self.root / "source"
        self.mono.mkdir()
        self.archive = self.root / "locked.tar.gz"
        self.original = b"int option = 1;\nint untouched = 7;\n"
        with tarfile.open(self.archive, "w:gz") as package:
            info = tarfile.TarInfo("mono-locked/runtime.c")
            info.size = len(self.original)
            package.addfile(info, io.BytesIO(self.original))
        (self.mono / "runtime.c").write_bytes(self.original)
        first = "int option = 2;\nint untouched = 7;\n"
        second = "int option = 3;\nint untouched = 7;\n"
        self.patches = []
        for index, (before, after) in enumerate(((self.original.decode(), first), (first, second))):
            patch = self.root / f"{index}.patch"
            patch.write_text("".join(difflib.unified_diff(before.splitlines(True), after.splitlines(True), fromfile="a/runtime.c", tofile="b/runtime.c")))
            self.patches.append(patch)
        self.expected = second.encode()

    def apply(self, patches):
        apply_patch_stack(self.mono, self.archive, "mono-locked", patches)

    def test_reprepare_and_upgrade_overlapping_hunks(self):
        self.apply(self.patches[:1])
        self.apply(self.patches)
        self.assertEqual((self.mono / "runtime.c").read_bytes(), self.expected)
        self.apply(self.patches)
        self.assertEqual((self.mono / "runtime.c").read_bytes(), self.expected)

    def test_unknown_edits_preserved_before_any_mutation(self):
        self.apply(self.patches[:1])
        unexpected = b"int option = 2;\nint untouched = 99;\n"
        (self.mono / "runtime.c").write_bytes(unexpected)
        with self.assertRaisesRegex(RuntimeError, "exact locked patch prefix"):
            self.apply(self.patches)
        self.assertEqual((self.mono / "runtime.c").read_bytes(), unexpected)

    def test_stale_patch_body_cannot_be_silently_accepted(self):
        self.apply(self.patches)
        self.patches[1].write_text(self.patches[1].read_text().replace("+int option = 3;", "+int option = 4;"))
        with self.assertRaisesRegex(RuntimeError, "exact locked patch prefix"):
            self.apply(self.patches)
        self.assertEqual((self.mono / "runtime.c").read_bytes(), self.expected)


class OutputIsolationTests(unittest.TestCase):
    def test_prepare_rejects_shared_source_symlink_before_download_or_source_mutation(self):
        with tempfile.TemporaryDirectory(prefix="wire-mono-output-test-") as temporary:
            root = Path(temporary) / "output"; root.mkdir()
            original = Path(temporary) / "original"; original.mkdir()
            sentinel = original / "runtime.c"; sentinel.write_text("original source\n")
            (root / "sources").symlink_to(original, target_is_directory=True)
            with self.assertRaisesRegex(RuntimeError, "physical clone"):
                prepare(root, {})
            self.assertEqual(sentinel.read_text(), "original source\n")
            self.assertFalse((root / "archives").exists())

    def test_runtime_rejects_shared_build_symlink_before_reading_config_or_building(self):
        with tempfile.TemporaryDirectory(prefix="wire-mono-output-test-") as temporary:
            root = Path(temporary) / "output"; root.mkdir()
            original = Path(temporary) / "build-original"; original.mkdir()
            sentinel = original / "Makefile"; sentinel.write_text("original build\n")
            (root / "build-runtime").symlink_to(original, target_is_directory=True)
            with self.assertRaisesRegex(RuntimeError, "physical clone"):
                build_runtime(root, {}, "runtime", 1)
            self.assertEqual(sentinel.read_text(), "original build\n")
            self.assertFalse((root / "runtime-configuration.json").exists())


if __name__ == "__main__":
    unittest.main()
