# RustOS Projekt-Regeln

- **Local LLM Bug Reporter**: Wenn immer wir GitHub Actions für Projekte bauen, integriere die "Ollama AI Bug Reporter" Pipeline, falls anwendbar. Das bedeutet: `qwen2.5-coder:3b` in Actions pullen und Compile-Logs per `curl` auswerten und als GitHub Issue reporten.
- **Termux & Android Constraints**: Optimiere bei ressourcenintensiven Aufgaben stets den Speicherbedarf. Nutze bei Cargo-Projekten `.cargo/config.toml` mit limitierten `jobs` (z.B. 4) und reduzierten `codegen-units`.
- **System-Workarounds**: Wenn in Termux Pakete wie `rustup` fehlen, verlinke native Tools (wie `llvm-objcopy`) direkt in den Rustlib-Sysroot. Nutze immer `RUSTC_BOOTSTRAP=1` um `-Z build-std` (Nightly-Features) auf Stable zu erlauben.
- **Subagents & Internet Search**: Führe IMMER intelligente Websuchen durch (z.B. nach Fehlercodes oder Best Practices) und nutze SUBAGENTEN (`invoke_subagent`), um komplexe Probleme parallel und tiefgründig zu analysieren! Denke mit und finde aktiv Fehler, bevor sie auftreten.
