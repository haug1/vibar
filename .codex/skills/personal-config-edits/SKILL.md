---
name: personal-config-edits
description: Edit personal vibar runtime config files under ~/.config/vibar (especially config.jsonc and style.css). Use when the user asks to change their personal bar setup, styling, or module behavior and does not explicitly request modifying repository defaults/examples.
---

# Personal Config Edits

## Path Resolution

Resolve personal targets first:

- Config: `~/.config/vibar/config.jsonc`
- Style: `~/.config/vibar/style.css`

If the user says "config", "my config", or "style.css" without an explicit path, map to these personal files.

## Reference Source

Use `/home/main/repos/vibar/docs/modules.md` as the canonical module configuration and CSS-class reference when editing `~/.config/vibar/config.jsonc` or `~/.config/vibar/style.css`.

## Guardrails

Do not edit repository defaults unless the user explicitly asks for project/example files:

- `/home/main/repos/vibar/config.jsonc`
- `/home/main/repos/vibar/style.css`

When a request is ambiguous, assume personal-file intent and state the exact target path before editing.

## Edit Workflow

1. Read the resolved personal file.
2. Consult `/home/main/repos/vibar/docs/modules.md` for module fields/classes when behavior depends on module semantics.
3. Apply the smallest possible change for the request.
4. Preserve existing formatting style (including JSONC comments in `config.jsonc`).
5. If the file is missing, create it only when needed to satisfy the request.
6. If the path is outside writable sandbox permissions, request escalation before writing.

## Response Requirements

- Report the absolute path(s) edited.
- Summarize behavior/style changes made.
- Mention validation run (or explicitly state if none was run).
