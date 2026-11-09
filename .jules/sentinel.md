Date and Title: 2026-10-17 - Fix Panic Risk and Slicing in fs Module
Finding: `unwrap()` and direct slice indexing in `kernel/src/fs/mod.rs` (write_file and delete_file) could trigger unrecoverable Kernel Panics if the path was incorrectly split.
Root Cause: The filesystem parser directly sliced string parts without validating their existence safely through standard Rust abstractions.
Prevention: Replaced slicing and `unwrap()` with `split_last().ok_or(FsError)` to enforce safe memory bounds checking and correct error propagation in Ring 0.
### Bash Variable Naming Constraints in GitHub Actions
When writing shell scripts in GitHub Actions (or generally in bash), variables must follow standard shell naming conventions: they can contain alphanumeric characters and underscores, but not periods (`.`).

Using a name like `os.makedirs_cmd` causes bash to interpret it as an external command named `os.makedirs_cmd` rather than variable assignment (which resulted in a 127 `No such file or directory` error). Replacing it with `OS_MAKEDIRS_CMD` correctly parses as a variable assignment.

---
Date and Title: 2026-11-09 - CI Gate That Could Not Fail (clippy/test were decorative)
Finding: `.github/workflows/build.yml` runs clippy and the test suite with `continue-on-error: true`. A step can only be evaluated as failed when the job fails, so a clippy warning or a failing test produced a green run: the pipeline only opened an AI-written issue.
Root Cause: "report, don't block" was applied to the *gate* steps instead of to the *analysis* steps. Combined with the QEMU panic grep being wrapped in `|| true`, nothing in the pipeline could ever turn red.
Prevention: a second workflow (`verify.yml`) re-runs fmt/clippy/tests/docs/boot without `continue-on-error` and exposes one required check named `verify`. Analysis steps may stay tolerant; gates may not.

Date and Title: 2026-11-09 - Panic Handler Was Blind on Headless Runners
Finding: the kernel's panic handler painted a red screen and stopped. In CI there is no display, so a panic looked exactly like a successful boot.
Root Cause: all diagnostics went to the framebuffer; the serial port was only used by optional `serial_println!` calls that no boot path performed.
Prevention: `serial::init_early()` + a `[boot]` banner in `kernel_main`, and `serial::write_bytes_raw`/`panic_write` in the panic handler (raw port I/O: no locks, no allocator, guarded busy-wait). `tools/rustos.py qemu` now fails when the banner is missing.

Date and Title: 2026-11-09 - Documentation Drift Was Invisible
Finding: `README.md` claimed `codegen-units = 8` while `Cargo.toml` used `1`, and described a `.cargo/config.toml` that did not exist in the repository.
Root Cause: prose was the only source of truth for build-profile facts, and nothing compared prose with manifests.
Prevention: `python3 tools/rustos.py docs` treats the repository as data — it verifies markdown links, that every `docs/*.md` is indexed, that the README's `codegen-units` claim matches `Cargo.toml`, and that referenced config files exist. Run in CI on every push.

Date and Title: 2026-11-09 - Section Regex Leaked Across TOML Profiles
Finding: a unit test for `parse_release_profile` proved that the terminator `(?:\n\[|\Z)` missed indented section headers, so `[profile.dev]` keys were attributed to `[profile.release]`.
Root Cause: the regex assumed section headers always start in column 0.
Prevention: `\n\s*\[` and a regression test. General rule: every parser in `tools/` gets a fixture with *unusual but legal* formatting, not just the formatting of today's file.

Date and Title: 2026-11-09 - Boot-Time Page Fault in the e1000 Driver (hidden for months)
Finding: with a NIC present (`QEMU`'s default user-mode network), the kernel page-faulted at boot while reading `BAR0 + 0x5400` (the e1000 MAC register) — address `0xfeb85400`, error code `0x0`. The bootloader's page tables map the kernel, the boot info and the framebuffer, but not PCI BARs.
Root Cause: `E1000::new` performs a `read_volatile` on MMIO immediately after PCI enumeration, assuming the window is mapped. The panic handler only painted the framebuffer, so on a headless runner the panic was invisible: the pipeline's `grep -iE "panic|triple fault|exception"` saw nothing and reported success. The kernel had been dying at boot in CI whenever a NIC was attached.
Prevention: (1) the panic handler duplicates its report to COM1 through raw port I/O (`serial::panic_write`), so a panic is always observable; (2) `kernel/src/paging.rs` answers "is this virtual address mapped?" by walking CR3 without side effects; (3) `init_e1000` refuses to touch a BAR that is not mapped and logs `[net] ... networking disabled` instead of taking the machine down. A driver must never assume that firmware mapped its MMIO window.

Date and Title: 2026-11-09 - "Tolerant" CI Steps Cannot Fail
Finding: the master pipeline ran the QEMU boot check as `timeout 10 qemu ... || true` followed by a grep. Since the kernel printed nothing on serial, `qemu_output.log` was empty and the grep always passed. Combined with `continue-on-error: true` on the clippy and test steps, the pipeline could not turn red.
Root Cause: tolerance was applied to gates instead of to the reporting steps, and the boot check had no positive signal ("did it boot?") — only the absence of a negative one.
Prevention: the boot check now requires the `[boot]` banner *and* the absence of panic markers, and the failures are published as annotations (`python3 tools/rustos.py annotate`) so they remain readable even when job logs or artifacts are unreachable.
