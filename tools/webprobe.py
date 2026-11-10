#!/usr/bin/env python3
"""Render pages with the RustOS browser engine and check the result from Python.

Why this exists
---------------
`rustos-web` is a `no_std`+`alloc` crate with no dependencies, so it can be
built with a bare `rustc` call - no Cargo, no registry, no network. This script
uses that to run *end-to-end* render checks from Python:

    rustc lib.rs -> librustos_web.rlib
    rustc examples/render_dump.rs -> render_dump
    echo "<html>...</html>" | render_dump html - 640

The display list printed by `render_dump` is then validated here. These are the
invariants the compositor relies on; a violation is a visible bug (text painted
outside the window, invisible rows, absurd page height):

* every command lies inside the page (`x >= 0`, `y >= 0`, `x + w <= width`),
* no command is empty (`w > 0`, `h > 0`) - an empty command is wasted work,
* commands are emitted top-to-bottom (`y` never decreases),
* the reported page height covers every command,
* hostile input never panics (the process must exit 0) and never produces a
  command set that breaks the rules above.

Usage
-----
    python3 tools/webprobe.py --check            # render all fixtures, exit 1 on problems
    python3 tools/webprobe.py --check --verbose  # also print the command counts
    python3 tools/webprobe.py --dump fixtures/web/simple.html
    python3 tools/webprobe.py --self-test        # check the checker itself (no rustc)

The module is import-safe and dependency free so `tools/tests/` can unit test
`check_dump()` without a toolchain.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ENGINE = ROOT / "rustos-web"
DEFAULT_WIDTH = 640

# ── fixtures ────────────────────────────────────────────────────────────────
# Small, hand-written pages that cover the paths the kernel actually walks.

def _chunked_response() -> bytes:
    """A chunked HTTP/1.1 response with correct frame sizes."""
    parts = [b"<html><body><h1>Chunked</h1>", b"<p>body arrives in ", b"pieces</p></body></html>"]
    head = (
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n"
        b"Transfer-Encoding: chunked\r\n\r\n"
    )
    frames = b"".join(b"%x\r\n%s\r\n" % (len(part), part) for part in parts)
    return head + frames + b"0\r\n\r\n"


FIXTURES: dict[str, tuple[str, bytes]] = {
    "simple": (
        "html",
        b"""<!doctype html><html><head><title>Simple</title></head>
<body><h1>Hello RustOS</h1><p>Plain paragraph with <b>bold</b>,
<i>italic</i> and <a href="http://example.com/">a link</a>.</p>
<ul><li>one</li><li>two</li></ul><hr><p>after the rule</p></body></html>""",
    ),
    "wide": (
        "html",
        b"""<html><body><h2>Long tokens</h2>
<p>""" + b"x" * 400 + b"""</p>
<p>prefix """ + b"y" * 120 + b""" suffix</p>
<pre>""" + b"0123456789" * 40 + b"""</pre></body></html>""",
    ),
    "nested": (
        "html",
        b"""<html><body><div class="a"><div><div><div><div>
<h3>Deep</h3><p>Nested blocks must not offset the flow.</p>
<table><tr><th>k</th><th>v</th></tr><tr><td>a</td><td>b</td></tr></table>
</div></div></div></div></div></body></html>""",
    ),
    "hostile": (
        "html",
        b"""<html><body><script>alert("x")</script><style>body{}</style>
