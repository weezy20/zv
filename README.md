# zv (zig version) manager

`zv` is a blazing fast, cross-platform, simple-to-use, compiler toolchain manager for Ziglang, written in Rust. `zv` aims to be the only tool you need for zig stuff on your machine. If you need per-project zig versions, we have it. If you need to just run a single `zig build` with a different zig version that's possible as well. `zv` enables you to write stuff like `zig +master build` and it'll fetch run the build command with master build if you don't have it locally. You can also specify `zig +0.15.1 build` or any other `zig` command really. You also have the option of pinning per-project zig versions using a `.zigversion` file with a valid version number or even strings like `latest` or `master` to make sure you're always using the latest or master branch.

It also doubles up as a project template starter, providing multiple variants of a zig project from a barebones template with just enough code to run `zig build run` or the standard zig project template. Find out more in `zv init --help`.

`zv` prefers community mirrors for downloads as that's the official recommendation with `minisign` and `shasum` verification done before any usage can begin.

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

```sh
# Run zv setup as a one-time step to directories & environment zv needs
# Default zv directory is $HOME/.zv on unix like systems and $USERPROFILE/.zv on windows
# Zv is aware of unix shells on windows & powershell on unix so the correct env variables are used to determine default location.
# If you have ZV_DIR set, it'll prompt you if you wish to make it permanent.


# First preview changes:
zv setup --dry-run | zv setup -d

# Then once you're happy run:
zv setup

# To get help about a particular subcommand:
zv <subcommand> -h (short help) | --help (long help)
# E.g:
zv setup --help
```

Once `zv setup` is installed you can remove the the cargo binary if you used cargo: `cargo uninstall zv`
`zv` will automatically install itself in `ZV_DIR/bin` after making sure that has been included in your `PATH`.

Upgrading can be done the same way. You install `zv` from cargo or build it yourself and run `zv setup` to find & replace your existing 
installation. This is only temporary until `zv upgrade` is implemented after which this won't be required.

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
zig +<version> [..zig args]            # Run zig using a specific <version> (fetches and downloads version if not present locally)    
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
