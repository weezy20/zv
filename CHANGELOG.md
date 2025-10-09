# Changelog 

## v0.3.1 → v0.4.0

## ✨ Features

- **`zv init`**: Added `build.zig.zon` generation via `-p/--package/--zon` flag (#17)
- **`zv init`**: Generate `.zigversion` file to specify minimum Zig version for builds
- **Self-update**: Refactored using `self-replace` dependency with SHA256 verification for downloaded github assets (#21). This also removes a ton of dependencies.
- **Self-update**: Added `upgrade` as alias for `update` command

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