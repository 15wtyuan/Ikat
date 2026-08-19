---
name: loom
description: Build, validate, or scaffold a LoomGUI UI workspace — any check / build / init / new / list / show / font add / atlas add need. Use BEFORE writing HTML/CSS in a LoomGUI workspace (fence rules live in the loomgui-editor skill) and AFTER editing to validate.
---

# loom — LoomGUI workspace CLI

`loom` is the single entry point for validating, building, and configuring a LoomGUI UI workspace. This workspace ships with the CLI at `.loom/loom(.exe)` — it needs no LoomGUI repository checkout.

## Locating the binary (in order)

1. `.loom/loom` (or `.loom/loom.exe`) inside the workspace — the bundled copy.
2. `loom` on PATH.
3. Inside a LoomGUI repository checkout: `cargo run -p loomgui_pkg -- <subcommand> ...`.

If none exists, download `loom.exe` from the LoomGUI GitHub Release whose tag matches the installed Unity package version.

## Commands

```
loom check  [<dir>] [--format human|json]     # validate: fence + registry + asset coverage; writes NOTHING
loom build  [<dir>] [--format human|json]     # check + write artifacts into output_dir
loom init   <dir> [--agent claude|agents]... [--unity-root <path>] [--output <dir>] [--force]
loom new    <name>                            # create ui/<name>/main.html + register the package
loom list   pkg|atlas|font [--format json]    # summary per entity (one line each)
loom show   <pkg> [--format json]             # one package's pages + custom components
loom font add <file> --family <f> [--default] [--fallback]
loom atlas add <dir> [--name <n>] [--max-size <n>] [--padding <n>] [--standalone]
loom scaffold [--agent claude|agents]...              # refresh agent docs + skills only (safe on existing workspaces)
loom version [--format json]
```

`new` / `list` / `show` / `font add` / `atlas add` / `scaffold` run in the current directory (the workspace root). `check` / `build` / `init` take a directory (default: current).

**Editing `loom.workspace.json` always goes through these commands — never hand-edit it.** The workspace holds an entire game's UI (hundreds of packages, thousands of pages/images); hand edits are how configurations break silently.

## Exit codes (frozen contract)

| Code | Meaning |
|---|---|
| 0 | Clean. Warnings do NOT fail the run. |
| 1 | Errors found (check/build diagnostics), or a write command conflict (duplicate name, overlapping atlas dir). The output contains everything needed to fix it. |
| 2 | Usage / config / io failure (bad args, unreadable workspace.json, missing unity_root target, io errors). |

## Machine output (`--format json`)

`check` / `build` print ONE JSON document to **stdout** (progress goes to stderr). Same for list/show/write-command results. Top-level shape:

```jsonc
{
  "command": "check",
  "format_version": 1,           // versioned; fields only get added, never changed
  "success": false,
  "summary": { "errors": 3, "warnings": 1 },
  "message": "3 error(s) in workspace",   // present on failure
  "diagnostics": [
    {
      "severity": "error",       // "error" | "warning"
      "code": "FenceUnknownTag", // fence code, or a packer code (see below)
      "component": "main",
      "file": "ui/battle/main.html",
      "line": 12, "column": 3,
      "message": "tag `p` is not in the fence",
      "help": "..."              // nullable
    }
  ],
  "report": { ... }              // build artifacts summary; build success only
}
```

Packer-synthesized codes you will see: `DuplicateComponentName`, `ComponentNameRequiresHyphen`, `ComponentMultipleRoots`, `ComponentKeyframesNameCollision` (warning), `FontFileMissing`, `SpriteMissingFromAtlas`, `SpriteAtlasConflict`, `AtlasImageOverflow`, `PackError`.

Diagnostics are **collect-all**: every error in every file is reported in one run. Fix them ALL in one editing pass, then re-run `check`.

## The authoring loop

1. `loom list pkg` — see what exists. `loom show <pkg>` for one package's pages/components. Never bulk-scan the workspace yourself.
2. New UI package: `loom new <name>` — creates a minimal legal page, check passes immediately.
3. **No fonts yet? Ask the user for a font file first** (rendering needs one), then `loom font add <file> --family <name> --default`.
4. Put PNG assets under a directory (e.g. `assets/icons/`), then `loom atlas add assets/icons`. An `<img src>` is covered when its file lives under some atlas dir — the packer cross-validates.
5. Write HTML/CSS per the loomgui-editor skill (the fence rulebook).
6. `loom check --format json` → fix ALL diagnostics in one pass → repeat until exit 0.
7. Ask the user whether to publish; on approval run `loom build --format json` and report the artifact paths from `report`.

Warnings do not block the build, but handle them: W1/W2 (`FenceBorderWithoutStyle`, `FenceBgImageWithoutSize`) mean the browser preview will NOT match the runtime.

## Output location (how build reaches Unity)

`output_dir` in loom.workspace.json is resolved against a base: if `.loom/unity.json` exists (written by the GUI packer on workspace creation), artifacts go to `<unity_root>/<output_dir>` — typically straight into the Unity project's `Assets/Bundles`. Without it, output stays local under the workspace. If build fails with exit 2 mentioning unity_root, the Unity project moved — reopen the GUI packer to rebind, or delete `.loom/unity.json` to output locally.

`stdout` carries data; `stderr` carries progress and human rendering. Parse stdout JSON; never scrape stderr.
