# AGENTS.md

Instructions for coding agents working in this repository.

## Purpose

- Project: `mybar`
- Scope: minimal Wayland taskbar in Rust using GTK4 + `gtk4-layer-shell`
- Principle: keep changes incremental, predictable, and reproducible

## Standard Workflow

Run commands from repo root:

- Generate/update lockfile: `make lock`
- Always make sure this command passes changes: `make ci`

If dependencies change in `Cargo.toml`, update and commit `Cargo.lock`.

## Technical Guardrails

- Prefer lockfile-based commands (`--locked`) where defined.
- Avoid large refactors unless explicitly requested.

## Files That Must Stay In Sync

When behavior, commands, architecture, or docs structure change, update:

- `README.md` for user-facing usage/docs
- `SESSION_NOTES.md` for concise future-session orientation
- `docs/modules.md` for module config/styling behavior
- `docs/developer.md` for architecture/extension workflow
- `config.jsonc` example if config schema/defaults changed

## Safety

- Never commit secrets/tokens/keys.
- Do not run destructive git/file operations unless explicitly requested.
- If unexpected unrelated local changes appear, pause and ask before proceeding.

## Maintenance

- Treat this file as the canonical agent contract for this repo.
- Update this file when team workflow or project standards change.
