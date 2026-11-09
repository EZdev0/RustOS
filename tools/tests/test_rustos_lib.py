"""Unit tests for the RustOS build tool helpers.

Run with:  python3 -m unittest discover -s tools/tests -v
"""

from __future__ import annotations

import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import rustos_lib as lib  # noqa: E402


class HumanBytesTests(unittest.TestCase):
    def test_units(self):
        self.assertEqual(lib.human_bytes(0), "0 B")
        self.assertEqual(lib.human_bytes(999), "999 B")
        self.assertEqual(lib.human_bytes(1024), "1.0 KiB")
        self.assertEqual(lib.human_bytes(1536), "1.5 KiB")
        self.assertEqual(lib.human_bytes(5 * 1024 * 1024), "5.0 MiB")
        self.assertEqual(lib.human_bytes(-1), "-1 B")


class ManifestParsingTests(unittest.TestCase):
    MANIFEST = """
    [workspace]
    resolver = "2"
    members = [
        "builder",
        "kernel",
        "rustos-web",
    ]

    [profile.release]
    opt-level = 3
    lto = "fat"
    codegen-units = 1
    panic = "abort"
    strip = "debuginfo"

    [profile.dev]
    opt-level = 3
    incremental = true
    """

    def test_workspace_members(self):
        self.assertEqual(
            lib.parse_workspace_members(self.MANIFEST),
            ["builder", "kernel", "rustos-web"],
        )

    def test_workspace_members_empty(self):
        self.assertEqual(lib.parse_workspace_members("[package]\nname = 'x'"), [])

    def test_release_profile(self):
        profile = lib.parse_release_profile(self.MANIFEST)
        self.assertEqual(profile["codegen-units"], "1")
        self.assertEqual(profile["panic"], "abort")
        # Keys from the following profile must not leak into this one.
        self.assertNotIn("incremental", profile)

    def test_release_profile_missing(self):
        self.assertEqual(lib.parse_release_profile("[package]\nname='x'"), {})


class LogScanTests(unittest.TestCase):
    def test_detects_panics(self):
        log = "booting\n[INFO] heap ready\npanicked at src/main.rs:12\n"
        hits = lib.scan_log_for_panics(log)
        self.assertEqual(len(hits), 1)
        self.assertIn("panicked", hits[0])

    def test_ignores_clean_boots(self):
        log = "loading kernel\n[boot] framebuffer 800x600\n[net] link up\n"
        self.assertEqual(lib.scan_log_for_panics(log), [])

    def test_whitelisted_lines(self):
        log = "profile says panic = abort\n"
        self.assertEqual(lib.scan_log_for_panics(log), [])

    def test_boot_marker(self):
        self.assertTrue(lib.boot_marker_present("[boot] RustOS 0.2.0 entering kernel_main\n"))
        self.assertFalse(lib.boot_marker_present("GRUB loading\n"))
        self.assertFalse(lib.boot_marker_present(""))

    def test_triple_fault_and_page_fault(self):
        log = "EXCEPTION: TRIPLE FAULT\npage fault at 0x0\n"
        self.assertEqual(len(lib.scan_log_for_panics(log)), 2)


