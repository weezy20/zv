# zv (zig version) manager

`zv` is a blazing-fast, cross-platform, simple-to-use compiler toolchain manager for Zig, written in Rust. `zv` aims to be a fast and dependable manager for Zig and ZLS, both system-wide and project-specific, with either inline syntax `zig +<version>` or by using a `.zigversion` file in your project root. A `version` can be a semver, `master`, or `latest` for the latest stable. These values are fetched from the cached Ziglang download index available at <a href="https://ziglang.org/download/index.json">Zig official index</a>.

With `zv` binaries, you can now build with the master branch without changing your default system Zig version:
```sh
zig +master build
```
Caching is built-in, so you never have to wait any longer than strictly necessary.

You can also specify a version number, e.g., `zig +0.15.1`. With `zv`, you also have the option of pinning per-project Zig versions using a `.zigversion` file, which takes the same format as `+` would for inline Zig commands. You can have a project with these contents:

```py
# file .zigversion
0.15.1
```
which will always use version `0.15.1` when you run any `zig` command inside it. How cool is that?

It also doubles as a project template starter, providing multiple variants of a Zig project, from a barebones template with a very trimmed-down `build.zig` and `main.zig` file, or the standard Zig project template. Find out more with `zv init --help`.

`zv` prefers community mirrors for downloads, as that's the official recommendation, with `minisign` and `shasum` verification done before any toolchain is installed.

## Installation


**Quick install script (Recommended):**

Linux/macOS:
```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/weezy20/zv/releases/latest/download/zv-installer.sh | sh
```

Windows (PowerShell):
```powershell
irm https://github.com/weezy20/zv/releases/latest/download/zv-installer.ps1 | iex
```

**HomeBrew:**
```sh
brew install weezy20/tap/zv
```

**NPM:**
```sh
npm install -g @weezy20/zv
```

<details>
<summary><b>Cargo (crates.io or build from source)</b></summary>

**Step 1:** Install the binary from crates.io or build from source

Using `cargo install`:
```sh
# From crates.io
cargo install zv

# OR from GitHub
cargo install --git https://github.com/weezy20/zv --locked
```

Or build from source (clone the repo first):
```sh
# Build release binary (creates target/release/zv)
cargo build --release
```

**Step 2:** Preview setup changes (optional but recommended)

Using `cargo installed binary`:
```sh
$HOME/.cargo/bin/zv setup --dry-run
# OR shorthand:
$HOME/.cargo/bin/zv setup -d
```

Or from source:
```sh
cargo run --release -- setup --dry-run
# OR shorthand:
cargo run --release -- setup -d
```

**Step 3:** Run setup to install `zv` to `ZV_DIR/bin` and configure your shell

Using `cargo installed binary`:
```sh
$HOME/.cargo/bin/zv setup
```

Or from source:
```sh
cargo run --release -- setup
# or if you already have ZV_DIR/bin in path
cargo run --release -- sync
```

This self-installs `zv` to `ZV_DIR/bin` (default: `$HOME/.zv/bin` on Unix, `%USERPROFILE%\.zv\bin` on Windows) and adds it to your PATH.

**Step 4:** Remove the cargo binary (optional cleanup - only if you used `cargo install`)
```sh
cargo uninstall zv
```

From now on, use the `zv` installed in `ZV_DIR/bin`.

</details>

---

> **Note:** Run `zv setup` after installation to self-install `zv` to `ZV_DIR/bin` (default: `$HOME/.zv/bin` on Unix, `%USERPROFILE%\.zv\bin` on Windows).

## Updating `zv` 

If you have the repo cloned or are using cargo-installed binary:
```sh
# Builds new version and simultaneously runs setup to update the binary in ZV_DIR/bin
cargo run --release -- setup
# Or you can also use sync
cargo run --release -- sync # Recommended
```
If you have the quick install script you should have a `zv-update` command available:
```sh
zv-update # fetches latest release and puts it in the default location for the method you used above
```
If you used `zv-update` your `ZV_DIR/bin/zv` might still be on the older version. Just run `zv setup` or `zv sync` with the newer `bin` to update the binary in `ZV_DIR/bin`. 
This replaces your existing `ZV_DIR/bin/zv` installation. This is not strictly necessary but recommeneded to keep your `ZV_DIR/bin/zv` binary up to date.

## Usage

All `zv` data lives in `ZV_DIR`, including Zig versions, downloads, cache files, and the `zv` binary itself.

**Default locations:**
- Unix/Linux/macOS: `$HOME/.zv`
- Windows: `%USERPROFILE%\.zv`

You can customize this by setting the `ZV_DIR` environment variable.

## Use `zv` for project creation:

```sh
zv init [project_name]                 # Create a new Zig project with a name
zv init                                # Create a new Zig project in the current working directory
zv init --zig | -z                     # Create a new Zig project using the standard template provided by `zig init`
```

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
zv sync                                # Resync community mirrors list from [ziglang.org/download/community-mirrors.txt]; also force resync of index to fetch latest nightly builds.
zv help                                # Detailed instructions for zv. Use `--help` for long help or `-h` for short help with a subcommand.
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
| **`ZV_DIR`**              | Defines the home directory for `zv`.                                                                                       | Default `$HOME/.zv` for linux/macos or unix. For windows it usually will be `%USERPROFILE%/.zv`                                      |
| **`ZV_INDEX_TTL_DAYS`**   | Number of days between automatic [index](https://ziglang.org/download/index.json) syncs.                                   | **21 days** — Using `master` or `latest` in inline mode use a shorter cache duration of just 1 day unlike `use` which will always fetch `master` & `latest` from network, so practically, you never have to worry about setting this variable yourself. |
| **`ZV_MIRRORS_TTL_DAYS`** | Number of days before refreshing the mirrors list. Broken mirrors degrade automatically. Use `zv sync` to force refresh. | **21 days** — mirrors and index can be resynced immediately with `zv sync`. `master` relies on latest builds & so does `latest` and some community mirrors may not have it available; `zv` will retry other mirrors in that case.      |
| **`NO_COLOR`**            | If set, disables color output in all zv commands.                                                                          | No color output; useful for non-TTY environments or scripts.                    |
|**`ZV_FETCH_TIMEOUT_SECS`**   | Request timeout to use for network operations requiring fetching index/mirrors list from `ziglang.org`.                | Default 15 seconds for most operations.

---

### Tips:
- If you prefer some mirrors to others, you can put it as `rank = 1` on your preferred mirrors (Default is rank 1 for all mirros) or lower the rank of mirrors that you don't want. `rank` is a range from 1..255, lower is better and more preferred when doing random selection. The mirrors file is generated at `<ZV_DIR>/mirrors.toml`

- Currently `zv use master` will only install the master as present in zig-index. This means that older master installations still remain under the masters folder and can be selected via `zv use master@<older master version>` which can be obtained via `zv ls`. Note, installing older master versions like this doesn't work because for `master` we exclusively consult the information in the zig-index. However, there's no reason this can't be supported in the future with a separate `dev@<version>` syntax.