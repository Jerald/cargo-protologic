# Changelog
## 0.X.0

## 0.2.1
- Support `--debug` in `cargo protologic build`. This uses debug build for rust and removes most `wasm_opt` optimizations, but makes things slow.

## 0.2.0
- Much more noisy! Now there are messages telling you what's happening. Maybe too many?
- Now tells you the before/after size from running `wasm_opt`. Has nice-ish formatting!
- Builds fleets as `cdylib` automatically.
    - Now we use `cargo rustc` instead of `cargo build`, which allows passing a `--crate-type` flag to control what output artifacts are produced. This makes it easier for users, who no longer need to set this manually!
- Uses cargo workspace `default-members` to decide which packages to build as fleets.
    - Previously it would build all packages. Now you can have helper packages in your workspace without them being built as fleets.
- Optimized fleet artifacts no longer have `fleet_` prepended to them. It made things annoying...
- [Internal] Uses `cargo metadata` and `serde_json` to parse out information. This is used to power other changes!

## 0.1.0

Initial release!