class ImageTests(unittest.TestCase):
    def _write_image(self, tmp: Path, size: int, signature: bool) -> Path:
        data = bytearray(size)
        if signature and size >= 512:
            data[510] = 0x55
            data[511] = 0xAA
        path = tmp / "disk.img"
        path.write_bytes(bytes(data))
        return path

    def test_mbr_magic(self):
        self.assertTrue(lib.image_magic(bytes(510) + b"\x55\xaa"))
        self.assertFalse(lib.image_magic(bytes(512)))
        self.assertFalse(lib.image_magic(b"\x55"))

    def test_inspect_missing_image(self):
        report = lib.inspect_image(Path("/nonexistent/rustos.img"))
        self.assertFalse(report["exists"])
        self.assertFalse(report["bootable"])

    def test_inspect_bootable_image(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_image(Path(tmp), 4096, signature=True)
            report = lib.inspect_image(path)
            self.assertTrue(report["exists"])
            self.assertTrue(report["bootable"])
            self.assertEqual(report["size"], 4096)
            self.assertIn("KiB", report["human_size"])

    def test_inspect_non_bootable_image(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_image(Path(tmp), 4096, signature=False)
            self.assertFalse(lib.inspect_image(path)["bootable"])


class MarkdownTests(unittest.TestCase):
    def test_extracts_relative_links_only(self):
        text = (
            "[a](docs/a.md) [b](https://example.com) [c](#anchor) "
            "[d](docs/b.md#section) [e](mailto:x@y.z)"
        )
        self.assertEqual(lib.markdown_links(text), ["docs/a.md", "docs/b.md"])

    def test_broken_and_ok_links(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "docs" / "exists.md").write_text("# hi\n", encoding="utf-8")
            (root / "README.md").write_text(
                "[ok](docs/exists.md) [bad](docs/missing.md)\n", encoding="utf-8"
            )
            problems = lib.check_markdown_links(root, lib.iter_docs(root))
            self.assertEqual(len(problems), 1)
            self.assertIn("missing.md", problems[0])

    def test_iter_docs_includes_readme_and_docs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "README.md").write_text("x", encoding="utf-8")
            (root / "docs" / "a.md").write_text("x", encoding="utf-8")
            names = sorted(path.name for path in lib.iter_docs(root))
            self.assertEqual(names, ["README.md", "a.md"])


class ConsistencyTests(unittest.TestCase):
    def test_detects_codegen_units_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Cargo.toml").write_text(
                "[profile.release]\ncodegen-units = 1\n", encoding="utf-8"
            )
            (root / "README.md").write_text(
                "We set `codegen-units = 8` for release builds.", encoding="utf-8"
            )
            problems = lib.profile_consistency(root)
            self.assertTrue(any("codegen-units" in problem for problem in problems))

    def test_matching_claims_are_clean(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Cargo.toml").write_text(
                "[profile.release]\ncodegen-units = 1\n", encoding="utf-8"
            )
            (root / "README.md").write_text("codegen-units = 1", encoding="utf-8")
            self.assertEqual(lib.profile_consistency(root), [])

    def test_missing_cargo_config_claim(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Cargo.toml").write_text("[profile.release]\n", encoding="utf-8")
            (root / "README.md").write_text(
                "we use `.cargo/config.toml` with jobs", encoding="utf-8"
            )
            problems = lib.profile_consistency(root)
            self.assertTrue(any(".cargo/config.toml" in problem for problem in problems))


class AnnotationTests(unittest.TestCase):
    def test_percent_is_escaped(self):
        self.assertEqual(lib.gha_escape("100% ok"), "100%25 ok")
        self.assertEqual(lib.gha_escape("a\nb"), "a%0Ab")

    def test_ansi_is_stripped(self):
        self.assertEqual(lib.strip_ansi("\x1b[1mbold\x1b[0m"), "bold")
        self.assertNotIn("\x1b", lib.gha_error("\x1b[31mred\x1b[0m panic"))

    def test_annotation_prefix_is_not_doubled(self):
        self.assertEqual(lib.gha_error("::error::already"), "::error::already")
        self.assertTrue(lib.gha_error("boom").startswith("::error::"))

    def test_annotate_lines_keeps_the_tail(self):
        text = "\n".join(f"line{i}" for i in range(20))
        lines = lib.annotate_lines(text, 3)
        self.assertEqual(len(lines), 3)
        self.assertIn("line19", lines[-1])

    def test_annotate_lines_handles_empty_output(self):
        lines = lib.annotate_lines("   \n\n", 5)
        self.assertEqual(len(lines), 1)
        self.assertIn("no output", lines[0])

    def test_clippy_annotations_pair_location(self):
        log = (
            "   Checking kernel v0.2.0\n"
            "error: unused variable: `width`\n"
            "  --> kernel/src/desktop/browser.rs:120:9\n"
            "warning: this loop never loops\n"
            "  --> kernel/src/x.rs:9:5\n"
        )
        lines = lib.clippy_annotations(log)
        self.assertEqual(len(lines), 2)
        self.assertIn("browser.rs:120:9", lines[0])
        self.assertIn("unused variable", lines[0])

    def test_clippy_annotations_fall_back_to_tail(self):
        log = "error: could not compile `kernel`\nno location\n"
        lines = lib.clippy_annotations(log)
        self.assertTrue(lines)
        self.assertTrue(all(line.startswith("::error::") for line in lines))

    def test_clippy_annotations_respect_limit(self):
        log = "\n".join(f"error: e{i}\n  --> f.rs:{i}:1" for i in range(30))
        self.assertEqual(len(lib.clippy_annotations(log, 4)), 4)


class DocIndexTests(unittest.TestCase):
    def test_unlinked_doc_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "README.md").write_text("see [index](docs/README.md)", encoding="utf-8")
            (root / "docs" / "README.md").write_text("[a](a.md)", encoding="utf-8")
            (root / "docs" / "a.md").write_text("x", encoding="utf-8")
            (root / "docs" / "orphan.md").write_text("x", encoding="utf-8")
            problems = lib.check_doc_index(root)
            self.assertEqual(len(problems), 1)
            self.assertIn("orphan.md", problems[0])

    def test_all_linked_is_clean(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs").mkdir()
            (root / "docs" / "README.md").write_text("[a](a.md) [b](b.md)", encoding="utf-8")
            (root / "docs" / "a.md").write_text("x", encoding="utf-8")
            (root / "docs" / "b.md").write_text("x", encoding="utf-8")
            self.assertEqual(lib.check_doc_index(root), [])


class CommandPlanTests(unittest.TestCase):
    def test_every_step_is_documented_and_non_empty(self):
        for step in ("fmt", "clippy-host", "clippy-kernel", "test", "build"):
            plan = lib.cargo_command(step)
            self.assertTrue(plan, f"{step} has no commands")
            for argv in plan:
                self.assertEqual(argv[0], "cargo")

    def test_unknown_step_is_empty(self):
        self.assertEqual(lib.cargo_command("nope"), [])

    def test_kernel_lint_uses_build_std(self):
        argv = lib.cargo_command("clippy-kernel")[0]
        self.assertIn("-Z", argv)
        self.assertIn(lib.KERNEL_TARGET, argv)


class RunCommandTests(unittest.TestCase):
    def test_missing_binary_returns_127(self):
        code, output = lib.run_command(["definitely-not-a-binary-xyz"], Path("."))
        self.assertEqual(code, 127)
        self.assertIn("command not found", output)

    def test_timeout_is_reported(self):
        def fake_runner(*_args, **_kwargs):
            raise subprocess.TimeoutExpired(cmd="slow", timeout=1)

        code, output = lib.run_command(["slow"], Path("."), runner=fake_runner, timeout=1)
        self.assertEqual(code, 124)
        self.assertIn("timeout", output)

    def test_output_is_combined(self):
        def fake_runner(*_args, **_kwargs):
            return subprocess.CompletedProcess(args=["x"], returncode=0, stdout="out\n", stderr="err\n")

        code, output = lib.run_command(["x"], Path("."), runner=fake_runner)
        self.assertEqual(code, 0)
        self.assertIn("out", output)
        self.assertIn("err", output)


class ReportTests(unittest.TestCase):
    def test_empty_report_is_ok(self):
        report = lib.Report()
        self.assertTrue(report.ok)
        self.assertEqual(lib.summarize_failures(report), "all checks passed")
        self.assertIn('"ok": true', report.to_json())

    def test_failures_are_summarised(self):
        report = lib.Report()
        report.add(lib.CheckResult("fmt", True))
        report.add(lib.CheckResult("clippy", False, "warning: unused"))
        self.assertFalse(report.ok)
        self.assertEqual(len(report.failed), 1)
        text = lib.summarize_failures(report)
        self.assertIn("clippy", text)
        self.assertIn("PASS", report.to_markdown())
        self.assertIn("FAIL", report.to_markdown())

    def test_markdown_escapes_pipes(self):
        report = lib.Report()
        report.add(lib.CheckResult("x", False, "a | b"))
        self.assertIn("a \\| b", report.to_markdown())


class TailTests(unittest.TestCase):
    def test_short_input(self):
        self.assertEqual(lib.tail("a\nb", 5), "a\nb")

    def test_truncates_to_last_lines(self):
        text = "\n".join(f"line{i}" for i in range(10))
        result = lib.tail(text, 3)
        self.assertEqual(result.splitlines(), ["line7", "line8", "line9"])


class CliSmokeTests(unittest.TestCase):
    """The CLI must be importable and its parser must expose every command."""

    def test_parser_commands(self):
        import rustos

        parser = rustos.build_parser()
        actions = [a for a in parser._actions if a.dest == "command"]
        self.assertTrue(actions, "no subcommand action")
        choices = set(actions[0].choices)
        for command in ("doctor", "fmt", "clippy", "test", "docs", "build", "qemu",
                        "verify", "report", "annotate"):
            self.assertIn(command, choices)

    def test_doctor_runs(self):
        import rustos

        self.assertIn(rustos.main(["doctor"]), (0, 1))


if __name__ == "__main__":
    unittest.main()
