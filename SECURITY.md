# Security Policy for RustOS

## Supported Versions

We currently support the following versions of the RustOS kernel and builder components with security patches:

| Version | Supported          | Description |
| ------- | ------------------ | ----------- |
| `main`  | :white_check_mark: | Rolling release / active development |
| `0.1.x` | :white_check_mark: | Current stable prototype |
| `< 0.1` | :x:                | Unsupported |

## Threat Model & In-Scope Vulnerabilities

RustOS is a `#![no_std]` kernel relying heavily on Rust's memory safety guarantees. We take kernel-level exploits seriously. The following are considered critical vulnerabilities:

- **Memory Safety Violations:** Any reliable method to trigger Use-After-Free (UAF), Double Free, or Buffer Overflows by bypassing Rust's `unsafe` invariants.
- **Kernel Panics (DoS):** Any unhandled `.unwrap()`, out-of-bounds array access, or division-by-zero that allows a malicious userspace application to crash the kernel scheduler.
- **Privilege Escalation:** Escaping userspace boundaries to execute arbitrary code in Ring 0.
- **CI/CD Supply Chain:** Leaked secrets or injection vulnerabilities in the Multi-Agent `/Jules-brainstormer` and `@rustos-ci` pipelines.

*Out of scope:* Physical attacks (e.g., cold boot attacks, hardware fault injections) and physical side-channels.

## Reporting a Vulnerability

**Please DO NOT report security vulnerabilities through public GitHub issues.**

1. Navigate to the **Security** tab in this repository.
2. Click on **Advisories** and then **Report a vulnerability** (GitHub Private Vulnerability Reporting).
3. Provide a detailed description of the vulnerability, including:
   - The affected subsystem (e.g., `kernel/src/allocator.rs` or `kernel/src/hardware/e1000.rs`).
   - A Proof of Concept (PoC) or a clear set of steps to reproduce the issue.
   - The potential impact of the exploit.

You can expect an initial response from the repository owners within 48 hours. If the vulnerability is confirmed, we will coordinate a fix in a private branch using our internal CI agents and release a security advisory once patched.
