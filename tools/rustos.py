#!/usr/bin/env python3
"""RustOS build & verification tool.

    python3 tools/rustos.py doctor      # is this machine able to build RustOS?
    python3 tools/rustos.py fmt         # formatting gate
    python3 tools/rustos.py clippy      # host crates (fast, no nightly needed)
    python3 tools/rustos.py test        # all host unit tests
    python3 tools/rustos.py docs        # documentation link/consistency gate
    python3 tools/rustos.py build       # kernel + bootable image
    python3 tools/rustos.py qemu        # headless boot smoke test
    python3 tools/rustos.py verify      # everything that works without QEMU
    python3 tools/rustos.py report      # machine-readable summary (JSON)

Design notes: stdlib only, no `pip install`, no network access, works on
Termux/macOS/Windows and mirrors exactly what `.github/workflows/verify.yml`
runs. Python is used deliberately: the build host (e.g. a phone running Termux)
cannot be assumed to have a working `make`, `bash` or `binutils`.
"""

from __future__ import annotations

import argparse
import os
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rustos_lib as lib  # noqa: E402

ROOT = lib.repo_root()
IMAGE = ROOT / "target" / lib.IMAGE_NAME

BOLD = "\033[1m"
GREEN = "\033[32m"
RED = "\033[31m"
YELLOW = "\033[33m"
RESET = "\033[0m"


def color(text: str, code: str, enabled: bool = True) -> str:
    if not enabled or not sys.stdout.isatty():
        return text
    return f"{code}{text}{RESET}"


def say(message: str) -> None:
    print(message, flush=True)


def run_step(report: lib.Report, name: str, argv: list[str], env: dict | None = None,
             timeout: int | None = None, quiet: bool = False) -> bool:
    """Run one command, record it in the report, print a compact summary."""
    merged = os.environ.copy()
    if env:
        merged.update(env)
    say(f"{BOLD}==> {name}{RESET}: {' '.join(argv)}")
    code, output = lib.run_command(argv, ROOT, env=merged, timeout=timeout)
    if code == 0:
        if not quiet:
            tail = lib.tail(output, 5)
            if tail:
                say(tail)
        report.add(lib.CheckResult(name, True, f"{' '.join(argv)}"))
        say(color(f"    ok ({name})", GREEN))
        return True
    say(lib.tail(output, 30))
    report.add(lib.CheckResult(name, False, lib.tail(output, 3).replace("\n", " | ")))
    say(color(f"    failed ({name}) exit={code}", RED))
    return False


def cargo_available() -> bool:
    return shutil.which("cargo") is not None


def rustup_target_installed(target: str) -> bool | None:
    if shutil.which("rustup") is None:
        return None
    code, output = lib.run_command(["rustup", "target", "list", "--installed"], ROOT)
    if code != 0:
        return None
    return target in output.split()


def cmd_doctor(_args: argparse.Namespace) -> int:
    say(f"{BOLD}RustOS doctor{RESET}")
    code, rustc = lib.run_command(["rustc", "--version"], ROOT)
    say(f"  rustc       : {rustc.strip() if code == 0 else 'NOT FOUND'}")
    code, cargo = lib.run_command(["cargo", "--version"], ROOT)
    say(f"  cargo       : {cargo.strip() if code == 0 else 'NOT FOUND'}")
    installed = rustup_target_installed(lib.KERNEL_TARGET)
    if installed is None:
        say(f"  target      : rustup not present (target check skipped)")
    else:
        say(f"  target      : {lib.KERNEL_TARGET} {'installed' if installed else 'MISSING'}")
    say(f"  qemu        : {shutil.which('qemu-system-x86_64') or 'not installed'}")
    say(f"  python      : {sys.version.split()[0]}")
    say(f"  repo root   : {ROOT}")

    manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    members = lib.parse_workspace_members(manifest)
    say(f"  workspace   : {', '.join(members) or 'unknown'}")
    for member in members:
        exists = (ROOT / member / "Cargo.toml").exists()
        say(f"    - {member:<12} {'ok' if exists else 'MISSING Cargo.toml'}")

    image = lib.inspect_image(IMAGE)
    if image["exists"]:
        say(
            f"  image       : {image['human_size']} "
            f"({'bootable' if image['bootable'] else 'NOT bootable'})"
        )
    else:
        say("  image       : not built yet (python3 tools/rustos.py build)")

    problems = lib.profile_consistency(ROOT)
    problems += lib.check_doc_index(ROOT)
    broken = lib.check_markdown_links(ROOT, lib.iter_docs(ROOT))
    say(f"  docs        : {len(broken)} broken links, {len(problems)} consistency issues")
    for problem in broken + problems:
        say(f"    ! {problem}")
    return 0 if not (broken or problems) else 1


def run_fmt(report: lib.Report) -> bool:
    for argv in lib.cargo_command("fmt"):
        return run_step(report, "fmt", argv)
    return True


def run_clippy(report: lib.Report, full: bool) -> bool:
    ok = True
    for argv in lib.cargo_command("clippy-host"):
        ok &= run_step(report, "clippy", argv)
    if full:
        env = {"RUSTC_BOOTSTRAP": "1"}
        for argv in lib.cargo_command("clippy-kernel"):
            ok &= run_step(report, "clippy-kernel", argv, env=env)
    return ok


def run_tests(report: lib.Report) -> bool:
    ok = True
    for argv in lib.cargo_command("test"):
        ok &= run_step(report, "test", argv)
    ok &= run_step(
        report,
        "test-python",
        [sys.executable, "-m", "unittest", "discover", "-s", "tools/tests", "-v"],
    )
    return ok


