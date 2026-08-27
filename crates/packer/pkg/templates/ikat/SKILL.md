---
name: ikat
description: |
  Operate the ikat CLI — build, validate, or configure a Ikat UI
  workspace. Use BEFORE writing HTML/CSS in a Ikat workspace (run
  `ikat list` / `ikat show` instead of scanning files), and AFTER edits
  to validate (`ikat check`) and package (`ikat build`). Also for
  workspace configuration and version sync: init, upgrade, new package,
  font add, atlas add.
  Fence authoring rules live in the ikat-editor skill.
---

# ikat — Ikat workspace CLI

`ikat` is the single entry point for validating, building, and
configuring a Ikat UI workspace.

## Workspace topology

```
<session root>/                ← open ONE agent session here
  .ikat/                       ← committed to git (team shares the CLI)
    ikat(.exe)                 ← bundled CLI, version-matched to the release
    config.json                ← { "ui_root": "ui", "unity_root": "unity" }
  .agents/skills/              ← ikat-editor / ikat-runtime / ikat-preview / ikat
  ui/                          ← the UI workspace (ui_root's target)
    ikat.workspace.json        ← build configuration (edit via commands only)
    ui/ assets/ fonts/ ...
```

`.ikat/config.json` pointers are relative to the session root
(`ui_root: "."` = single-directory workspace). Every command accepts the
session root, the ui workspace itself, or a direct child of it — from
the session root, plain `ikat check` with no argument works.

`unity_root` (when present) is the output base: `output_dir` resolves
against the Unity project (writing `Assets/Bundles` lands straight in
Assets). Without it, outputs stay local under the ui workspace. A broken
pointer fails with exit 2 — never a silent local fallback.

## Locating the binary (in order)

1. `.ikat/ikat` (or `.ikat/ikat.exe`) at the session root — the bundled copy.
2. `ikat` on PATH.
3. Inside a Ikat repository checkout: `cargo run -p ikat_pkg -- <subcommand> ...`.

## Commands

```
ikat check  [<dir>] [--format human|json]     validate: fence + registry + asset coverage; writes NOTHING
ikat build  [<dir>] [--format human|json]     check + write artifacts into output_dir
ikat init   <dir> [--ui <dir>] [--agent claude|agents]... [--unity-root <path>] [--output <dir>] [--force]
ikat new    <name>                            create ui/<name>/main.html + register the package
ikat list   pkg|atlas|font [--format json]    summary per entity (one line each)
ikat show   <pkg> [--format json]             one package's pages + custom components
ikat font add <file> --family <f> [--default] [--fallback]
ikat atlas add <dir> [--name <n>] [--max-size <n>] [--padding <n>] [--standalone]
ikat design [WxH] [--match letterbox|fit-width|fit-height] [--clear]
ikat scaffold [--agent claude|agents]...      refresh workspace generated artifacts (skills + .ikat CLI + version stamp)
ikat preview [<dir>] [--port <n>] [--idle-timeout <s>] [--stop]
ikat version [--format json]
```

`<dir>` (and the cwd for the remaining commands) resolves through
config discovery (see Workspace topology). **Editing
`ikat.workspace.json` always goes through these commands — never
hand-edit it.** The workspace holds an entire game's UI; hand edits are
how configurations break silently.

## `ikat preview` — the human preview workbench

Long-running local server for HUMANS to review pages in a browser
before anything reaches the engine. Prints ONE JSON document with `url`
to stdout, then keeps serving (access log on stderr). The workbench:
package/page tree on the left, preview on the right at the design
resolution under `match_mode` scaling (never reflows — preview must
predict the runtime), device-frame switching with safe-area guides,
settings in browser localStorage. It auto-injects the workspace's
the framework behavior boot (`/ikat-preview/lib/boot.js`, always) plus
workspace consumer scripts (`<pkg-dir>/preview/main.js` + the
matching `preview/pages/<page>.js`) when they exist — sources stay clean
and the `preview/` dir never packs. Consumer simulation scripts are the
`ikat-preview` skill's business.

Lifecycle (absorbed from real-world server pain): stable port per
workspace (path-hash in 41000–41999 — an open tab survives restarts,
just refresh); server info persisted to `.ikat/preview.json` (find the
URL there if stdout got swallowed by backgrounding); reuse — a second
`ikat preview` on the same workspace reports the running instance's URL
and exits; auto-exit after 4h idle (override with `--idle-timeout <s>`)
and when the owning shell dies; `ikat preview --stop` shuts the instance
down (verifies ownership via a token before killing). Binds 127.0.0.1
only.

This is NOT the rejected `watch`/`mcp` idea: no file watching, no auto
rebuild — the human refreshes; sources are read live on every request.

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

1. `ikat list pkg` — see what exists. `ikat show <pkg>` for one
   package's pages/components. Never bulk-scan the workspace yourself.
