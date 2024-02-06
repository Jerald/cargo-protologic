use std::fs::DirEntry;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::SystemTime;

use anyhow::Context;
use clap::Parser;
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
    /// A helper for create Protologic fleets in rust!
    #[command(subcommand)]
    Protologic(Commands),
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Commands {
    /// Builds Protologic fleets in the cargo workspace.
    ///
    /// By default will build all packages in the workspace as fleets. You may pass a package name explicitly if you'd like.
    Build {
        /// Package to build
        #[arg(short, long)]
        package: Option<String>,
    },

    /// List all built fleets. If you see none, try building them!
    List {},

    /// Run battle between two fleets. The replay file will be put in your current directory. Requires your workspace to have exactly two fleets!
    ///
    /// Optionally can open the replay in the player.
    Run {
        /// The location of the Protologic/Release repo. Can specify as an environment variable for ease of use!
        #[arg(short, long, env)]
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
        Commands::Build { package } => {
            build(package)?
                .wait()
                .context("trying to wait until the `cargo build` execution has finished")?;

            let is_wasm_output = |entry: &DirEntry| {
                PathBuf::from(entry.file_name())
                    .extension()
                    .and_then(|ext| Some(ext.to_str()? == "wasm"))
                    .unwrap_or(false)
            };

            let wasm_output = std::fs::read_dir(cargo_output_base_path())
                .context("Can't find wasm output from build")?
                .filter(|entry| entry.as_ref().is_ok_and(is_wasm_output));

            for entry in wasm_output {
                optimize_wasm(entry?.path())?;
            }
        }
        Commands::List {} => {
            for entry in find_fleets()? {
                println!("Found fleet: {entry:?}");
            }
        }
        Commands::Run {
            protologic_path,
            debug,
            player,
        } => {
            let fleets = find_fleets()?;
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

fn find_fleets() -> anyhow::Result<Vec<DirEntry>> {
    std::fs::read_dir(fleet_output_base_path()?)
        .context("trying to list fleet output directory")?
        .collect::<io::Result<Vec<DirEntry>>>()
        .context("trying to collect fleets in output directory")
}

fn build(package: Option<String>) -> anyhow::Result<Child> {
    let mut cargo = Command::new("cargo");
    cargo.arg("build");

    if let Some(package) = package {
        cargo.arg("-p").arg(package);
    } else {
        cargo.arg("--workspace");
    }

    cargo.args(["--target", WASI_TARGET]);
    cargo.arg("--release");

    cargo.spawn().context("trying to build packages with cargo")
}

fn optimize_wasm(input_file: impl AsRef<Path>) -> anyhow::Result<()> {
    let opt_options = make_wasm_opt();

    let wasm_file_name = input_file
        .as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("Input path must be a wasm file!");
    let output_file = wasm_opt_output_path(wasm_file_name)?;
    opt_options
        .run(input_file, output_file)
        .context("Error optimizing wasm binary")
}

fn cargo_output_base_path() -> String {
    format!("./target/{WASI_TARGET}/release/")
}

fn wasm_opt_output_path(input_file_name: impl AsRef<str>) -> anyhow::Result<PathBuf> {
    Ok(fleet_output_base_path()?.join(format!("fleet_{}", input_file_name.as_ref())))
}

fn fleet_output_base_path() -> anyhow::Result<PathBuf> {
    let path = PathBuf::from("./target/protologic_fleets/");

    if !path.exists() {
        std::fs::create_dir(&path).context("trying to create fleet output path")?;
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

fn make_wasm_opt() -> OptimizationOptions {
    let mut opt_options = wasm_opt::OptimizationOptions::new_opt_level_4();

    opt_options
        .enable_feature(wasm_opt::Feature::BulkMemory)
        .enable_feature(wasm_opt::Feature::Simd);

    opt_options
        .add_pass(wasm_opt::Pass::Asyncify)
        .set_pass_arg("asyncify-imports", "wasi_snapshot_preview1.sched_yield")
        .add_pass(wasm_opt::Pass::StripDwarf);

    opt_options
}

fn extract_fleet_name(fleet_path: PathBuf) -> anyhow::Result<String> {
    let fleet_file_name = fleet_path
        .with_extension("")
        .file_name()
        .context("fleet name wouldn't be found in fleet path. Try again?")?
        .to_str()
        .context("you need to name your fleet valid unicode!")?
        .to_owned();

    let (_, fleet_name) = fleet_file_name
        .split_once('_')
        .context("trying to split fleet file by underscore")?;

    Ok(fleet_name.to_string())
}

fn battle_output_path(fleet1: &DirEntry, fleet2: &DirEntry) -> anyhow::Result<PathBuf> {
    let now = std::time::SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();
    let fleet1_name = extract_fleet_name(fleet1.path())?;
    let fleet2_name = extract_fleet_name(fleet2.path())?;

    Ok(std::env::current_dir()?.join(format!("{now}_{fleet1_name}_{fleet2_name}")))
}