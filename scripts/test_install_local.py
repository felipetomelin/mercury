from __future__ import annotations

import json
import os
import selectors
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


INSTALLER = Path(__file__).resolve().with_name("install-local.sh")


class LocalInstallTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="mercury install test ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.repo = self.root / "checkout with spaces"
        (self.repo / "scripts").mkdir(parents=True)
        self.script = self.repo / "scripts" / INSTALLER.name
        shutil.copyfile(INSTALLER, self.script)
        self.prefix = self.root / "local prefix"
        self.binary = self.prefix / "lib" / "mercury" / "mercury"
        self.command = self.prefix / "bin" / "mercury"
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.log = self.root / "cargo.json"
        self.env = dict(os.environ)
        self.env.pop("CARGO_BUILD_TARGET", None)
        self.env["PATH"] = f"{self.prefix / 'bin'}:{self.tools}:/usr/bin:/bin"
        self.env["TEST_CARGO_LOG"] = str(self.log)
        self.env["TEST_CHECKOUT"] = str(self.repo)
        self.env["TEST_BINARY"] = self.executable_text("0.9.3-test")
        cargo = self.tools / "cargo"
        cargo.write_text(
            f"#!{sys.executable}\n"
            "import json, os, pathlib, sys\n"
            "assert pathlib.Path.cwd() == pathlib.Path(os.environ['TEST_CHECKOUT'])\n"
            "pathlib.Path(os.environ['TEST_CARGO_LOG']).write_text(json.dumps(sys.argv[1:]))\n"
            "if os.environ.get('TEST_BUILD_FAIL'): sys.exit(42)\n"
            "profile = 'release' if '--release' in sys.argv else 'debug'\n"
            "host = sys.argv[sys.argv.index('--target') + 1]\n"
            "dest = pathlib.Path(os.environ['TEST_CHECKOUT']) / 'target' / host / profile / 'mercury'\n"
            "dest.parent.mkdir(parents=True, exist_ok=True)\n"
            "dest.write_text(os.environ['TEST_BINARY'])\n"
            "dest.chmod(0o755)\n"
        )
        cargo.chmod(0o755)
        rustc = self.tools / "rustc"
        rustc.write_text("#!/bin/sh\nprintf 'test-host\\n'\n")
        rustc.chmod(0o755)

    @staticmethod
    def executable_text(version: str) -> str:
        return (
            "#!/bin/sh\n"
            'if [ "${1:-}" = --hold ]; then\n'
            "  printf 'ready\\n'\n"
            "  read -r response\n"
            "fi\n"
            f"printf 'mercury {version}\\n'\n"
        )

    def artifact(self, profile: str = "debug", text: str | None = None) -> Path:
        dest = self.repo / "target" / profile / "mercury"
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(text or self.executable_text("0.9.3-test"))
        dest.chmod(0o755)
        return dest

    def run_installer(self, *args: str, ok: bool = True) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            ["/bin/bash", str(self.script), "--prefix", str(self.prefix), *args],
            cwd=self.root,
            env=self.env,
            text=True,
            capture_output=True,
            timeout=20,
        )
        if ok:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def install_dev(self) -> subprocess.CompletedProcess[str]:
        self.artifact()
        return self.run_installer("--profile", "dev", "--no-build")

    def test_default_build_installs_release_command_from_unrelated_directory(self) -> None:
        result = self.run_installer()
        self.assertIn("release profile; mercury data namespace", result.stdout)
        args = json.loads(self.log.read_text())
        self.assertIn("--release", args)
        self.assertIn("--locked", args)
        self.assertEqual(args[args.index("--target-dir") + 1], str(self.repo / "target"))
        self.assertEqual(args[args.index("--target") + 1], "test-host")
        self.assertEqual(self.command.resolve(), self.binary)
        result = subprocess.run(
            ["/bin/bash", "-c", "mercury --version"], cwd=self.root,
            env=self.env, capture_output=True, text=True, check=True, timeout=5,
        )
        self.assertEqual(result.stdout, "mercury 0.9.3-test\n")
        self.assertNotIn("export PATH", result.stdout)

    def test_dev_build_uses_debug_artifact_and_namespace(self) -> None:
        result = self.run_installer("--profile", "dev")
        self.assertIn("dev profile; mercury-dev data namespace", result.stdout)
        self.assertNotIn("--release", json.loads(self.log.read_text()))
        self.assertEqual(self.binary.read_bytes(), (self.repo / "target/test-host/debug/mercury").read_bytes())

    def test_no_build_does_not_invoke_cargo(self) -> None:
        self.install_dev()
        self.assertFalse(self.log.exists())

    def test_no_build_can_reuse_host_artifact_from_previous_install(self) -> None:
        self.run_installer("--profile", "dev")
        self.assertFalse((self.repo / "target/debug/mercury").exists())
        self.run_installer("--profile", "dev", "--no-build")
        self.assertEqual(self.command.resolve(), self.binary)

    def test_no_build_conventional_artifact_needs_no_rust_toolchain(self) -> None:
        self.artifact()
        (self.tools / "rustc").unlink()
        (self.tools / "cargo").unlink()
        self.run_installer("--profile", "dev", "--no-build")
        self.assertTrue(self.command.is_symlink())

    def test_installed_command_survives_checkout_removal(self) -> None:
        self.install_dev()
        shutil.rmtree(self.repo)
        result = subprocess.run(
            [str(self.command), "--version"], capture_output=True, text=True,
            check=True, timeout=5,
        )
        self.assertEqual(result.stdout, "mercury 0.9.3-test\n")

    def test_reinstall_atomically_replaces_inode(self) -> None:
        self.install_dev()
        before = self.binary.stat().st_ino
        self.artifact(text=self.executable_text("new"))
        self.run_installer("--profile", "dev", "--no-build")
        self.assertNotEqual(self.binary.stat().st_ino, before)
        self.assertIn("mercury new", self.binary.read_text())
        self.assertEqual(list(self.binary.parent.glob(".install.*")), [])

    def test_running_process_survives_reinstall(self) -> None:
        self.install_dev()
        process = subprocess.Popen(
            [str(self.command), "--hold"], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.addCleanup(lambda: process.kill() if process.poll() is None else None)
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            self.assertTrue(selector.select(timeout=5), "fixture never became ready")
        self.assertEqual(process.stdout.readline(), "ready\n")
        self.artifact(text=self.executable_text("new"))
        self.run_installer("--profile", "dev", "--no-build")
        self.assertIsNone(process.poll())
        output, error = process.communicate("finish\n", timeout=5)
        self.assertEqual(process.returncode, 0, error)
        self.assertEqual(output, "mercury 0.9.3-test\n")

    def test_build_failure_preserves_installed_binary(self) -> None:
        self.install_dev()
        before = self.binary.read_bytes()
        self.env["TEST_BUILD_FAIL"] = "1"
        self.run_installer("--profile", "dev", ok=False)
        self.assertEqual(self.binary.read_bytes(), before)
        self.assertEqual(self.command.resolve(), self.binary)

    def test_missing_artifact_preserves_installed_binary(self) -> None:
        self.install_dev()
        before = self.binary.read_bytes()
        (self.repo / "target/debug/mercury").unlink()
        result = self.run_installer("--profile", "dev", "--no-build", ok=False)
        self.assertIn("No executable", result.stderr)
        self.assertEqual(self.binary.read_bytes(), before)

    def test_invalid_or_failing_binary_does_not_replace_previous_install(self) -> None:
        self.install_dev()
        before = self.binary.read_bytes()
        for text in ("#!/bin/sh\nexit 9\n", "#!/bin/sh\necho wrong-product\n"):
            with self.subTest(text=text):
                self.artifact(text=text)
                self.run_installer("--profile", "dev", "--no-build", ok=False)
                self.assertEqual(self.binary.read_bytes(), before)
                self.assertEqual(list(self.binary.parent.glob(".install.*")), [])

    def test_failed_initial_validation_does_not_expose_a_command(self) -> None:
        self.artifact(text="#!/bin/sh\nexit 9\n")
        self.run_installer("--profile", "dev", "--no-build", ok=False)
        self.assertFalse(self.command.is_symlink())
        self.assertFalse(self.binary.exists())
        self.assertEqual(list(self.binary.parent.glob(".install.*")), [])

    def test_rename_failure_preserves_previous_install(self) -> None:
        self.install_dev()
        before = self.binary.read_bytes()
        self.artifact(text=self.executable_text("new"))
        failing_mv = self.tools / "mv"
        failing_mv.write_text("#!/bin/sh\nexit 7\n")
        failing_mv.chmod(0o755)
        self.run_installer("--profile", "dev", "--no-build", ok=False)
        self.assertEqual(self.binary.read_bytes(), before)
        self.assertEqual(self.command.resolve(), self.binary)
        self.assertEqual(list(self.binary.parent.glob(".install.*")), [])

    def test_failed_initial_rename_removes_new_command_link(self) -> None:
        self.artifact()
        failing_mv = self.tools / "mv"
        failing_mv.write_text("#!/bin/sh\nexit 7\n")
        failing_mv.chmod(0o755)
        self.run_installer("--profile", "dev", "--no-build", ok=False)
        self.assertFalse(self.command.is_symlink())
        self.assertFalse(self.binary.exists())

    def test_conflicting_file_is_preserved_before_build(self) -> None:
        self.command.parent.mkdir(parents=True)
        self.command.write_text("another program")
        self.run_installer(ok=False)
        self.assertEqual(self.command.read_text(), "another program")
        self.assertFalse(self.log.exists())

    def test_conflicting_directory_is_preserved(self) -> None:
        self.command.mkdir(parents=True)
        self.run_installer(ok=False)
        self.assertTrue(self.command.is_dir())

    def test_foreign_and_dangling_symlinks_are_preserved(self) -> None:
        self.command.parent.mkdir(parents=True)
        for name in ("foreign", "missing"):
            target = self.root / name
            if name == "foreign":
                target.write_text("keep")
            with self.subTest(name=name):
                self.command.symlink_to(target)
                self.run_installer(ok=False)
                self.assertEqual(os.readlink(self.command), str(target))
                self.command.unlink()

    def test_internal_symlink_directories_are_refused(self) -> None:
        self.artifact()
        for relative in ("bin", "lib", "lib/mercury"):
            with self.subTest(relative=relative):
                redirect = self.prefix / relative
                redirect.parent.mkdir(parents=True, exist_ok=True)
                outside = self.root / (relative.replace("/", "-") + "-outside")
                outside.mkdir()
                redirect.symlink_to(outside, target_is_directory=True)
                self.run_installer("--profile", "dev", "--no-build", ok=False)
                self.assertEqual(list(outside.iterdir()), [])
                redirect.unlink()

    def test_internal_binary_symlink_is_not_followed(self) -> None:
        self.install_dev()
        self.binary.unlink()
        outside = self.root / "keep"
        outside.write_text("keep")
        self.binary.symlink_to(outside)
        self.run_installer("--profile", "dev", "--no-build", ok=False)
        self.run_installer("--uninstall", ok=False)
        self.assertEqual(outside.read_text(), "keep")

    def test_unmanaged_internal_binary_is_preserved(self) -> None:
        self.binary.parent.mkdir(parents=True)
        self.binary.write_text("not owned")
        self.run_installer(ok=False)
        self.run_installer("--uninstall", ok=False)
        self.assertEqual(self.binary.read_text(), "not owned")

    def test_uninstall_is_idempotent_and_preserves_config_state_and_other_files(self) -> None:
        self.install_dev()
        preserved = [
            self.root / ".config/mercury-dev/config.toml",
            self.root / ".local/state/mercury-dev/history.db",
            self.root / "vault/note.md",
            self.binary.parent / "unrelated.txt",
        ]
        for path in preserved:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("keep me")
        self.run_installer("--uninstall")
        self.run_installer("--uninstall")
        self.assertFalse(self.command.is_symlink())
        self.assertFalse(self.binary.exists())
        for path in preserved:
            self.assertEqual(path.read_text(), "keep me")

    def test_uninstall_refuses_foreign_command(self) -> None:
        self.command.parent.mkdir(parents=True)
        self.command.write_text("keep")
        self.run_installer("--uninstall", ok=False)
        self.assertEqual(self.command.read_text(), "keep")

    def test_uninstall_missing_prefix_does_not_create_it(self) -> None:
        self.run_installer("--uninstall")
        self.assertFalse(self.prefix.exists())

    def test_dangling_managed_link_can_be_removed(self) -> None:
        self.command.parent.mkdir(parents=True)
        self.command.symlink_to(self.binary)
        self.run_installer("--uninstall")
        self.assertFalse(self.command.is_symlink())

    def test_missing_path_prints_shell_escaped_instruction(self) -> None:
        self.env["PATH"] = "/usr/bin:/bin"
        result = self.install_dev()
        self.assertIn("export PATH=", result.stdout)
        line = next(line.strip() for line in result.stdout.splitlines() if "export PATH=" in line)
        result = subprocess.run(
            ["/bin/bash", "-c", line + "; mercury --version"], env=self.env,
            capture_output=True, text=True, check=True, timeout=5,
        )
        self.assertEqual(result.stdout, "mercury 0.9.3-test\n")

    def test_shadowing_command_is_reported(self) -> None:
        other = self.tools / "mercury"
        other.write_text(self.executable_text("foreign"))
        other.chmod(0o755)
        self.env["PATH"] = f"{self.tools}:{self.prefix / 'bin'}:/usr/bin:/bin"
        result = self.install_dev()
        self.assertIn(f"Another mercury currently has precedence: {other}", result.stdout)
        self.assertIn("export PATH=", result.stdout)

    def test_cross_target_is_refused(self) -> None:
        self.env["CARGO_BUILD_TARGET"] = "x86_64-pc-windows-msvc"
        result = self.run_installer(ok=False)
        self.assertIn("CARGO_BUILD_TARGET", result.stderr)
        self.assertFalse(self.log.exists())

    def test_argument_errors_do_not_install(self) -> None:
        for args in (
            ("--prefix", "relative"), ("--prefix", "/"), ("--prefix", "/tmp/colon:path"),
            ("--profile", "wrong"), ("--profile",), ("--prefix",), ("--unknown",),
        ):
            with self.subTest(args=args):
                self.run_installer(*args, ok=False)
                self.assertFalse(self.command.exists())

    def test_help_does_not_build_or_create_prefix(self) -> None:
        result = self.run_installer("--help")
        self.assertIn("--uninstall", result.stdout)
        self.assertFalse(self.prefix.exists())
        self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
