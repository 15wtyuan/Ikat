---
name: loom
description: |
  Operate the loom CLI — build, validate, or configure a LoomGUI UI
  workspace. Use BEFORE writing HTML/CSS in a LoomGUI workspace (run
  `loom list` / `loom show` instead of scanning files), and AFTER edits
  to validate (`loom check`) and package (`loom build`). Also for
  workspace configuration and version sync: init, upgrade, new package,
  font add, atlas add.
  Fence authoring rules live in the loomgui-editor skill.
---

# loom — LoomGUI workspace CLI

`loom` is the single entry point for validating, building, and
configuring a LoomGUI UI workspace.

## Workspace topology

```
<session root>/                ← open ONE agent session here
  .loom/                       ← committed to git (team shares the CLI)
    loom(.exe)                 ← bundled CLI, version-matched to the release
    config.json                ← { "ui_root": "ui", "unity_root": "unity" }
  .agents/skills/              ← loomgui-editor / loomgui-runtime / loom
  ui/                          ← the UI workspace (ui_root's target)
    loom.workspace.json        ← build configuration (edit via commands only)
    ui/ assets/ fonts/ ...
```

`.loom/config.json` pointers are relative to the session root
(`ui_root: "."` = single-directory workspace). Every command accepts the
session root, the ui workspace itself, or a direct child of it — from
the session root, plain `loom check` with no argument works.

`unity_root` (when present) is the output base: `output_dir` resolves
against the Unity project (writing `Assets/Bundles` lands straight in
Assets). Without it, outputs stay local under the ui workspace. A broken
pointer fails with exit 2 — never a silent local fallback.

## Locating the binary (in order)

1. `.loom/loom` (or `.loom/loom.exe`) at the session root — the bundled copy.
2. `loom` on PATH.
3. Inside a LoomGUI repository checkout: `cargo run -p loomgui_pkg -- <subcommand> ...`.

## Commands

```
loom check  [<dir>] [--format human|json]     validate: fence + registry + asset coverage; writes NOTHING
loom build  [<dir>] [--format human|json]     check + write artifacts into output_dir
loom init   <dir> [--ui <dir>] [--agent claude|agents]... [--unity-root <path>] [--output <dir>] [--force]
loom new    <name>                            create ui/<name>/main.html + register the package
loom list   pkg|atlas|font [--format json]    summary per entity (one line each)
loom show   <pkg> [--format json]             one package's pages + custom components
loom font add <file> --family <f> [--default] [--fallback]
loom atlas add <dir> [--name <n>] [--max-size <n>] [--padding <n>] [--standalone]
loom design [WxH] [--match letterbox|fit-width|fit-height] [--clear]
loom scaffold [--agent claude|agents]...      refresh workspace generated artifacts (skills + .loom CLI + version stamp)
loom version [--format json]
```

`<dir>` (and the cwd for the remaining commands) resolves through
config discovery (see Workspace topology). **Editing
`loom.workspace.json` always goes through these commands — never
hand-edit it.** The workspace holds an entire game's UI; hand edits are
how configurations break silently.

## Exit codes (frozen contract)

| Code | Meaning |
|---|---|
| 0 | Clean. Warnings do NOT fail the run. |
| 1 | Errors found (check/build diagnostics), or a write command conflict (duplicate name, overlapping atlas dir). The output contains everything needed to fix it. |
| 2 | Usage / config / io failure (bad args, unreadable workspace.json, broken config.json pointer, io errors). |

## Machine output (`--format json`)

`check` / `build` print ONE JSON document to **stdout** (progress and
human rendering go to stderr). Same for list/show/write-command results.
Parse stdout; never scrape stderr.

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
      "code": "FenceUnknownTag", // fence code, or a packer code (below)
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

Packer-synthesized codes you will see: `DuplicateComponentName`,
`ComponentNameRequiresHyphen`, `ComponentMultipleRoots`,
`ComponentKeyframesNameCollision` (warning), `FontFileMissing`,
`SpriteMissingFromAtlas`, `SpriteAtlasConflict`, `AtlasImageOverflow`,
`PackError`.

Diagnostics are **collect-all**: every error in every file is reported in
one run. Fix them ALL in one editing pass, then re-run `check`.

## The authoring loop

1. `loom list pkg` — see what exists. `loom show <pkg>` for one
   package's pages/components. Never bulk-scan the workspace yourself.
2. New UI package: `loom new <name>` — creates a minimal legal page,
   check passes immediately.
3. **No fonts yet? Ask the user for a font file first** (rendering needs
   one), then `loom font add <file> --family <name> --default`.
4. Put PNG assets under a directory (e.g. `assets/icons/`), then
   `loom atlas add assets/icons`. An `<img src>` is covered when its file
   lives under some atlas dir — the packer cross-validates.
5. Write HTML/CSS per the loomgui-editor skill (the fence rulebook).
6. `loom check --format json` → fix ALL diagnostics in one pass → repeat
   until exit 0.
7. Ask the user whether to publish; on approval run `loom build --format
   json` and report the artifact paths from `report`.

Warnings do not block the build, but handle them: W1/W2
(`FenceBorderWithoutStyle`, `FenceBgImageWithoutSize`) mean the browser
preview will NOT match the runtime.

## `loom init`

```
loom init <root> [--ui <dir>] [--agent claude|agents]... [--unity-root <path>] [--output <dir>] [--force]
```

- `<root>` — the session root: gets `.loom/` (CLI copy + config.json)
  and the agent skills directories.
