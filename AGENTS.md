# AGENTS.md

Instructions for coding agents working in this repository.

## Purpose

- Project: `vibar`
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

## Documentation Ownership

Each doc file has a clear scope — avoid duplicating information across files:

- `docs/modules.md` — canonical reference for module config, behavior, and styling (update here when module behavior changes)
- `docs/developer.md` — code structure, file locations, implementation decisions (update here when architecture changes)
- `README.md` — user-facing overview, build/run commands, feature highlights (update here when user-facing workflow changes)
- `config.jsonc` — example config (update here when config schema/defaults change)

## Safety

- Never commit secrets/tokens/keys.
- Do not run destructive git/file operations unless explicitly requested.
- If unexpected unrelated local changes appear, pause and ask before proceeding.

## Maintenance

- Treat this file as the canonical agent contract for this repo.
- Update this file when team workflow or project standards change.
