"""Side-effect-light helpers for the RustOS build tool.

Everything in this module is pure (or takes an injected runner) so that it can be
unit tested on any machine, including the CI runner and a Termux phone:

    python3 -m unittest discover -s tools/tests -v

The command line wrapper lives in ``tools/rustos.py``; the rules it enforces are
documented in ``docs/build_tooling.md``.
"""

from __future__ import annotations

import json
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Iterable, Sequence

IMAGE_NAME = "my_rust_os_desktop.img"
KERNEL_TARGET = "x86_64-unknown-none"
KERNEL_CRATE = "kernel"
WEB_CRATE = "rustos-web"
BUILDER_CRATE = "builder"

# Serial-log markers that mean the boot test failed even if QEMU exited 0.
FAILURE_MARKERS: tuple[str, ...] = (
    "panic",
    "PANIC",
    "triple fault",
    "Exception",
    "EXCEPTION",
    "page fault",
    "allocation error",
    "unhandled",
)

# The kernel prints this from `kernel_main` before touching the framebuffer.
BOOT_MARKER = "[boot]"

# Lines that contain a failure marker but are known-benign.
FAILURE_WHITELIST: tuple[str, ...] = (
    "panic = abort",
    "panic-abort",
)


def repo_root() -> Path:
    """Repository root (the parent of the ``tools`` directory)."""
    return Path(__file__).resolve().parent.parent


@dataclass
class CheckResult:
    """Outcome of a single verification step."""

    name: str
    ok: bool
    detail: str = ""

    def as_dict(self) -> dict:
        return {"name": self.name, "ok": self.ok, "detail": self.detail}


@dataclass
class Report:
    """Collects check results and renders the final summary."""

    checks: list[CheckResult] = field(default_factory=list)

    def add(self, result: CheckResult) -> CheckResult:
        self.checks.append(result)
        return result

    @property
    def ok(self) -> bool:
        return all(check.ok for check in self.checks)

    @property
    def failed(self) -> list[CheckResult]:
        return [check for check in self.checks if not check.ok]

    def to_json(self) -> str:
        return json.dumps(
            {
                "ok": self.ok,
                "checks": [check.as_dict() for check in self.checks],
            },
            indent=2,
        )

    def to_markdown(self) -> str:
        lines = ["| check | result | detail |", "| --- | --- | --- |"]
        for check in self.checks:
            status = "PASS" if check.ok else "FAIL"
            detail = check.detail.replace("|", "\\|")
            lines.append(f"| `{check.name}` | {status} | {detail} |")
        return "\n".join(lines)


def human_bytes(size: int) -> str:
    """Format a byte count with a binary unit suffix."""
    if size < 0:
        return f"{size} B"
    units = ("B", "KiB", "MiB", "GiB")
    value = float(size)
    unit = units[0]
    for unit in units:
        if value < 1024 or unit == units[-1]:
            break
        value /= 1024
    if unit == "B":
        return f"{int(value)} B"
    return f"{value:.1f} {unit}"


def parse_workspace_members(cargo_toml: str) -> list[str]:
    """Extract ``members = [...]`` from a workspace manifest."""
    match = re.search(r"members\s*=\s*\[(.*?)\]", cargo_toml, re.S)
    if not match:
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


def parse_release_profile(cargo_toml: str) -> dict[str, str]:
    """Extract ``[profile.release]`` key/value pairs as raw strings."""
    # Section headers may be indented, so allow leading whitespace before the
    # next `[section]` when looking for the end of the block.
    match = re.search(r"\[profile\.release\](.*?)(?:\n\s*\[|\Z)", cargo_toml, re.S)
    if not match:
        return {}
    profile: dict[str, str] = {}
    for line in match.group(1).splitlines():
        line = line.split("#", 1)[0].strip()
        if not line or "=" not in line:
            continue
        key, value = line.split("=", 1)
        profile[key.strip()] = value.strip().strip('"')
    return profile


