# Changelog 

## v0.5.1 → v0.6.0

## ✨ Features
- Support installations of non indexed zig versions. MachEngine and many other projects rely on zig versions that are not available in the official zig index. Now `zv` supports installations of non-indexed zig versions as they're most likely present on community mirrors even if not indexed. Maybe a future version of `zv` will expand the index to include the machengine zig index for shasum/size information.

## v0.5.0 → v0.5.1

## Bug Fixes
- Before: `zv install 0.15.2` would fail if zig index hadn't expired and `zv sync` was not run, or `zv install latest` wasn't used to fetch latest zig.
- Now: `zv install <version>` falls back to fetching latest zig index if specified version is not found in cached index instead of erroring out.
- That doesn't mean `zv v0.5.0` can't install the latest without a sync, you just had to use `zv install latest` or `zv use latest` or `zv use 0.15.2 -f` without any kind of sync beforehand.

## v0.4.0 → v0.5.0
## ✨ Features
- **`zv install`**: Install without setting a zig version as active.
- Uses same flags as `zv use` i.e. `-f` to force using ziglang.org as a download source.
- Shorthand: **`zv i <version>`** installs the specified version without setting it as active.
- Can also install multiple versions which are comma separated list: `zv i 0.11,master,stable,latest`
- De-duplication for zigversions is handled internally so if you specify `zv i latest,stable` then it only installs one version, provided cached index stable == remote index stable which is true for 99.99% of the time.

## v0.3.1 → v0.4.0

## ✨ Features

- **`zv init`**: Added `build.zig.zon` generation via `-p/--package/--zon` flag (#17)
- **`zv init`**: Generate `.zigversion` file to specify minimum Zig version for builds
- **Self-update**: Refactored using `self-replace` dependency with SHA256 verification for downloaded github assets (#21). This also removes a ton of dependencies.
- **Self-update**: Added `upgrade` as alias for `update` command
- **Self-update**: Added `--rc` flag to include pre-release versions when checking for updates

## Bug Fixes

- Fixed phantom active Zig display when no version was set (zv)
- Fixed toolchain manager fallback for mismatches between version scan and `active.json`
- Improved auto-switching logic when cleaning/removing installations
- Fixed ZIP extraction path issues in update mechanism

## Others

- Deduplicated shim generation logic, Added quiet flag to supress output during automatic shim regeneration
- Target triple detection now at compile time
- Better async handling (replaced blocking calls with `await`)
- Updated documentation and Homebrew tap configuration

---