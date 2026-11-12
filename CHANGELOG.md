# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-11-12

### Added
- Comprehensive unit test suite across all core modules (`mac`, `distance`, `sampler`, `clock`, `commands`, `monitor`, `hci`, `cli`) — 74 tests
- Integration tests for the compiled binary (`tests/cli_integration.rs`) covering `--help`, missing args, invalid MAC, and unknown options
- `CHANGELOG.md` following Keep a Changelog format

### Changed
- Derived `Debug` and `PartialEq` on `cli::RunArgs` and `cli::Parsed` to support test assertions
- README testing section clarified

### Removed
- Stale `cargo-xtask` wrapper script (superseded by the `.cargo/config.toml` alias, use `cargo xtask`)
- Unused `src/bnnb_main.rs` (referenced the old `bnnb` crate name)

## [0.1.0] - 2026-11-11

### Added
- Initial Rust port of the original Python `bluetooth-near.py` (BNNB)
- Zero-dependency raw HCI socket communication (no libbluetooth)
- L2CAP connect with RSSI and TX power reading
- Log-distance path-loss model with polynomial accuracy curves
- 7-sample averaging for stable distance measurements
- Edge-triggered lock/unlock after configurable missed polls
- Configurable CLI with dry-run, once, and max-iterations modes
- Clean exit codes (0/1/2/3)
- xtask build pattern with `cargo xtask clean`
- Beerware license
