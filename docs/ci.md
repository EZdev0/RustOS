# CI pipelines: what runs, what blocks a merge, how to reproduce locally

RustOS has two GitHub Actions workflows with different jobs:

| Workflow | File | Purpose | Blocks merges? |
| --- | --- | --- | --- |
| ✅ **RustOS Verify** | `.github/workflows/verify.yml` | hard gate: formatting, lints, tests, docs, boot | **yes** (required check `verify`) |
| 🦀 **RustOS Master Pipeline** | `.github/workflows/build.yml` | images, analysis report, releases, AI reporter | no (reporting/publishing) |
| 🔄 Dependabot Auto-Installer | `.github/workflows/dependabot.yml` | dependency + security updates | no |
| 🤖 Multi-agent workflows | `.github/workflows/rustos-*.yml`, `ules-brainstormer-orchestrator.yml`, `jules-owner-single-session.yml` | issue/PR automation, local-LLM tooling | no |

## Why two pipelines?

The master pipeline is intentionally *tolerant*: it runs clippy and tests with
`continue-on-error: true` so that a failure still produces a log, a QEMU serial
capture, and an AI-written issue. The side effect is that **a clippy or test
failure cannot turn that pipeline red** — it reports success and only opens an
issue. That is a reasonable release pipeline but a bad gate.

`verify.yml` therefore re-runs the important checks in strict mode
(`-D warnings`, no `continue-on-error`) and publishes a single required status
check named `verify`. The tolerant steps in `build.yml` now say so explicitly
("`✅ RustOS Verify` is the blocking gate") instead of looking like a real gate,
and the release body is written through a heredoc so its markdown is not full of
literal `\n` sequences.

### What `verify.yml` does

```text
tooling   python3 -m unittest discover -s tools/tests
          python3 tools/rustos.py docs
          python3 tools/rustos.py doctor
fmt       cargo fmt --all -- --check
clippy    -p rustos-web -p builder --all-targets -- -D warnings
          -p kernel --target x86_64-unknown-none -Z build-std=… -- -D warnings
test      cargo test --workspace --exclude kernel
          cargo test -p rustos-web --lib
          python3 -m unittest discover -s tools/tests
boot      python3 tools/rustos.py build            (kernel + MBR image)
          python3 tools/rustos.py qemu --net none    (15 s, no NIC: must boot)
          python3 tools/rustos.py qemu --net e1000    (20 s, NIC present: must
                                                      boot, i.e. the driver has
                                                      to skip an unmapped BAR)
ai-report only on main when something failed → opens a READ-ONLY issue
verify    aggregates the five jobs; fails if any of them did not succeed
```

All jobs run on `ubuntu-latest`, use `concurrency` cancellation, cache
`~/.cargo/registry` + `target`, and have explicit `timeout-minutes` so a hung
QEMU cannot burn the run budget.

### Failure output you can actually read

Job logs and uploaded artifacts live on a storage host that automation cannot
always reach. Therefore every failing step publishes its diagnostics twice:

* `python3 tools/rustos.py annotate --file <log> [--mode clippy]` turns the last
  lines (or the compiler diagnostics) into `::error::` annotations, which are
  readable through the checks API, and
* `tools/ci/publish_logs.sh` force-pushes the captured logs to the `ci-logs`
  branch (`contents: write` is granted to the clippy and boot jobs only).

Both mechanisms were the reason the boot-time page fault described below could
finally be diagnosed: `git show origin/ci-logs:qemu_serial.log` contained the
full panic.

### Bugs this pipeline found on its first runs

| Bug | How it showed up |
| --- | --- |
| Kernel page-faulted at boot whenever a NIC was attached (unmapped PCI BAR) | serial log contained `[PANIC] ... PAGE FAULT ... 0xfeb85400` |
| Panic handler was invisible headless (framebuffer only) | the log above was empty before the serial change |
| README claimed `codegen-units = 8`, a `.cargo/config.toml` that does not exist | `tools/rustos.py docs` |
| `clippy::collapsible_match` in the browser app | kernel clippy job |
| A non-existent action SHA (`dtolnay/rust-toolchain@6d653a…`) | `cargo fmt` job failed at setup |

