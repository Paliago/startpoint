# Startpoint

Native Rust/GPUI game launcher. Windows first; macOS supported.
Steam, Epic, GOG and manual games; editable metadata, artwork, themes and local activity/journals.

## Run

Rust 1.88+. Windows requires MSVC, Visual Studio Build Tools (**Desktop development with C++**) and the Windows SDK.

```sh
cargo run --locked -- --example-library
cargo run --locked
```

The example library has isolated data and simulated launches. Use `--diagnostics` for discovery details.
SteamGridDB artwork is optional; API keys stay in the OS credential store.

On macOS without the Metal compiler, add `--features runtime-shaders` to Cargo commands.

## Check

```sh
cargo test --locked --workspace
```

Code: `crates/sdk` (API), `crates/core` (backend), `crates/desktop` (GPUI).
