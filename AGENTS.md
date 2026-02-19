# AGENTS.md

Instructions for coding agents working in this repository.

## Purpose

- Project: `mybar`
- Scope: minimal Wayland taskbar in Rust using GTK4 + `gtk4-layer-shell`
- Principle: keep changes incremental, predictable, and reproducible

## Standard Workflow

Run commands from repo root:

- Install deps/toolchain: `make deps`
- Generate/update lockfile: `make lock`
- Build: `make build`
- Run: `make run`
- Full local CI checks: `make ci`

If dependencies change in `Cargo.toml`, update and commit `Cargo.lock`.

## Technical Guardrails

- Prefer lockfile-based commands (`--locked`) where defined.
- Do not bypass `scripts/build.sh` checks for normal builds.
- Keep module/config changes backwards compatible when possible.
- Avoid large refactors unless explicitly requested.

## Files That Must Stay In Sync

When behavior, commands, or architecture changes, update:

- `README.md` for user-facing usage/docs
- `SESSION_NOTES.md` for concise future-session orientation
- `config.jsonc` example if config schema/defaults changed

## Style and Change Hygiene

- Keep patches focused and minimal.
- Preserve existing naming/style unless there is a strong reason.
- Add comments only where logic is non-obvious.
- Never commit transient/local files (for example `.session`, `target/`).

## Safety

- Never commit secrets/tokens/keys.
- Do not run destructive git/file operations unless explicitly requested.
- If unexpected unrelated local changes appear, pause and ask before proceeding.

## Maintenance

- Treat this file as the canonical agent contract for this repo.
- Update this file when team workflow or project standards change.
