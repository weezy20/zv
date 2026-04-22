# Upgrading from v0.9.x — Breaking Changes (Linux / macOS)

> **Windows users**: nothing changes for you. Skip this page.

This release adopts the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html) on Linux and macOS. Files are now split across purpose-appropriate directories instead of living under a single `~/.zv` root:

| Role | Old path | New path |
|------|----------|----------|
| Data (binaries, versions) | `~/.zv/` | `~/.local/share/zv/` |
| Config (`zv.toml`) | `~/.zv/zv.toml` | `~/.config/zv/zv.toml` |
| Cache (index, mirrors, downloads) | `~/.zv/` | `~/.cache/zv/` |
| Public bin (symlinks) | `~/.zv/bin/` in PATH | `~/.local/bin/` (usually already in PATH) |

## Who is affected

Any Linux or macOS user who:
- installed zv **before this release**, **and**
- does **not** have `ZV_DIR` set in their environment.

**If you have `ZV_DIR` set, you are not affected.** `ZV_DIR` still acts as a self-contained root for all paths — identical to the old behaviour.

## What breaks if you do nothing

- zv no longer sees your installed Zig versions — they are still at `~/.zv/versions/` but zv now looks in `~/.local/share/zv/versions/`.
- Your active Zig selection (`~/.zv/zv.toml`) is invisible to the new config path.
- The welcome message says **"Setup incomplete. Run `zv sync`"** even though your old install works fine.
- `zv sync` / `zv use` / `zv install` operate on the new empty state, not your old one.
- Your shell RC still sources `~/.zv/env`, keeping the old `~/.zv/bin` in `PATH` — so `zig` keeps working via the old shim, but zv commands increasingly diverge from it.
- Running `zv setup` creates a second installation alongside the old one.

## Option A — Migrate to XDG (recommended)

Run the following once after upgrading:

```sh
# 1. Create new XDG directories
mkdir -p ~/.local/share/zv ~/.config/zv ~/.cache/zv

# 2. Move installed Zig versions (the bulk of the data)
mv ~/.zv/versions ~/.local/share/zv/versions

# 3. Copy config
cp ~/.zv/zv.toml ~/.config/zv/zv.toml 2>/dev/null || true

# 4. Copy cache files (zv will re-fetch these if missing, but saves a sync)
cp ~/.zv/index.toml   ~/.cache/zv/index.toml   2>/dev/null || true
cp ~/.zv/mirrors.toml ~/.cache/zv/mirrors.toml  2>/dev/null || true
cp ~/.zv/master       ~/.cache/zv/master        2>/dev/null || true

# 5. Re-run zv sync to place the binary and create ~/.local/bin symlinks
zv sync

# 6. Remove the old source line from your shell RC
#    Look for:  source ~/.zv/env   or   . ~/.zv/env
#    Delete or comment it out, then open a new shell.

# 7. Verify everything looks right
zv          # should show "✔ Ready to Use"
zv list     # should show your previously installed versions

# 8. (Optional) Remove the old directory once satisfied
rm -rf ~/.zv
```

## Option B — Keep the old layout (zero disruption)

Add `ZV_DIR` to your shell profile to restore the pre-XDG behaviour:

```sh
# Add to ~/.bashrc, ~/.zshenv, ~/.zprofile, or equivalent
export ZV_DIR="$HOME/.zv"
```

When `ZV_DIR` is set, zv uses it as a self-contained root for **all** paths (data, config, cache) and does not apply XDG splitting. Everything works exactly as before — no files need to move.

## Why no automatic migration?

Silently moving files and re-patching shell configs without user consent is worse than doing nothing. `ZV_DIR` is a zero-effort escape hatch for users who don't want to migrate right now. Automatic migration will be added in a follow-up release once the XDG layout has stabilised.

## Symptom reference

| Symptom | Cause | Fix |
|---------|-------|-----|
| "Setup incomplete. Run `zv sync`" on startup | `~/.local/share/zv/bin/zv` does not exist yet | Run Option A or set `ZV_DIR` |
| `zig` works but `zv list` shows nothing | Old `~/.zv/bin` is still in `PATH`; new state is empty | Same |
| `zv setup` creates a second empty installation | New paths don't see `~/.zv` | Same |
| `zv` shows "not in PATH" despite `zig` working | `source_set` now checks `~/.local/bin`, not `~/.zv/bin` | Run `zv setup` after migration, or set `ZV_DIR` |
| `~/.zv` still on disk after upgrading | No automatic cleanup | Safe to delete once migrated |
