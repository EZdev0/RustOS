"""Tests for `tools/webprobe.py` - the Python side of the render checks.

Two layers:

* `check_dump()` is pure Python, so the validator itself is tested with
  synthetic display lists (fast, no toolchain needed).
* the end-to-end tests compile the engine with `rustc` and render the built-in
  fixtures; they are skipped when no compiler is available.
"""

from __future__ import annotations

import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

import webprobe  # noqa: E402

CLEAN = "\n".join(
    [
        "meta title Example",
        "meta status HTTP 200 - example.com",
        "meta width 640",
        "meta height 120",
        "meta truncated false",
        "cmd fill 0 0 640 120",
        "cmd text 10 20 80 11 1 Hello",
        "cmd text 90 20 40 11 1  you",
    ]
)


class CheckDumpTests(unittest.TestCase):
    """The validator must accept good pages and explain broken ones."""

    def problems(self, dump: str) -> list[str]:
        return webprobe.check_dump(dump).problems

    def test_clean_dump_has_no_problems(self):
        report = webprobe.check_dump(CLEAN)
        self.assertTrue(report.ok, report.problems)
        self.assertEqual(report.commands, 3)
        self.assertEqual((report.width, report.height), (640, 120))

    def test_overflow_is_reported(self):
        dump = CLEAN.replace("cmd text 90 20 40 11 1  you", "cmd text 600 20 160 11 1  wide")
        problems = self.problems(dump)
        self.assertTrue(any("overflows" in p for p in problems), problems)

    def test_empty_command_is_reported(self):
        problems = self.problems(CLEAN + "\ncmd fill 0 0 0 10")
        self.assertTrue(any("empty command" in p for p in problems), problems)

    def test_negative_coordinates_are_reported(self):
        problems = self.problems(CLEAN + "\ncmd text -5 20 8 11 1 x")
        self.assertTrue(any("negative x" in p for p in problems), problems)
        self.assertTrue(any("negative y" in p for p in problems) is False)

    def test_below_page_height_is_reported(self):
        problems = self.problems(CLEAN + "\ncmd fill 0 118 640 8")
        self.assertTrue(any("below the page height" in p for p in problems), problems)

    def test_truncated_page_is_a_failure(self):
        problems = self.problems(CLEAN.replace("meta truncated false", "meta truncated true"))
        self.assertTrue(any("truncated" in p for p in problems), problems)

    def test_text_moving_up_is_reported(self):
        dump = CLEAN + "\ncmd text 10 10 24 11 1 back"
        problems = self.problems(dump)
        self.assertTrue(any("text moves up" in p for p in problems), problems)

    def test_overlapping_runs_on_one_line_are_reported(self):
        dump = CLEAN + "\ncmd text 60 20 40 11 1 overlap"
        problems = self.problems(dump)
        self.assertTrue(any("overlapping text" in p for p in problems), problems)

    def test_missing_meta_is_reported(self):
        problems = self.problems("cmd fill 0 0 10 10")
        self.assertTrue(any("missing meta" in p for p in problems), problems)

    def test_short_and_unknown_records_are_reported(self):
        problems = self.problems(CLEAN + "\ncmd text 1 2\nbogus line")
        self.assertTrue(any("short command" in p for p in problems), problems)
        self.assertTrue(any("unknown record" in p for p in problems), problems)

    def test_chunked_fixture_declares_correct_frame_sizes(self):
        kind, payload = webprobe.FIXTURES["http"]
        self.assertEqual(kind, "http")
        _, _, header_and_body = payload.partition(b"\r\n\r\n")
        remaining = header_and_body
        frames = 0
        while True:
            size_line, _, rest = remaining.partition(b"\r\n")
            size = int(size_line, 16)
            if size == 0:
                break
            self.assertGreaterEqual(len(rest), size + 2, "truncated chunk body")
            self.assertEqual(rest[size : size + 2], b"\r\n", "chunk not terminated")
            remaining = rest[size + 2 :]
            frames += 1
        self.assertGreaterEqual(frames, 2)


def _rustc_available() -> bool:
    try:
        webprobe.find_rustc()
        return True
    except webprobe.ToolchainMissing:
        return False


@unittest.skipUnless(_rustc_available(), "rustc not available")
class RenderEndToEndTests(unittest.TestCase):
    """Compile the engine and check the real display lists."""

    @classmethod
    def setUpClass(cls):
        cls._tmp = tempfile.TemporaryDirectory(prefix="webprobe-test-")
        cls.binary = webprobe.build_dumper(pathlib.Path(cls._tmp.name))

    @classmethod
    def tearDownClass(cls):
        cls._tmp.cleanup()

    def test_all_fixtures_render_and_keep_invariants(self):
        for name, (kind, payload) in webprobe.FIXTURES.items():
            with self.subTest(fixture=name):
                dump = webprobe.render(self.binary, kind, payload)
                report = webprobe.check_dump(dump, name=name)
                self.assertTrue(report.ok, f"{name}: {report.problems[:5]}")

    def test_long_token_is_broken_instead_of_overflowing(self):
        dump = webprobe.render(
            self.binary,
            "html",
            b"<p>" + b"Z" * 500 + b"</p>",
            width=320,
        )
        report = webprobe.check_dump(dump, name="long-token")
        self.assertTrue(report.ok, report.problems[:5])
        self.assertGreater(report.commands, 1, "a 500 char token needs several runs")

    def test_empty_document_still_has_a_page_height(self):
        dump = webprobe.render(self.binary, "html", b"")
        report = webprobe.check_dump(dump, name="empty")
        self.assertTrue(report.ok, report.problems)
        self.assertGreater(report.height, 0)

    def test_hostile_markup_does_not_panic(self):
        payload = (
            b"<div><p><b><i>unclosed <table><tr><td><script>x</script>"
            b"<" * 200
            + b"&nosuch;&#xffffff;<style>*{}</style></div>"
        )
        dump = webprobe.render(self.binary, "html", payload)
        self.assertIn("meta width", dump)

    def test_dark_and_narrow_renders_stay_in_bounds(self):
        payload = b"<h1>Narrow</h1><p>" + b"word " * 200 + b"</p>"
        dump = webprobe.render(self.binary, "html", payload, width=80)
        report = webprobe.check_dump(dump, name="narrow")
        self.assertTrue(report.ok, report.problems[:5])


class SelfTestTests(unittest.TestCase):
    def test_self_test_passes(self):
        self.assertTrue(webprobe._self_test())


if __name__ == "__main__":
    unittest.main()