2. New UI package: `ikat new <name>` — creates a minimal legal page,
   check passes immediately.
3. **No fonts yet? Ask the user for a font file first** (rendering needs
   one), then `ikat font add <file> --family <name> --default`.
4. Put PNG assets under a directory (e.g. `assets/icons/`), then
   `ikat atlas add assets/icons`. An `<img src>` is covered when its file
   lives under some atlas dir — the packer cross-validates.
5. Write HTML/CSS per the ikat-editor skill (the fence rulebook).
6. `ikat check --format json` → fix ALL diagnostics in one pass → repeat
   until exit 0.
7. Ask the user whether to publish; on approval run `ikat build --format
   json` and report the artifact paths from `report`.

Warnings do not block the build, but handle them: W1/W2
(`FenceBorderWithoutStyle`, `FenceBgImageWithoutSize`) mean the browser
preview will NOT match the runtime.

## `ikat init`

```
ikat init <root> [--ui <dir>] [--agent claude|agents]... [--unity-root <path>] [--output <dir>] [--force]
```

- `<root>` — the session root: gets `.ikat/` (CLI copy + config.json)
  and the agent skills directories.
- `--ui <dir>` — where the UI workspace lives (relative to root or
  absolute; default: the root itself — the single-directory form).
- `--unity-root <path>` — recorded as `unity_root` (relativized when
  possible); omit for local-output workspaces. The GUI packer passes it
  automatically when launched from Unity (`Ikat > Open Packer`).
- `--agent` — which skills directories to write (`.claude/skills/`,
  `.agents/skills/`; repeatable, default `agents`).
- Refuses when the target already has a `ikat.workspace.json` unless
  `--force`.

`ikat scaffold` refreshes the workspace's **generated artifacts** at the
session root: the three agent skills, the `.ikat/` CLI copy (self-copy
from whichever ikat.exe you run) and `.ikat/scaffold.version`. It never
touches `config.json`, `ikat.workspace.json`, or sources. After a
package/CLI upgrade, run it once — `ikat check` warns
(`StaleScaffold`) when the version stamp is older than the running
CLI. Without `--agent` it refreshes whichever agent dirs already
exist (`.agents/skills` and/or `.claude/skills`; default `agents` for
a first-time scaffold). For the full upgrade ritual see
[Version sync](#version-sync-workspace--unity-package).

## Version sync (workspace ↔ Unity package)

Two version sources must stay in lockstep; each can only see itself:

- workspace side: `.ikat/scaffold.version` (or `.ikat/ikat.exe version
  --format json` → `cli`)
- Unity side: `<unity_root>/Packages/packages-lock.json` →
  `com.ikat.unity` → `version` (for git-URL installs this is the
  package.json version at the pinned tag)

The bundled `.ikat/ikat.exe` reports ITS OWN version — an old exe
never flags itself, and `StaleScaffold` only compares the stamp
against the *running* CLI. Drift is only visible by comparing the two
sides directly. Do that compare whenever the Unity package version
changes and before builds that follow a Unity-side upgrade.

**Workspace older than Unity** (the common drift — user bumped the
manifest tag):

1. Get the new exe: copy `<unity_root>/Library/PackageCache/
   com.ikat.unity*/Editor/Tools/ikat(.exe)` (glob — the cache dir
   name has a hash suffix), or download
   `https://github.com/15wtyuan/Ikat/releases/download/<tag>/ikat.exe`
   (do NOT use the `/releases/latest` endpoint — 0.x releases are all
   prerelease, that endpoint 404s; use the release list).
2. Overwrite `.ikat/ikat(.exe)` with it.
3. Run `.ikat/ikat(.exe) scaffold` — refreshes skills + version stamp
   (self-copy skips, same path).

Verify: `ikat check` reports no `StaleScaffold`, and the three
versions (manifest tag ↔ lock `version` ↔ `.ikat/scaffold.version`)
agree.

**Workspace newer than Unity**: the exe may emit `.pkg.bin` the old
in-package `.dll` cannot read (`format_version` only grows). Tell the
user to bump the manifest tag to match before you build.

Never "upgrade" via `ikat init --force` — it resets
`ikat.workspace.json` to an empty skeleton and wipes every registered
package.

## `ikat.workspace.json` fields

Written by the commands above; documented here for reading.

### Top-level

| Field | Type | Description |
|---|---|---|
| `version` | u32 | Config format version (currently `1`) |
| `output_dir` | string | Path (relative to the output base — see topology) where build artifacts go |
| `design` | {w, h}? | Design resolution (design px). `ikat build` passes it through to `ikat.runtime.json`; the engine integration layer consumes it as the source of truth for resolution adaptation. Absent = engine-side fallback (Driver Inspector field) |
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
