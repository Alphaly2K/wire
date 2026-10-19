"""Probe build inputs must resolve from the repository, including external cwd."""
import contextlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import probe_graphics_present
import probe_moltenvk


class BuildIntercepted(Exception):
    pass


@contextlib.contextmanager
def external_directory(directory):
    original = Path.cwd()
    os.chdir(directory)
    try:
        yield
    finally:
        os.chdir(original)


class GraphicsProbeTests(unittest.TestCase):
    def test_present_build_resolves_real_ui_source_from_external_cwd(self):
        source = Path(__file__).resolve().parents[1] / 'src/platform/macos.m'
        with tempfile.TemporaryDirectory() as directory, external_directory(directory):
            output = Path(directory).resolve() / 'present'
            argv = ['probe_graphics_present.py', '--backend', 'unused',
                    '--moltenvk', 'unused', '--out', str(output)]
            with patch('sys.argv', argv), patch.object(probe_graphics_present.subprocess, 'run',
                    side_effect=BuildIntercepted) as build, self.assertRaises(BuildIntercepted):
                probe_graphics_present.main()
            command = build.call_args.args[0]
            self.assertEqual(command[:2], ['xcrun', 'clang'])
            self.assertIn(str(source), command)
            self.assertTrue(source.is_file())
            self.assertEqual(command[-2:], ['-o', str(output / 'libwire_present_probe.dylib')])

    def test_moltenvk_build_resolves_real_c_source_from_external_cwd(self):
        source = Path(__file__).resolve().with_name('moltenvk_probe.c')
        with tempfile.TemporaryDirectory() as directory, external_directory(directory):
            include = Path(directory) / 'include'
            (include / 'vulkan').mkdir(parents=True)
            (include / 'vulkan/vulkan.h').touch()
            library = Path(directory) / 'libMoltenVK.dylib'
            library.touch()
            output = Path(directory).resolve() / 'probe'
            argv = ['probe_moltenvk.py', '--include', str(include),
                    '--library', str(library), '--out', str(output)]
            with patch('sys.argv', argv), patch.object(probe_moltenvk.sys, 'platform', 'darwin'), \
                    patch.object(probe_moltenvk.subprocess, 'run', side_effect=BuildIntercepted) as build, \
                    self.assertRaises(BuildIntercepted):
                probe_moltenvk.main()
            command = build.call_args.args[0]
            self.assertEqual(command[:2], ['xcrun', 'clang'])
            self.assertIn(str(source), command)
            self.assertTrue(source.is_file())
            self.assertEqual(command[-2:], ['-o', str(output / 'moltenvk-probe')])