- `--ui <dir>` — where the UI workspace lives (relative to root or
  absolute; default: the root itself — the single-directory form).
- `--unity-root <path>` — recorded as `unity_root` (relativized when
  possible); omit for local-output workspaces. The GUI packer passes it
  automatically when launched from Unity (`LoomGUI > Open Packer`).
- `--agent` — which skills directories to write (`.claude/skills/`,
  `.agents/skills/`; repeatable, default `agents`).
- Refuses when the target already has a `loom.workspace.json` unless
  `--force`.

`loom scaffold` refreshes the workspace's **generated artifacts** at the
session root: the three agent skills, the `.loom/` CLI copy (self-copy
from whichever loom.exe you run) and `.loom/scaffold.version`. It never
touches `config.json`, `loom.workspace.json`, or sources. After a
package/CLI upgrade, run it once — `loom check` warns
(`StaleScaffold`) when the version stamp is older than the running
CLI. Without `--agent` it refreshes whichever agent dirs already
exist (`.agents/skills` and/or `.claude/skills`; default `agents` for
a first-time scaffold). For the full upgrade ritual see
[Version sync](#version-sync-workspace--unity-package).

## Version sync (workspace ↔ Unity package)

Two version sources must stay in lockstep; each can only see itself:

- workspace side: `.loom/scaffold.version` (or `.loom/loom.exe version
  --format json` → `cli`)
- Unity side: `<unity_root>/Packages/packages-lock.json` →
  `com.loomgui.unity` → `version` (for git-URL installs this is the
  package.json version at the pinned tag)

The bundled `.loom/loom.exe` reports ITS OWN version — an old exe
never flags itself, and `StaleScaffold` only compares the stamp
against the *running* CLI. Drift is only visible by comparing the two
sides directly. Do that compare whenever the Unity package version
changes and before builds that follow a Unity-side upgrade.

**Workspace older than Unity** (the common drift — user bumped the
manifest tag):

1. Get the new exe: copy `<unity_root>/Library/PackageCache/
   com.loomgui.unity*/Editor/Tools/loom(.exe)` (glob — the cache dir
   name has a hash suffix), or download
   `https://github.com/15wtyuan/LoomGUI/releases/download/<tag>/loom.exe`
   (do NOT use the `/releases/latest` endpoint — 0.x releases are all
   prerelease, that endpoint 404s; use the release list).
2. Overwrite `.loom/loom(.exe)` with it.
3. Run `.loom/loom(.exe) scaffold` — refreshes skills + version stamp
   (self-copy skips, same path).

Verify: `loom check` reports no `StaleScaffold`, and the three
versions (manifest tag ↔ lock `version` ↔ `.loom/scaffold.version`)
agree.

**Workspace newer than Unity**: the exe may emit `.pkg.bin` the old
in-package `.dll` cannot read (`format_version` only grows). Tell the
user to bump the manifest tag to match before you build.

Never "upgrade" via `loom init --force` — it resets
`loom.workspace.json` to an empty skeleton and wipes every registered
package.

## `loom.workspace.json` fields

Written by the commands above; documented here for reading.

### Top-level

| Field | Type | Description |
|---|---|---|
| `version` | u32 | Config format version (currently `1`) |
| `output_dir` | string | Path (relative to the output base — see topology) where build artifacts go |
| `design` | {w, h}? | Design resolution (design px). `loom build` passes it through to `loom.runtime.json`; the engine integration layer consumes it as the source of truth for resolution adaptation. Absent = engine-side fallback (Driver Inspector field) |
| `match_mode` | string? | Adaptation mode: `letterbox` (contain, pillar/letter bars — default) / `fit-width` / `fit-height` (lock one axis to the design, reflow the other — `vw`/`vh`/`%` flow with the canvas). Absent = `letterbox` |

| `packages` | PackageCfg[] | UI packages to build (one `.pkg.bin` per package) |
| `atlases` | AtlasCfg[] | Texture atlases to pack from image sources |
| `fonts` | FontCfg[] | Fonts to include in the runtime bundle |

All paths are relative to the workspace root and use forward slashes.

### `packages[]`

| Field | Type | Description |
|---|---|---|
| `name` | string | Package name (produces `{name}.pkg.bin`) |
| `dirs` | string[] | Directories to collect `.html` files from (relative to workspace root) |
| `html` | string[] | Empty `[]` = auto-scan (packer scans `dirs` top-level `.html` files); non-empty = build only these listed files. Set back to `[]` to restore auto-scan. |

### `atlases[]`

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | (required) | Atlas name (produces `{name}.png` + `{name}.atlas.json`) |
| `default` | bool | `false` | Whether images not explicitly assigned to any atlas go here |
| `standalone` | bool | `false` | Whether each image gets its own page (for very large images) |
| `dirs` | string[] | (required) | Directories to recursively scan for `.png` files |
| `max_size` | u32 | `2048` | Maximum atlas page size in pixels (powers of 2 recommended) |
| `padding` | u32 | `4` | Padding between sprites in pixels (prevents bleed) |

### `fonts[]`

| Field | Type | Default | Description |
|---|---|---|---|
| `family` | string | (required) | Font family name (matches CSS `font-family`) |
| `file` | string | (required) | Path to font file relative to workspace root |
| `default` | bool | `false` | Whether this is the default font (used when no family is specified) |
| `fallback` | bool | `false` | Whether this font is used as a fallback (for missing glyphs) |