def scan_log_for_panics(text: str) -> list[str]:
    """Return the log lines that indicate a kernel panic or boot failure."""
    hits: list[str] = []
    for line in text.splitlines():
        if not any(marker in line for marker in FAILURE_MARKERS):
            continue
        if any(benign in line for benign in FAILURE_WHITELIST):
            continue
        hits.append(line.strip())
    return hits


_ANSI_RE = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")


def strip_ansi(text: str) -> str:
    """Remove ANSI escape sequences (colour codes in captured output)."""
    return _ANSI_RE.sub("", text)


def gha_escape(text: str) -> str:
    """Escape a message for a GitHub Actions workflow command."""
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def gha_error(message: str) -> str:
    """Build an `::error::` annotation line from arbitrary captured output."""
    clean = strip_ansi(message).replace("\r", "").strip()
    clean = clean.replace("::error::", "").replace("::warning::", "")
    return "::error::" + gha_escape(clean)


def annotate_lines(text: str, limit: int = 8) -> list[str]:
    """Turn the last `limit` non-empty lines into annotations."""
    lines = [line for line in strip_ansi(text).splitlines() if line.strip()]
    if not lines:
        return [gha_error("(no output was captured)")]
    return [gha_error(line) for line in lines[-limit:]]


def clippy_annotations(text: str, limit: int = 8) -> list[str]:
    """Pick compiler diagnostics out of a clippy/cargo log for annotations."""
    lines = strip_ansi(text).splitlines()
    found: list[str] = []
    for index, line in enumerate(lines):
        if re.match(r"^(error|warning)(\[[^\]]*\])?:", line):
            location = ""
            if index + 1 < len(lines):
                following = lines[index + 1].strip()
                if following.startswith("-->"):
                    location = " @ " + following[3:].strip()
            found.append(gha_error(line + location))
            if len(found) >= limit:
                break
    return found or annotate_lines(text, limit)


def boot_marker_present(text: str) -> bool:
    """True when the kernel actually reached `kernel_main`."""
    return BOOT_MARKER in text


def image_magic(image: bytes) -> bool:
    """True when the buffer carries the 0x55AA MBR boot signature."""
    if len(image) < 512:
        return False
    return image[510] == 0x55 and image[511] == 0xAA


def inspect_image(path: Path, read_head: Callable[[Path, int], bytes] | None = None) -> dict:
    """Summarise a disk image without shelling out to external tools."""
    reader = read_head or (lambda p, n: p.read_bytes()[:n])
    if not path.exists():
        return {"exists": False, "path": str(path), "size": 0, "bootable": False}
    size = path.stat().st_size
    head = reader(path, 512)
    return {
        "exists": True,
        "path": str(path),
        "size": size,
        "human_size": human_bytes(size),
        "bootable": image_magic(head),
    }


def markdown_links(text: str) -> list[str]:
    """Extract relative link targets from markdown (ignoring URLs and anchors)."""
    targets: list[str] = []
    for match in re.finditer(r"\[[^\]]*\]\(([^)]+)\)", text):
        target = match.group(1).strip()
        target = target.split("#", 1)[0]
        if not target:
            continue
        if re.match(r"^[a-z]+://", target) or target.startswith("mailto:"):
            continue
        targets.append(target)
    return targets


def check_markdown_links(root: Path, files: Sequence[Path]) -> list[str]:
    """Return a list of broken relative links found in ``files``."""
    problems: list[str] = []
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as error:  # pragma: no cover - unreadable file
            problems.append(f"{path}: unreadable ({error})")
            continue
        for target in markdown_links(text):
            resolved = (path.parent / target).resolve()
            if not resolved.exists():
                problems.append(f"{path.name}: broken link -> {target}")
    return problems


