use std::fs::DirEntry;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::SystemTime;

use anyhow::Context;
use bytesize::ByteSize;
use clap::Parser;
use serde::{Deserialize, Serialize};
use wasm_opt::OptimizationOptions;

/// You shouldn't see this! Run this tool like `cargo protologic`.
#[derive(clap::Parser, Debug)]
#[command(name = "cargo-protologic", bin_name = "cargo")]
#[command(author, version)]
struct CargoProtologic {
    #[command(subcommand)]
    command: ProtologicCommand,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum ProtologicCommand {
    /// A helper for creating Protologic fleets in rust!
    #[command(subcommand)]
    Protologic(Commands),
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Commands {
    /// Builds Protologic fleets from the cargo workspace.
    ///
    /// With no argument, it will build the default members of the workspace. You may pass a package name explicitly instead.
    Build {
        /// Package to build. May be repeated multiple times!
        #[arg(short, long)]
        package: Option<Vec<String>>,
        /// Enables debug build and removes wasm_opt optimizations. Makes things very slow!
        #[arg(short, long, default_value = "false")]
        debug: bool
    },

    /// List all built fleets. If you see none, try building them!
    List {},

    /// Run battle between two fleets. The replay file will be put in your current directory. Requires your workspace to have exactly two fleets!
    ///
    /// Optionally can open the replay in the player.
    Run {
        /// The location of the Protologic/Release repo. Can specify as an environment variable for ease of use!
        #[arg(long, env)]
        protologic_path: PathBuf,
        /// Whether to set the `--debug` flag in Protologic.
        #[arg(short, long, default_value = "false")]
        debug: bool,
        /// Do you want the replay opened in the player?
        #[arg(short, long, default_value = "false")]
        player: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let CargoProtologic {
        command: ProtologicCommand::Protologic(command),
    } = CargoProtologic::parse();
    println!("{command:?}");

    match command {
        Commands::Build { package , debug } => {
            println!("Building packages...");
            for package in package.map_or_else(list_workspace_fleets, Result::Ok)? {
                build(package, debug)?
                    .wait()
                    .context("trying to wait until the `cargo build` execution has finished")?;
            }

            let is_wasm_output = |entry: &DirEntry| {
                entry
                    .path()
                    .extension()
                    .and_then(|ext| Some(ext.to_str()? == "wasm"))
                    .unwrap_or(false)
            };

            let wasm_output = std::fs::read_dir(cargo_output_base_path(debug)?)
                .context("Can't find wasm output from build")?
                .filter(|entry| entry.as_ref().is_ok_and(is_wasm_output))
                .collect::<Vec<_>>();

            if wasm_output.is_empty() {
                println!("No wasm output found. Your build didn't produce any .wasm files!");
            } else {
                println!("Optimizing wasm outputs...");
                for entry in wasm_output {
                    optimize_wasm(entry?.path(), debug)?;
                }
                println!("Done optimizing!");
            }
        }
        Commands::List {} => {
            println!("Listing built fleets...");

            for entry in find_built_fleets()? {
                println!("Found fleet: {entry:?}");
            }
        }
        Commands::Run {
            protologic_path,
            debug,
            player,
        } => {
            let fleets = find_built_fleets()?;
            let [fleet1, fleet2] = fleets
                .first_chunk()
                .context("tried to get find fleets 1 and 2")?;

            let battle_output = battle_output_path(fleet1, fleet2)?;

            println!("Starting the protologic sim...");

            std::process::Command::new(protologic_sim_path(&protologic_path))
                .arg("--fleets")
                .args([fleet1.path(), fleet2.path()])
                .arg("--debug")
                .arg(debug.to_string())
                .arg("--output")
                .arg(battle_output.clone())
                .spawn()
                .context("trying to run sim on fleets")?
                .wait()
                .context("trying to wait until the protologic sim has finished running")?;

            println!("Protologic sim complete!");

            if player {
                println!("Starting the protologic player! The command will exit now.");

                let mut command =
                    std::process::Command::new(protologic_player_path(&protologic_path));
                command.arg(battle_output.with_extension("json.deflate"));
                println!("Command to open player: {:?}", command);

                command
                    .spawn()
                    .context("trying to open protologic player from sim output")?;
            }
        }
    }

    Ok(())
}

const WASI_TARGET: &str = "wasm32-wasi";

#[derive(Serialize, Deserialize, Debug)]
struct ParsedMetadata {
    workspace_default_members: Vec<String>,
    target_directory: PathBuf,
}

fn cargo_metadata() -> anyhow::Result<ParsedMetadata> {
    let mut cargo = std::process::Command::new("cargo");
    cargo.arg("metadata").args(["--format-version", "1"]);

    let output = cargo
        .output()
        .context("trying to run `cargo metadata` to find workspace members")?;

    serde_json::from_slice(&output.stdout).context("trying to parse `cargo metadata` output")
}

/// Lists the fleets in the workspace.
///
/// Currently it provides all `default-members` of the cargo workspace. The intended workflow is
/// to make non-fleet packages (i.e. helpers) non-default members.
fn list_workspace_fleets() -> anyhow::Result<Vec<String>> {
    let metadata = cargo_metadata()?;
    println!("Metadata: {metadata:?}");

    Ok(metadata.workspace_default_members)
}

fn find_built_fleets() -> anyhow::Result<Vec<DirEntry>> {
    std::fs::read_dir(fleet_output_base_path()?)
        .context("trying to list fleet output directory")?
        .collect::<io::Result<Vec<DirEntry>>>()
        .context("trying to collect fleets in output directory")
}

fn build(package: String, debug: bool) -> anyhow::Result<Child> {
    let mut cargo = Command::new("cargo");
    cargo
        // Using `rustc` instead of `build` so we can pass `--crate-type`
        .arg("rustc")
        .args(["-p", &package])
        // This is needed for rustc to produce a .wasm artifact
        .args(["--crate-type", "cdylib"])
        .args(["--target", WASI_TARGET]);

    if !debug { 
        cargo.arg("--release");
    }

    cargo.spawn().context("trying to build packages with cargo")
}

fn optimize_wasm(input_path: impl AsRef<Path>, debug: bool) -> anyhow::Result<()> {
    fn size_from_fs(path: impl AsRef<Path>) -> anyhow::Result<u64> {
        std::fs::metadata(path)
            .context("trying to access path to query size")
            .map(|m| m.len())
    }

    let input_size = size_from_fs(&input_path)?;

    let wasm_file_name = input_path
        .as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("Input path must be a wasm file!");

    let output_path = wasm_opt_output_path(wasm_file_name)?;
    make_wasm_opt(debug)
        .run(&input_path, &output_path)
        .context("Error optimizing wasm binary")?;

    let output_size = size_from_fs(&output_path)?;

    let fleet_name = extract_fleet_name(&input_path)?;
    println!(
        "[Optimizing wasm] Fleet '{fleet_name}' optimized {} -> {}",
        ByteSize::b(input_size),
        ByteSize::b(output_size)
    );

    Ok(())
}

fn cargo_output_base_path(debug: bool) -> anyhow::Result<PathBuf> {
    let metadata = cargo_metadata()?;
    let profile = if debug { "debug" } else { "release" };
    Ok(metadata
        .target_directory
        .join(format!("./{WASI_TARGET}/{profile}/")))
}

fn wasm_opt_output_path(input_file_name: impl AsRef<str>) -> anyhow::Result<PathBuf> {
    Ok(fleet_output_base_path()?.join(input_file_name.as_ref()))
}

fn fleet_output_base_path() -> anyhow::Result<PathBuf> {
    let path = PathBuf::from("./target/protologic_fleets/");

    if !path.exists() {
        std::fs::create_dir(&path)
            .with_context(|| format!("trying to create fleet output path: {path:?}",))?;
    }

    Ok(path)
}

fn protologic_sim_path(protologic_path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        protologic_path.join("Sim/Windows/Protologic.Terminal.exe")
    }

    #[cfg(target_os = "linux")]
    {
        protologic_path
            .as_path()
            .join("Sim/Linux/Protologic.Terminal")
    }
}

fn protologic_player_path(protologic_path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        protologic_path.join("Player/Windows/PROTOLOGIC.exe")
    }

    #[cfg(target_os = "linux")]
    {
        compile_error!("Protologic player doesn't support Linux! Go bug Martin to support this :)")
    }
}

fn make_wasm_opt(debug: bool) -> OptimizationOptions {
    let mut opt_options = if debug {
        wasm_opt::OptimizationOptions::new_opt_level_0()
    } else {
        wasm_opt::OptimizationOptions::new_opt_level_4()
    };

    if debug {
        opt_options.debug_info(true);
    } else {
        opt_options.add_pass(wasm_opt::Pass::StripDwarf);
    }

    opt_options
            .enable_feature(wasm_opt::Feature::BulkMemory)
            .enable_feature(wasm_opt::Feature::Simd);

    opt_options
        .add_pass(wasm_opt::Pass::Asyncify)
        .set_pass_arg("asyncify-imports", "wasi_snapshot_preview1.sched_yield");

    opt_options
}

/// Takes the path to a fleet, extracting out the name of the fleet the correct way
fn extract_fleet_name(fleet_path: impl AsRef<Path>) -> anyhow::Result<String> {
    fleet_path
        .as_ref()
        // drop the `.wasm`
        .with_extension("")
        .file_name()
        .context("fleet name wouldn't be found in fleet path. Try again?")?
        .to_str()
        .context("you need to name your fleet valid unicode!")
        .map(ToOwned::to_owned)
}

fn battle_output_path(fleet1: &DirEntry, fleet2: &DirEntry) -> anyhow::Result<PathBuf> {
    let now = std::time::SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();
    let fleet1_name = extract_fleet_name(fleet1.path())?;
    let fleet2_name = extract_fleet_name(fleet2.path())?;

    Ok(std::env::current_dir()?.join(format!("{now}_{fleet1_name}_{fleet2_name}")))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::extract_fleet_name;

    #[test]
    fn extract_fleet_name_is_sane() -> anyhow::Result<()> {
        let path = PathBuf::from("fleet_demo_fleet_foo_bar.wasm");
        let name = extract_fleet_name(path)?;
        assert_eq!("fleet_demo_fleet_foo_bar", name);

        let path = PathBuf::from("demo_fleet_foo_bar");
        let name = extract_fleet_name(path)?;
        assert_eq!("demo_fleet_foo_bar", name);

        Ok(())
    }
}
