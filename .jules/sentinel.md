Date and Title: 2026-10-17 - Fix Panic Risk and Slicing in fs Module
Finding: `unwrap()` and direct slice indexing in `kernel/src/fs/mod.rs` (write_file and delete_file) could trigger unrecoverable Kernel Panics if the path was incorrectly split.
Root Cause: The filesystem parser directly sliced string parts without validating their existence safely through standard Rust abstractions.
Prevention: Replaced slicing and `unwrap()` with `split_last().ok_or(FsError)` to enforce safe memory bounds checking and correct error propagation in Ring 0.
### Bash Variable Naming Constraints in GitHub Actions
When writing shell scripts in GitHub Actions (or generally in bash), variables must follow standard shell naming conventions: they can contain alphanumeric characters and underscores, but not periods (`.`).

Using a name like `os.makedirs_cmd` causes bash to interpret it as an external command named `os.makedirs_cmd` rather than variable assignment (which resulted in a 127 `No such file or directory` error). Replacing it with `OS_MAKEDIRS_CMD` correctly parses as a variable assignment.