<p>unclosed <b>bold <i>and italic
<div style="font-size: 400px">huge</div><span>&amp;&#65;&nosuch;</span>
<p><<<>>><a href="javascript:void(0)">js</a></p>
<div><div><div><div><div><div><div><div><div><div>
<p>&lt;not a tag&gt;</p></div></div></div></div></div></div></div></div></div></div>
</body></html>""",
    ),
    "empty": ("html", b""),
    "http": ("http", _chunked_response()),
    "http-until-close": (
        "http",
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n"
        b"<html><body><p>framed by connection close</p></body></html>",
    ),
}


@dataclass
class Report:
    """Result of validating one display list."""

    name: str = ""
    width: int = 0
    height: int = 0
    commands: int = 0
    problems: list[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.problems


def check_dump(text: str, name: str = "dump") -> Report:
    """Validate the textual display list produced by `render_dump`.

    Pure function - no toolchain required, which is what `tools/tests/` uses.
    """
    report = Report(name=name)
    width = None
    height = None
    commands: list[tuple[int, int, int, int, str, str]] = []

    for raw in text.splitlines():
        line = raw.strip()
        if not line:
            continue
        parts = line.split(" ")
        tag = parts[0]
        if tag == "meta":
            if len(parts) < 3:
                report.problems.append(f"malformed meta line: {raw!r}")
                continue
            key, value = parts[1], " ".join(parts[2:])
            if key == "width":
                width = _int(value, report, "meta width")
            elif key == "height":
                height = _int(value, report, "meta height")
            elif key == "truncated" and value == "true":
                report.problems.append("page truncated (layout cap hit)")
            continue
        if tag != "cmd":
            report.problems.append(f"unknown record: {raw!r}")
            continue
        if len(parts) < 6:
            report.problems.append(f"short command: {raw!r}")
            continue
        kind = parts[1]
        x = _int(parts[2], report, reason=f"{kind} x")
        y = _int(parts[3], report, reason=f"{kind} y")
        w = _int(parts[4], report, reason=f"{kind} w")
        h = _int(parts[5], report, reason=f"{kind} h")
        if None in (x, y, w, h):
            continue
        detail = "fill" if kind == "fill" else f"text {parts[7] if len(parts) > 7 else ''}"
        commands.append((x, y, w, h, kind, detail))

    report.commands = len(commands)
    if width is None or height is None:
        report.problems.append("missing meta width/height")
        return report
    report.width = width
    report.height = height

    for x, y, w, h, _kind, detail in commands:
        label = f"{detail} at ({x},{y}) size {w}x{h}"
        if w <= 0 or h <= 0:
            report.problems.append(f"empty command: {label}")
        if x < 0:
            report.problems.append(f"negative x: {label}")
        if y < 0:
            report.problems.append(f"negative y: {label}")
        if x >= 0 and w > 0 and x + w > width:
            report.problems.append(
                f"overflows the page by {x + w - width}px: {label}"
            )
        if y >= 0 and y + h > height:
            report.problems.append(
                f"below the page height ({height}): {label}"
            )

    # Text flows top-down: a text run that jumps back up means the flow cursor
    # was moved by a decoration, which would paint over an earlier line.
    last_text_y = None
    for _x, y, _w, _h, kind, detail in commands:
        if kind != "text":
            continue
        if last_text_y is not None and y < last_text_y:
            report.problems.append(
                f"text moves up ({last_text_y} -> {y}): {detail} at y={y}"
            )
        last_text_y = y if last_text_y is None else max(last_text_y, y)

    # Two text runs on the same baseline must not overlap; that is what a
    # double-emitted segment or a broken inline offset looks like on screen.
    by_row: dict[int, list[tuple[int, int, str]]] = {}
    for x, y, w, _h, kind, detail in commands:
        if kind == "text":
            by_row.setdefault(y, []).append((x, w, detail))
    for y, runs in by_row.items():
        runs.sort()
        for (x0, w0, d0), (x1, _w1, d1) in zip(runs, runs[1:]):
            if x1 < x0 + w0 - 1:
                report.problems.append(
                    f"overlapping text on line y={y}: {d0!r} and {d1!r}"
                )

    return report


def _int(value: str, report: Report, reason: str = "value") -> int | None:
    try:
        return int(value)
    except (TypeError, ValueError):
        report.problems.append(f"not an integer ({reason}): {value!r}")
        return None


# ── toolchain plumbing ──────────────────────────────────────────────────────


class ToolchainMissing(RuntimeError):
    """Raised when `rustc` is not available."""


def find_rustc() -> str:
    """Locate `rustc`, preferring an explicit RUSTC override."""
    override = os.environ.get("RUSTC")
    if override and Path(override).exists():
        return override
    found = shutil.which("rustc")
    if found:
        return found
    for candidate in (
        Path.home() / ".rusttc/prefix/bin/rustc",
        Path.home() / ".cargo/bin/rustc",
    ):
        if candidate.exists():
            return str(candidate)
    raise ToolchainMissing("rustc not found (set RUSTC=/path/to/rustc)")


def build_dumper(out_dir: Path) -> Path:
    """Compile the engine + `render_dump` with plain `rustc`."""
    rustc = find_rustc()
    out_dir.mkdir(parents=True, exist_ok=True)
    rlib = out_dir / "librustos_web.rlib"
    _run(
        [
            rustc,
            "--edition",
            "2021",
            "--crate-type",
            "lib",
            "--crate-name",
            "rustos_web",
            str(ENGINE / "src" / "lib.rs"),
            "-o",
            str(rlib),
        ]
    )
    binary = out_dir / "render_dump"
    _run(
        [
            rustc,
            "--edition",
            "2021",
            str(ENGINE / "examples" / "render_dump.rs"),
            "--extern",
            f"rustos_web={rlib}",
            "-o",
            str(binary),
        ]
    )
    return binary


def _run(cmd: list[str]) -> None:
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(
            f"{cmd[0]} failed ({result.returncode}):\n{result.stderr.strip()[:4000]}"
        )


def render(binary: Path, kind: str, payload: bytes, width: int = DEFAULT_WIDTH) -> str:
    """Run the dumper and return its stdout."""
    result = subprocess.run(
        [str(binary), kind, "-", str(width)],
        input=payload,
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"render_dump exited {result.returncode} on {kind} input: "
            f"{result.stderr.decode('utf-8', 'replace')[:2000]}"
        )
    return result.stdout.decode("utf-8", "replace")


def check_fixtures(binary: Path, width: int = DEFAULT_WIDTH, verbose: bool = False) -> list[Report]:
    """Render and validate every built-in fixture."""
    reports: list[Report] = []
    for name, (kind, payload) in FIXTURES.items():
        try:
            dump = render(binary, kind, payload, width)
        except RuntimeError as error:  # a panic is a failure of the engine
            report = Report(name=name, width=width)
            report.problems.append(str(error))
            reports.append(report)
            continue
        report = check_dump(dump, name=name)
        reports.append(report)
        if verbose:
            print(
                f"{name:<18} {kind:<5} cmds={report.commands:<5} "
                f"height={report.height:<6} {'ok' if report.ok else 'FAIL'}"
            )
    return reports


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="render and validate all fixtures")
    parser.add_argument("--dump", metavar="FILE", help="print the display list of one HTML file")
    parser.add_argument("--kind", choices=("html", "http"), default="html")
    parser.add_argument("--width", type=int, default=DEFAULT_WIDTH)
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--self-test", action="store_true", help="run the checker on synthetic dumps")
    args = parser.parse_args(argv)

    if args.self_test:
        return 0 if _self_test() else 1

    if args.dump:
        payload = Path(args.dump).read_bytes()
        with tempfile.TemporaryDirectory(prefix="webprobe-") as tmp:
            binary = build_dumper(Path(tmp))
            sys.stdout.write(render(binary, args.kind, payload, args.width))
        return 0

    if not args.check:
        parser.print_help()
        return 0

    with tempfile.TemporaryDirectory(prefix="webprobe-") as tmp:
        try:
            binary = build_dumper(Path(tmp))
        except ToolchainMissing as error:
            print(f"webprobe: {error}", file=sys.stderr)
            return 2
        except RuntimeError as error:
            print(f"webprobe: build failed\n{error}", file=sys.stderr)
            return 2
        reports = check_fixtures(binary, args.width, args.verbose)

    failed = [r for r in reports if not r.ok]
    for report in failed:
        print(f"FAIL {report.name}: {len(report.problems)} problem(s)")
        for problem in report.problems[:12]:
            print(f"  - {problem}")
        if len(report.problems) > 12:
            print(f"  ... {len(report.problems) - 12} more")
    if failed:
        print(f"webprobe: {len(failed)}/{len(reports)} fixture(s) failed")
        return 1
    print(f"webprobe: {len(reports)} fixtures rendered, all invariants hold")
    return 0


def _self_test() -> bool:
    """Sanity-check the checker (used by `--self-test` and the unit tests)."""
    good = "\n".join(
        [
            "meta title ok",
            "meta status 200 OK",
            "meta width 640",
            "meta height 200",
            "meta truncated false",
            "cmd fill 0 0 640 200",
            "cmd text 10 20 80 10 1 Hello",
        ]
    )
    bad = "\n".join(
        [
            "meta width 640",
            "meta height 200",
            "meta truncated false",
            "cmd text 600 20 160 10 1 Overflow",
            "cmd fill 0 0 0 10",
            "cmd text -5 30 8 10 1 Negative",
        ]
    )
    ok = check_dump(good).ok and len(check_dump(bad).problems) >= 3
    print(f"webprobe self-test: {'ok' if ok else 'FAILED'}")
    return ok


if __name__ == "__main__":
    sys.exit(main())