### The boot test is real

`kernel_main` prints a banner on COM1 *before* it touches the framebuffer:

```text
[boot] RustOS 0.2.0 entering kernel_main
[boot] heap ready (16384 KiB)
[boot] framebuffer 800x600 (3 bpp)
[boot] entering scheduler loop
```

The panic handler writes `[PANIC] …` to the same port (raw port I/O, no locks, no
allocation), so `tools/rustos.py qemu` can distinguish three outcomes:

* no `[boot]` banner → the kernel never started (bootloader/emulator problem),
  * the e1000 regression test additionally requires the boot to survive a NIC
    being present (`[net] e1000 at ... is not mapped ... networking disabled`),
* `[PANIC]`/`Exception`/`triple fault` markers → a regression,
* banner without markers → pass.

## Reproducing CI locally

```bash
python3 tools/rustos.py doctor            # toolchain, target, qemu, image
python3 tools/rustos.py verify            # everything except kernel clippy
python3 tools/rustos.py verify --full     # + clippy for x86_64-unknown-none
python3 tools/rustos.py build && python3 tools/rustos.py qemu
```

Same commands, same exit codes: `0` success, `1` failure, `2` missing tooling.

## The AI failure reporter

Per the project rules (`.agents/AGENTS.md`), a local LLM analyses failures:

* the master pipeline installs Ollama and pulls `qwen2.5-coder:3b` **only after**
  a failure/success of the build job, and
* the verify pipeline does the same in an `ai-report` job that is limited to
  `main`, runs *after* everything else, and has **read-only** code access: it
  creates an issue with the job results and the model's suggestion, never a
  commit, branch or PR.

### Supply-chain note

`curl -fsSL https://ollama.com/install.sh | sh` executes a remote script inside
CI. It is gated by `if: failure()`/`ref == main`, does not receive repository
write tokens beyond `issues: write`, and runs after the build, but it is still a
moving target. Follow-up (tracked in `SECURITY.md`): pin the installer by SHA-256
or use a container image digest. The reporter is best-effort: `|| true` keeps a
failed install from hiding the real job failure.

## Verified state

Both pipelines were re-run on 2026-11-09 and are green end to end:

| Run | Workflow | Result |
| --- | --- | --- |
| `42701358011` | ✅ RustOS Verify | 7/7 jobs: tooling & docs, fmt, clippy host, clippy kernel, tests, build & boot (two QEMU boots), `verify` |
| `42701357544` | 🦀 RustOS Master Pipeline | 3/3 platforms built, image analysis succeeded, summary succeeded (release skipped on PRs) |

Kernel clippy runs with `-D warnings` against `x86_64-unknown-none`, so the
bare-metal code is linted as strictly as the host crates. The only finding that
was still open after the first green run was `cargo`'s optional
`unused_crate_dependencies` note about `volatile` in `kernel/Cargo.toml`; it is a
nightly `cargo` lint, not a `rustc`/clippy warning, and it does not fail the job.

## Artifacts

| Artifact | Content | Retention |
| --- | --- | --- |
| `rustos-image-verify` | `my_rust_os_desktop.img`, `qemu_serial.log` | 7 days |
| `clippy-host.log`, `clippy-kernel.log` | full lint output | 7 days |
| `boot.log`, `boot-none.log` | output of both boot tests | 7 days |
| `verify-report` | `target/rustos-status.json` (workspace, image, docs status) | 7 days |
| `RustOS-Limbo-Image-*` | per-OS images from the master pipeline | 14 days |
| `RustOS-Analysis-Report` | binwalk/fdisk report | 14 days |

## Release flow

`build.yml` builds the image on Linux, macOS and Windows, scans it with
`binwalk`/`fdisk` on Linux, writes `update_manifest.json` (size + CRC32), and
creates a GitHub release `v1.0.<run_number>`. The in-OS Update Center consumes
exactly that manifest (`kernel/src/desktop/updater.rs`).
