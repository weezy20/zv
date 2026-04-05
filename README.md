<h1 align="center">
zv (zig version)
</h1>

<p align="center">
  <img src="logo/zv.png" alt="zv logo" width="400" />
</p>

`zv` is a blazing-fast, cross-platform, simple-to-use compiler toolchain manager for Zig, written in Rust. `zv` aims to be a fast and dependable manager for Zig and ZLS, both system-wide and project-specific, with either inline syntax `zig +<version>` or by using a `.zigversion` file in your project root. A `version` can be a semver, `master`, or `latest` for the latest stable. These values are fetched from the cached Ziglang download index available at <a href="https://ziglang.org/download/index.json">Zig official index</a>.

With `zv` binaries, you can now build with the master branch without changing your default system Zig version:
```sh
zig +master build
```
Caching is built-in, so you never have to wait any longer than strictly necessary.

You can also specify a version number, e.g., `zig +0.15.1`. With `zv`, you also have the option of pinning per-project Zig versions using a `.zigversion` file, which takes the same format as `+` would for inline Zig commands. You can have a project with these contents:

```py
# file .zigversion
0.15.2
```
which will always use version `0.15.1` when you run any `zig` command inside it. How cool is that?

It also doubles as a project template starter, providing multiple variants of a Zig project, from a barebones template with a very trimmed-down `build.zig` and `main.zig` file, or the standard Zig project template. Find out more with `zv init --help`.

`zv` uses randomized ranked community mirrors for downloads (can be overridden to use ziglang.org with -f), as that's the official recommendation, with `minisign` and `shasum` verification done before any toolchain is installed. Future versions should bring in an optimization to rank the mirrors based on speed so that faster mirrors are selected more often without user intervention.

## Upgrading from v0.9.x — Breaking Changes (Linux / macOS)

> **Windows users**: nothing changes for you. Skip this section.

This release adopts the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html) on Linux and macOS. Files are now split across purpose-appropriate directories instead of living under a single `~/.zv` root:

| Role | Old path | New path |
|------|----------|----------|
| Data (binaries, versions) | `~/.zv/` | `~/.local/share/zv/` |
| Config (`zv.toml`) | `~/.zv/zv.toml` | `~/.config/zv/zv.toml` |
| Cache (index, mirrors, downloads) | `~/.zv/` | `~/.cache/zv/` |
| Public bin (symlinks) | `~/.zv/bin/` in PATH | `~/.local/bin/` (usually already in PATH) |

### Who is affected

Any Linux or macOS user who:
- installed zv **before this release**, **and**
- does **not** have `ZV_DIR` set in their environment.

**If you have `ZV_DIR` set, you are not affected.** `ZV_DIR` still acts as a self-contained root for all paths — identical to the old behaviour.

### What breaks if you do nothing

- zv no longer sees your installed Zig versions — they are still at `~/.zv/versions/` but zv now looks in `~/.local/share/zv/versions/`.
- Your active Zig selection (`~/.zv/zv.toml`) is invisible to the new config path.
- The welcome message says **"Setup incomplete. Run `zv sync`"** even though your old install works fine.
- `zv sync` / `zv use` / `zv install` operate on the new empty state, not your old one.
- Your shell RC still sources `~/.zv/env`, keeping the old `~/.zv/bin` in `PATH` — so `zig` keeps working via the old shim, but zv commands increasingly diverge from it.
- Running `zv setup` creates a second installation alongside the old one.

### Option A — Migrate to XDG (recommended)

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
cp ~/.zv/mirrors.toml ~/.cache/zv/mirrors.toml 2>/dev/null || true
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

### Option B — Keep the old layout (zero disruption)

Add `ZV_DIR` to your shell profile to restore the pre-XDG behaviour:

```sh
# Add to ~/.bashrc, ~/.zshenv, ~/.zprofile, or equivalent
export ZV_DIR="$HOME/.zv"
```

When `ZV_DIR` is set, zv uses it as a self-contained root for **all** paths (data, config, cache) and does not apply XDG splitting. Everything works exactly as before — no files need to move.

### Why no automatic migration?

Silently moving files and re-patching shell configs without user consent is worse than doing nothing. `ZV_DIR` is a zero-effort escape hatch for users who don't want to migrate right now. Automatic migration will be added in a follow-up release once the XDG layout has stabilised.

### Symptom reference

| Symptom | Cause | Fix |
|---------|-------|-----|
| "Setup incomplete. Run `zv sync`" on startup | `~/.local/share/zv/bin/zv` does not exist yet | Run Option A or set `ZV_DIR` |
| `zig` works but `zv list` shows nothing | Old `~/.zv/bin` is still in `PATH`; new state is empty | Same |
| `zv setup` creates a second empty installation | New paths don't see `~/.zv` | Same |
| `zv` shows "not in PATH" despite `zig` working | `source_set` now checks `~/.local/bin`, not `~/.zv/bin` | Run `zv setup` after migration, or set `ZV_DIR` |
| `~/.zv` still on disk after upgrading | No automatic cleanup | Safe to delete once migrated |

