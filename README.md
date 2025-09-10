# zv (zig version) manager

`zv` is a blazing fast, cross-platform, simple-to-use, compiler toolchain manager for zig, written in Rust. `zv` aims to be a fast & dependable manager zig/zls both system-wide & project specific with either inline syntax `zig +<version>` or using a `.zigversion` file in your project root. A `version` could be a semver, `master`, `latest` for latest stable. These values are fetched from the cached ziglang download index available at <a href="https://ziglang.org/download/index.json">zig official index</a>

With `zv` binaries you can now build with the master branch without changing your default system zig version: 
```sh
zig +master build
```
Caching is built in so you never have to wait any longer than strictly necessary. 

You can also specify a version number `zig +0.15.1`. With `zv` you also have the option of pinning per-project zig versions using a `.zigversion` which takes the same format as `+` would for inline zig commands. You can have a project with these contents:

```sh
# file .zigversion
0.15.1
```
which will always use version `0.15.1` when you run any `zig` command inside it. How cool is that? 

It also doubles up as a project template starter, providing multiple variants of a zig project from a barebones template with a very trimmed down `build.zig` & `main.zig` file or the standard zig project template. Find out more with `zv init --help`.

`zv` prefers community mirrors for downloads as that's the official recommendation with `minisign` and `shasum` verification done before any toolchain is installed.

## Installation

```sh
# From crates.io
cargo install zv
```

```sh
# From GitHub
cargo install --git https://github.com/weezy20/zv
```

## Usage

All `zv` stuff lives in `ZV_DIR`, like zig versions, temporary downloads, cache files & `zv` itself, which you can set as a custom path.

If not set, default zv directory is `$HOME/.zv` on unix like systems and `$USERPROFILE/.zv` on windows. `zv` is aware of emulated shells and WSL so that it always picks the correct default location for `ZV_DIR`.

```sh
# Run zv setup as a one-time step to directories & environment zv needs
# If you have ZV_DIR set, it'll prompt you if you wish to make it permanent.


# First preview changes:
zv setup --dry-run | zv setup -d

# Then once you're happy run:
zv setup # This applies those changes & self-installs zv from your current working directory to <ZV_DIR>/bin

# To get help about a particular subcommand:
zv <subcommand> -h (short help) | --help (long help)
# E.g:
zv setup --help
```

Once `zv setup` is finished you can remove the the cargo installed binary if you used cargo: `cargo uninstall zv`.
`zv` will automatically install itself in `ZV_DIR/bin` after making sure that has been included in your `PATH` so there's no need for a second binary.

Upgrading can be done the same way. You install `zv` from cargo or build it yourself and run `zv setup` to find & replace your existing installation. This is only temporary until `zv upgrade` is implemented after which this won't be required.

## Use `zv` for project creation:

```sh
zv init [project_name]                 # Create a new Zig project with a name
zv init                                # Create a new Zig project in current working directory
zv init --zig | -z                     # Create a new Zig project using the standard template provided by `zig init`
```

## Use `zv` as your ziglang compiler toolchain manager:

```sh
# Version selection - basic usage
zv use <version | master| stable | latest > # Select a Zig version to use. Can be a semver, master (branch)
zv use 0.13.0                               # Use specific semantic version
zv use 0.14                                 # Use version (auto-completes to 0.14.0)
zv use master                               # Use master branch build (queries network to find the latest master build)
zv use stable                               # Use latest stable release (refers to cached index)
zv use latest                               # Use latest stable release (queries network to fetch the latest stable)

# Per project zig config
zig +<version> [...zig args]            # Run zig using a specific <version> (fetches and downloads version if not present locally)    
zig +master [...zig args]              # Run zig using master build. (If already cached - no download but a network request is made to verify version)
zig [...zig args]                      # Uses current configured zig or prefers version from `.zigversion` file in the repository adjacent to `build.zig`.                           

# Management commands
zv list  | ls                          # List installed Zig versions
zv clean | rm                          # Remove zig versions interactively.
zv clean | rm <version | all>          # Clean up all zv managed installations using `all` or just a single one (eg. zv clean 0.14).
zv clean 0.14,0.14.0                   # Clean up multiple Zig installations using a comma separated list.
zv rm master                           # Clean up the `master` branch toolchain.
zv setup                               # Setup shell environment for zv & display instructions for including `$HOME/.zv/bin` or `<ZV_DIR>/bin` to $PATH
zv sync                                # Resync community mirrors list from [ziglang.org/download/community-mirrors.txt]; Also force resync of index to fetch latest nightly builds.
zv help                                # Detailed instructions for zv. Use `--help` for long help or `-h` for short help with a subcommand.
```

`minisign` verification is done using [jedisct1/rust-minisign](https://github.com/jedisct1/rust-minisign) - A Minisign library in pure Rust

It also supports `NO_COLOR` for non TTY environments.

I hope you enjoy using it! ♥


---

### 🔧 Environment Variables for Customizing zv

| Variable                  | Description                                                                                                                | Default / Notes                                                                 |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| **`ZV_LOG`**              | Sets the log level (same as `RUST_LOG`). If set, logging follows the specified level.                                      | Inherits `RUST_LOG` behavior                                                    |
| **`ZV_DIR`**              | Defines the home directory for `zv`.                                                                                       | —                                                                               |
| **`ZV_INDEX_TTL_DAYS`**   | Number of days between automatic [index](https://ziglang.org/download/index.json) syncs.                                   | **21 days** – since `master` & `latest` are always fetched directly from network, we use those network requests to resync index anyways |
| **`ZV_MIRRORS_TTL_DAYS`** | Number of days before refreshing the mirrors list. Broken mirrors degrade automatically. Use `zv sync` to force refresh. | **21 days** – mirrors & index can be resynced immediately with `zv sync`      |

---

