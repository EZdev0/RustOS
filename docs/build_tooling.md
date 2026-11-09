# Build tooling (`tools/rustos.py`)

RustOS is built on phones (Termux), on macOS, on Windows and in CI. `make`,
`bash`, `binutils` and `python` cannot all be assumed to be present, so the
project's build/verification logic lives in **one Python file with no external
dependencies**: [`tools/rustos.py`](../tools/rustos.py).

```bash
python3 tools/rustos.py --help
```

| Command | Does | Exit codes |
| --- | --- | --- |
| `doctor` | reports rustc/cargo/target/qemu/python, workspace members, image status, docs problems | 0 ok, 1 problems |
| `fmt` | `cargo fmt --all -- --check` | 0/1 |
| `clippy` | clippy for `rustos-web` + `builder` (`--full` adds the kernel target) | 0/1 |
| `test` | `cargo test --workspace --exclude kernel` + Python unit tests | 0/1 |
| `docs` | markdown link check + documentation/claim consistency | 0/1 |
| `build` | `cargo run --bin builder` with `CI=1` (no emulator launch) | 0/1 |
| `qemu` | headless boot test (`--seconds`, default 15; `--net none|e1000`) | 0/1, 2 = no qemu |
| `annotate` | turn a captured log into GitHub Actions annotations (`--mode clippy\|tail`) | 0 |
| `verify` | fmt + clippy + tests + docs, writes `target/rustos-report.json` | 0/1 |
| `report` | JSON status (workspace, image, docs) | 0 |

The command table itself is data (`rustos_lib.cargo_command`), so the workflow,
the docs and the tests cannot drift apart.

## The documentation gate

`python3 tools/rustos.py docs` fails when:

* a relative markdown link points at a missing file (README, SECURITY, `docs/`,
  `.agents/`),
* a `docs/*.md` file is not linked from the documentation index,
* the README claims a `codegen-units` value that Cargo.toml does not use,
* the README references `.cargo/config.toml` when the file does not exist.

This gate exists because both drift bugs were real: the README claimed
`codegen-units = 8` (Cargo.toml said `1`) and described a `.cargo/config.toml`
with `jobs = 4` that was never committed. Documentation that lies is worse than
no documentation.

## Tests

```bash
python3 -m unittest discover -s tools/tests -v
```

44 tests cover byte formatting, manifest/profile parsing, MBR magic detection,
serial-log scanning, annotation escaping (ANSI/%), markdown link extraction, the consistency gate, the command
table, subprocess error handling (missing binary → 127, timeout → 124) and the
CLI parser.

## Nightly features without rustup (Termux)

The kernel needs `-Z build-std`, which is a nightly-only flag. `builder` exports
`RUSTC_BOOTSTRAP=1` around the cargo invocation, so a stable Termux `rustc`
accepts it. If `llvm-tools-preview` is missing, link the system tool:

```bash
SYSROOT=$(rustc --print sysroot)
TARGET=$(rustc -vV | grep host | cut -d' ' -f2)
mkdir -p "$SYSROOT/lib/rustlib/$TARGET/bin"
ln -sf "$(command -v llvm-objcopy)" "$SYSROOT/lib/rustlib/$TARGET/bin/llvm-objcopy"
```

## Memory-constrained hosts (phones, 8–12 GB RAM)

Building `core`/`alloc` from source spawns several `rustc` processes. On a phone,
cap the parallelism and keep the compiler's peak RSS low by adding a **local**
`.cargo/config.toml` (not committed, because CI benefits from more jobs):

```toml
[build]
jobs = 4

[profile.dev]
codegen-units = 8
```

The committed release profile stays aggressive (`opt-level = 3`, `lto = "fat"`,
`codegen-units = 1`) because the resulting image is what runs on the device.

## Adding a check

1. put the logic in `tools/rustos_lib.py` as a pure function,
2. add a unit test in `tools/tests/test_rustos_lib.py`,
3. wire it into `run_*` in `tools/rustos.py` **and** into
   `.github/workflows/verify.yml`,
4. document it here and in [ci.md](ci.md).