def profile_consistency(root: Path) -> list[str]:
    """Cross-check documentation claims against the real Cargo profiles."""
    problems: list[str] = []
    manifest = root / "Cargo.toml"
    readme = root / "README.md"
    if not manifest.exists():
        return ["Cargo.toml missing"]
    profile = parse_release_profile(manifest.read_text(encoding="utf-8"))
    codegen_units = profile.get("codegen-units")
    if codegen_units is not None and readme.exists():
        text = readme.read_text(encoding="utf-8")
        claims = re.findall(r"codegen-units\s*=\s*(\d+)", text)
        for claim in claims:
            if claim != codegen_units:
                problems.append(
                    f"README claims codegen-units = {claim} but Cargo.toml says {codegen_units}"
                )
    # A documented `.cargo/config.toml` must actually exist.
    if readme.exists():
        text = readme.read_text(encoding="utf-8")
        if ".cargo/config.toml" in text and not (root / ".cargo" / "config.toml").exists():
            problems.append("README references .cargo/config.toml which does not exist")
    return problems


def check_doc_index(root: Path) -> list[str]:
    """Every `docs/*.md` must be reachable from the documentation index."""
    docs_dir = root / "docs"
    if not docs_dir.is_dir():
        return []
    index_text = ""
    for candidate in (docs_dir / "README.md", root / "README.md"):
        if candidate.exists():
            index_text += candidate.read_text(encoding="utf-8")
    problems: list[str] = []
    for path in sorted(docs_dir.glob("*.md")):
        if path.name == "README.md":
            continue
        if path.name not in index_text:
            problems.append(f"docs/{path.name} is not linked from the documentation index")
    return problems


def cargo_command(step: str) -> list[list[str]]:
    """Return the cargo invocations for a named build step.

    Kept as data so the CI workflow, the docs and the tests cannot drift apart.
    """
    plans = {
        "fmt": [["cargo", "fmt", "--all", "--", "--check"]],
        "clippy-host": [
            ["cargo", "clippy", "-p", WEB_CRATE, "--all-targets", "--", "-D", "warnings"],
            ["cargo", "clippy", "-p", BUILDER_CRATE, "--all-targets", "--", "-D", "warnings"],
        ],
        "clippy-kernel": [
            [
                "cargo",
                "clippy",
                "-p",
                KERNEL_CRATE,
                "--target",
                KERNEL_TARGET,
                "-Z",
                "build-std=core,alloc,compiler_builtins",
                "--",
                "-D",
                "warnings",
            ]
        ],
        "test": [
            ["cargo", "test", "--workspace", "--exclude", KERNEL_CRATE],
        ],
        "build": [["cargo", "run", "--bin", "builder"]],
    }
    return plans.get(step, [])


def run_command(
    argv: Sequence[str],
    cwd: Path,
    env: dict[str, str] | None = None,
    runner: Callable[..., subprocess.CompletedProcess] = subprocess.run,
    timeout: int | None = None,
) -> tuple[int, str]:
    """Run a command, returning ``(exit_code, combined_output)``.

    A missing executable is reported as exit code 127 instead of raising, so the
    tool can print a helpful hint on a fresh Termux install.
    """
    try:
        completed = runner(
            list(argv),
            cwd=str(cwd),
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except FileNotFoundError:
        return 127, f"command not found: {argv[0]}"
    except subprocess.TimeoutExpired:
        return 124, f"timeout after {timeout}s: {' '.join(argv)}"
    output = (completed.stdout or "") + (completed.stderr or "")
    return completed.returncode, output


def tail(text: str, lines: int = 25) -> str:
    """Last ``lines`` lines of ``text`` (used for failure summaries)."""
    parts = text.strip().splitlines()
    if len(parts) <= lines:
        return "\n".join(parts)
    return "\n".join(parts[-lines:])


def summarize_failures(report: Report) -> str:
    """Human readable list of failing checks."""
    if report.ok:
        return "all checks passed"
    return "\n".join(
        f"- {check.name}: {check.detail or 'failed'}" for check in report.failed
    )


def iter_docs(root: Path) -> Iterable[Path]:
    """All markdown files that document the project."""
    candidates = [root / "README.md", root / "SECURITY.md"]
    docs_dir = root / "docs"
    if docs_dir.is_dir():
        candidates.extend(sorted(docs_dir.glob("*.md")))
    agents_dir = root / ".agents"
    if agents_dir.is_dir():
        candidates.extend(sorted(agents_dir.rglob("*.md")))
    return [path for path in candidates if path.exists()]
