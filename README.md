# cargo-protologic

This is a small tool to assist in writing [Protologic](https://github.com/Protologic) fleets.

To use the `run` subcommand, you should have the [Protologic Release](https://github.com/Protologic/Release) somewhere on your computer. To simplify usage, you can set the `PROTOLOGIC_PATH` environment variable to the location of the release.

### Usage

```
$ cargo protologic
A helper for creating Protologic fleets in rust!

Usage: cargo protologic <COMMAND>

Commands:
  build  Builds Protologic fleets from the cargo workspace
  list   List all built fleets. If you see none, try building them!
  run    Run battle between two fleets. The replay file will be put in your current directory. Requires your workspace to have exactly two fleets!
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### Features

- Automatically builds a crate the right way to be used by Protologic. No `cdylib` required!
    - Note, you still should configure the release profile as you desire for optimizations
- `build` subcommand uses cargo workspace `default-members` to pick fleets (by default). This enables you to have other helper crates in the workspace without them being confused for fleets!