def run_docs(report: lib.Report) -> bool:
    problems = lib.check_markdown_links(ROOT, lib.iter_docs(ROOT))
    problems += lib.profile_consistency(ROOT)
    problems += lib.check_doc_index(ROOT)
    if problems:
        say("\n".join(f"  ! {problem}" for problem in problems))
        report.add(lib.CheckResult("docs", False, f"{len(problems)} problems"))
        return False
    report.add(lib.CheckResult("docs", True, "all links and claims verified"))
    say(color("    ok (docs)", GREEN))
    return True


def cmd_verify(args: argparse.Namespace) -> int:
    report = lib.Report()
    if not cargo_available():
        say(color("cargo not found - install Rust first (see README)", RED))
        return 2
    run_fmt(report)
    run_clippy(report, full=args.full)
    run_tests(report)
    run_docs(report)

    say("")
    say(f"{BOLD}Summary{RESET}")
    say(report.to_markdown())
    out = ROOT / "target" / "rustos-report.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(report.to_json(), encoding="utf-8")
    say(f"\nreport written to {out.relative_to(ROOT)}")
    if not report.ok:
        say(color(lib.summarize_failures(report), RED))
        return 1
    return 0


def cmd_build(report: lib.Report) -> int:
    if not cargo_available():
        say(color("cargo not found", RED))
        return 2
    env = {"CI": "1", "RUSTC_BOOTSTRAP": "1"}
    for argv in lib.cargo_command("build"):
        if not run_step(report, "build", argv, env=env):
            return 1
    image = lib.inspect_image(IMAGE)
    if not image["exists"]:
        say(color(f"expected image at {IMAGE}", RED))
        return 1
    say(
        f"image: {image['human_size']} "
        f"({'bootable' if image['bootable'] else 'NOT BOOTABLE'})"
    )
    return 0 if image["bootable"] else 1


def cmd_qemu(args: argparse.Namespace) -> int:
    image = lib.inspect_image(IMAGE)
    if not image["exists"]:
        say(color("no image - run `python3 tools/rustos.py build` first", RED))
        return 2
    if shutil.which("qemu-system-x86_64") is None:
        say(color("qemu-system-x86_64 not installed", YELLOW))
        return 2
    log_path = ROOT / "target" / "qemu_serial.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    argv = [
        "qemu-system-x86_64",
        "-drive",
        f"format=raw,file={IMAGE}",
        "-display",
        "none",
        "-serial",
        "file:" + str(log_path),
        "-no-reboot",
        "-m",
        "256M",
    ]
    say(f"{BOLD}==> qemu boot test ({args.seconds}s){RESET}")
    code, _ = lib.run_command(argv, ROOT, timeout=args.seconds)
    text = log_path.read_text(encoding="utf-8", errors="replace") if log_path.exists() else ""
    failures = lib.scan_log_for_panics(text)
    say(f"serial log: {log_path} ({lib.human_bytes(len(text))})")
    if failures:
        say(color("panic markers found:", RED))
        say("\n".join(f"  {line}" for line in failures[:10]))
        return 1
    if not text.strip():
        say(color("serial log is empty - the kernel printed nothing", YELLOW))
        return 1
    if not lib.boot_marker_present(text):
        say(
            color(
                f"serial log has no '{lib.BOOT_MARKER}' banner - kernel did not boot",
                RED,
            )
        )
        say(lib.tail(text, 15))
        return 1
    say(color("boot test passed: no panic markers", GREEN))
    return 0


def cmd_report(_args: argparse.Namespace) -> int:
    image = lib.inspect_image(IMAGE)
    members = lib.parse_workspace_members((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    payload = {
        "workspace": members,
        "image": image,
        "docs": {
            "broken_links": lib.check_markdown_links(ROOT, lib.iter_docs(ROOT)),
            "consistency": lib.profile_consistency(ROOT),
            "index": lib.check_doc_index(ROOT),
        },
    }
    import json

    say(json.dumps(payload, indent=2))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="rustos.py",
        description="Build and verify the RustOS kernel without a shell script.",
    )
    sub = parser.add_subparsers(dest="command")

    sub.add_parser("doctor", help="report toolchain/board status")
    sub.add_parser("fmt", help="check formatting")
    clippy = sub.add_parser("clippy", help="run clippy")
    clippy.add_argument("--full", action="store_true", help="also lint the kernel target")
    sub.add_parser("test", help="run host tests (Rust + Python)")
    sub.add_parser("docs", help="check documentation")
    sub.add_parser("build", help="build kernel and bootable image")
    qemu = sub.add_parser("qemu", help="headless boot smoke test")
    qemu.add_argument("--seconds", type=int, default=15)
    verify = sub.add_parser("verify", help="fmt + clippy + tests + docs")
    verify.add_argument("--full", action="store_true", help="include kernel clippy")
    sub.add_parser("report", help="print a JSON status report")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    report = lib.Report()

    if args.command == "doctor":
        return cmd_doctor(args)
    if args.command == "fmt":
        return 0 if run_fmt(report) else 1
    if args.command == "clippy":
        return 0 if run_clippy(report, full=args.full) else 1
    if args.command == "test":
        return 0 if run_tests(report) else 1
    if args.command == "docs":
        return 0 if run_docs(report) else 1
    if args.command == "build":
        return cmd_build(report)
    if args.command == "qemu":
        return cmd_qemu(args)
    if args.command == "verify":
        return cmd_verify(args)
    if args.command == "report":
        return cmd_report(args)

    parser.print_help()
    return 0


if __name__ == "__main__":
    sys.exit(main())