---

## Installation

There are two primary paths: fetch a prebuilt binary or build from source. Either way, finish with `zv sync` — that's all you need. `zv setup` is a legacy fallback covered at the end.

### Option 1 — Prebuilt binary (Recommended)

**Linux/macOS:**
```sh
curl -fsSL https://github.com/weezy20/zv/releases/latest/download/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://github.com/weezy20/zv/releases/latest/download/install.ps1 | iex
```

If you get an execution policy error on Windows:
```powershell
powershell -ExecutionPolicy Bypass -Command "irm https://github.com/weezy20/zv/releases/latest/download/install.ps1 | iex"
```

The script downloads the release binary, places it at `~/.local/share/zv/bin/zv`, and creates a symlink at `~/.local/bin/zv` (Linux/macOS) or installs to `%USERPROFILE%\.zv\bin\zv.exe` (Windows). Then finish with:

```sh
zv sync
```

### Option 2 — Build from source

```sh
# Clone and build
cargo build --release

# Sync using the freshly built binary
./target/release/zv sync
```

Or via `cargo install`:
```sh
cargo install zv
~/.cargo/bin/zv sync
cargo uninstall zv   # optional: clean up the cargo copy; the live binary is now in ZV_DIR/bin
```

**HomeBrew / NPM / Bun:**
```sh
brew install weezy20/tap/zv

npm install -g @weezy20/zv
# or
bun i -g @weezy20/zv
```

---

### `zv setup` — legacy PATH configuration (last resort)

On **Linux**, `zv setup` is a no-op — run `zv sync` instead.

On **macOS** (non-XDG layout) or **Windows**, if `zv sync` warns that your PATH isn't configured, run `zv setup`. It will make the following changes to your system:

- **Creates** a shell environment file at `$ZV_DIR/env`
- **Appends** a `source $ZV_DIR/env` line to your shell RC file (e.g. `~/.bashrc`, `~/.zshrc`, `~/.zprofile`)

To preview these changes without applying them:
```sh
zv setup --dry-run
```

## Updating `zv` 

**Simple update (Recommended):**
```sh
zv update # Installs latest stable release on supported targets*.
```

This command checks for new releases on GitHub and updates `zv` in place. It works whether you're running from `ZV_DIR/bin/zv` or from an external location (like cargo install). 

**Options:**
```sh
zv update --force  # Force reinstall even if already on the latest version
zv upgrade         # Alias for update
zv update --rc     # Update to latest pre-release (release candidate) version
```



<details>
<summary>Alternative update methods:</summary>

If you have the repo cloned or are using a cargo-installed binary:
```sh
# Build new version and update the binary in ZV_DIR/bin
cargo run --release -- sync

# Or if you installed from crates.io, install it and sync
cargo install zv --force
zv sync
# or if you already have ZV_DIR/bin in path
~/.cargo/bin/zv sync # $CARGO_HOME/bin/zv sync
```
</details>

## Usage

All `zv` data follows the XDG Base Directory Specification on Linux/macOS:

**Default locations (Linux/macOS):**
- Data (binaries, versions): `$HOME/.local/share/zv`
- Config (`zv.toml`): `$HOME/.config/zv`
- Cache (index, mirrors, downloads): `$HOME/.cache/zv`
- Public bin (symlinks): `$HOME/.local/bin`

**Default locations (Windows):**
- All data: `%USERPROFILE%\.zv`

You can override the data directory by setting the `ZV_DIR` environment variable (falls back to pre-XDG self-contained layout).

## Use `zv` for project creation:

```sh
zv init [project_name]                 # Create a new Zig project with a name
zv init                                # Create a new Zig project in the current working directory
zv init --zig | -z                     # Create a new Zig project using the standard template provided by `zig init`
# Create a zig project with build.zig.zon:
zv init -p | --zon | --package  <?name>      # Create a zig project in current directory with build.zig.zon or with name if provided
# Note: this requires an active zig version >= 0.12.0 where build.zig.zon support was introduced

```
>Note: `zv init` will use the `build.zig` that's present in [templates/build.zig](templates/lean_build.zig) which is checked to work against minimum zig version specified in [templates/.zigversion](templates/.zigversion). If you want to use a different zig version, set it as active zig and use `zv init -z` or `zig init` directly.

## Use `zv` as your Zig compiler toolchain manager:

