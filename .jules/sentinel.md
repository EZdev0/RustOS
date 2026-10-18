Date and Title: 2026-10-17 - Fix Panic Risk and Slicing in fs Module
Finding: `unwrap()` and direct slice indexing in `kernel/src/fs/mod.rs` (write_file and delete_file) could trigger unrecoverable Kernel Panics if the path was incorrectly split.
Root Cause: The filesystem parser directly sliced string parts without validating their existence safely through standard Rust abstractions.
Prevention: Replaced slicing and `unwrap()` with `split_last().ok_or(FsError)` to enforce safe memory bounds checking and correct error propagation in Ring 0.
