# Manifest Specification (Draft v0.1)

Each package version is represented by a TOML manifest stored in the registry index.

## Required Fields

- `name`: package identifier.
- `version`: semantic version.
- `artifacts`: list of downloadable package artifacts.

## Optional Fields

- `license`
- `homepage`
- `dependencies`: map of package name to semver constraint.

## Artifact Fields

- `target`: Rust-style target triple (for example `x86_64-pc-windows-msvc`).
- `url`: artifact download location.
- `sha256`: expected SHA-256 digest of artifact bytes.
- `size` (optional): expected size in bytes.
- `signature` (optional in v0.1): detached signature reference.
- `archive` (optional): archive type override.

## Example

```toml
name = "ripgrep"
version = "14.1.0"
license = "MIT"
homepage = "https://github.com/BurntSushi/ripgrep"

[dependencies]
pcre2 = ">=10.0, <11.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://packages.example/ripgrep-14.1.0-x86_64-unknown-linux-gnu.tar.zst"
sha256 = "..."

[[artifacts]]
target = "x86_64-pc-windows-msvc"
url = "https://packages.example/ripgrep-14.1.0-x86_64-pc-windows-msvc.zip"
sha256 = "..."
```