```sh
# Version selection - basic usage
# pass in -f or --force-ziglang to download using `ziglang.org` instead of community mirrors (default & recommended)
zv use <version | master | stable | latest> # Select a Zig version to use. Can be a semver, master (branch)
zv use 0.13.0                               # Use a specific semantic version
zv use 0.14 -f                              # Use a version (auto-completes to 0.14.0) & downloads from `ziglang.org` due to -f
zv use master                               # Use master branch build (queries network to find the latest master build)
zv use stable                               # Use latest stable release (refers to cached index)
zv use latest                               # Use latest stable release (queries network to fetch the latest stable)
zv install <version,*> [-f ]                # Install one or more Zig versions without switching to it. Use -f to download from ziglang.org instead of community mirrors.
zv i 0.15.1,0.14.0,master                   # Install multiple versions at once using a comma-separated list

# Per-project Zig config
zig +<version> [...zig args]            # Run Zig using a specific <version> (fetches and downloads version if not present locally)
zig +master [...zig args]               # Run Zig using master build. (If already cached, no download, but a network request is made to verify version)
zig [...zig args]                       # Uses current configured Zig or prefers version from `.zigversion` file in the repository adjacent to `build.zig`.

# Management commands
zv list  | ls                          # List installed Zig versions
zv clean | rm                          # Remove Zig versions interactively. Additionally cleans up downloads cache, temporary download artifacts.
zv clean | rm <version | all>          # Clean up all zv-managed installations using `all` or just a single one (e.g., zv clean 0.14).
zv clean 0.14,0.14.0                   # Clean up multiple Zig installations using a comma-separated list.
zv clean --except <version,*>          # Clean up every version except the version mentioned as argument to --except <version> where <version> maybe a comma separated list of ZigVersions. E.g. (zv clean --except 0.14.1,master@0.16.0-dev.565+f50c64797,stable@0.15.1)
zv rm master                           # Clean up the `master` branch toolchain.
zv rm master --outdated                # Clean up any older master versions in the master folder that don't match latest `master`
zv setup                               # Set up shell environment for zv with interactive prompts (use --no-interactive for automation)
zv sync                                # Resync community mirrors list from [ziglang.org/download/community-mirrors.txt]; also force resync of index to fetch latest nightly builds. Replaces the zv binary in data dir if outdated against current invocation.
zv upgrade | update                    # Update zv to the latest release only if present in GH Releases: https://github.com/weezy20/zv/releases
zv help                                # Detailed instructions for zv. Use `--help` for long help or `-h` for short help with a subcommand.
zv uninstall                           # Uninstall zv completely by attempting to remove ZV_DIR.
```

`minisign` verification is done using [jedisct1/rust-minisign-verify](https://github.com/jedisct1/rust-minisign-verify) — a small minisign library in pure Rust.

It also supports `NO_COLOR` for non-TTY environments.

I hope you enjoy using it! ♥


---
### Customizing ZV behaviour:

### 🔧 Environment Variables for customizing zv

| Variable                  | Description                                                                                                                | Default / Notes                                                                 |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| **`ZV_LOG`**              | Sets the log level (same as `RUST_LOG`). If set, logging follows the specified level.                                      | Inherits `RUST_LOG` behavior                                                    |
| **`ZV_DIR`**              | Overrides the data directory for `zv`. When set, all paths (data, config, cache) live under this directory.               | Linux/macOS: `$XDG_DATA_HOME/zv` (default `$HOME/.local/share/zv`). Windows: `%USERPROFILE%\.zv`                                      |
| **`ZV_INDEX_TTL_DAYS`**   | Number of days between automatic [index](https://ziglang.org/download/index.json) syncs.                                   | **21 days** — Using `master` or `latest` in inline mode use a shorter cache duration of just 1 day unlike `use` which will always fetch `master` & `latest` from network, so practically, you never have to worry about setting this variable yourself. |
| **`ZV_MIRRORS_TTL_DAYS`** | Number of days before refreshing the mirrors list. Broken mirrors degrade automatically. Use `zv sync` to force refresh. | **21 days** — mirrors and index can be resynced immediately with `zv sync`. `master` relies on latest builds & so does `latest` and some community mirrors may not have it available; `zv` will retry other mirrors in that case.      |
| **`ZV_MAX_RETRIES`**      | Maximum number of retry attempts for downloads when a download fails.                                                      | **3 retries** — If a download fails, `zv` will retry up to this many times before giving up.                                                   |
| **`NO_COLOR`**            | If set, disables color output in all zv commands.                                                                          | No color output; useful for non-TTY environments or scripts.                    |
|**`ZV_FETCH_TIMEOUT_SECS`**   | Request timeout to use for network operations requiring fetching index/mirrors list from `ziglang.org`.                | Default 4 seconds for most operations.

---

### Tips:
- If you prefer some mirrors to others, you can put it as `rank = 1` on your preferred mirrors (Default is rank 1 for all mirrors) or lower the rank of mirrors that you don't want. `rank` is a range from 1..255, lower is better and more preferred when doing random selection. The mirrors file is generated at `$XDG_CACHE_HOME/zv/mirrors.toml` (default `~/.cache/zv/mirrors.toml`)

- Currently `zv use master` will only install the master as present in zig-index. This means that older master installations still remain under the masters folder and can be selected via `zv use master@<older master version>` which can be obtained via `zv ls`. Note, installing older master versions like this may work now (zv v0.6.0 onwards): `zv i <pre-release version>` or `zv use <pre-release version>` if some mirror has the build, it'll be fetched.
