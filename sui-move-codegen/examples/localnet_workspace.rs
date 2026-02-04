//! Generate a bindings workspace for a Move package (plus its dependency closure) on localnet.
//!
//! By default this connects to `http://127.0.0.1:9000` (override with `--grpc` or `SUI_GRPC`)
//! and writes output under `../target/bindings/<package_id_without_0x>`.
//!
//! Example:
//! - `cargo run -p sui-move-codegen --example localnet_workspace -- --check`
//! - `cargo run -p sui-move-codegen --example localnet_workspace -- 0x4cc... --check`
//! - `cargo run -p sui-move-codegen --example localnet_workspace -- 0x4cc... --external 0x0aaa...=../my-primitives-crate --check`

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use sui_move_codegen::render::RenderOptions;
use sui_move_codegen::workspace::{generate_bindings_workspace, WorkspaceOptions};
use sui_move_codegen::{Address, Client};

const DEFAULT_PACKAGE: &str =
    "0x4cc38b7c23bf14d7555503ab38a9748f9544c2c29c6519df412b4f6fb6971640";
const DEFAULT_GRPC: &str = "http://127.0.0.1:9000";

fn usage() -> ! {
    eprintln!(
        "Usage: localnet_workspace [package_id] [--grpc <url>] [--out <dir>] [--external <pkg>=<crate_dir>]... [--check]\n\n\
         Defaults:\n\
         - package_id: {DEFAULT_PACKAGE}\n\
         - grpc: {DEFAULT_GRPC} (or $SUI_GRPC)\n\
         - out: <repo>/target/bindings/<package_id_without_0x>\n"
    );
    std::process::exit(2);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).peekable();

    let mut package_id = DEFAULT_PACKAGE.to_string();
    let mut grpc = std::env::var("SUI_GRPC").unwrap_or_else(|_| DEFAULT_GRPC.to_string());
    let mut out_dir: Option<PathBuf> = None;
    let mut externals: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut check = false;

    // Optional first positional arg: package id.
    if let Some(first) = args.peek() {
        if !first.starts_with('-') {
            package_id = args.next().unwrap();
        }
    }

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--grpc" => grpc = args.next().unwrap_or_else(|| usage()),
            "--out" => out_dir = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--external" => {
                let spec = args.next().unwrap_or_else(|| usage());
                let (pkg, path) = spec
                    .split_once('=')
                    .ok_or("expected --external <pkg_id>=<crate_dir>")?;
                externals.insert(pkg.to_string(), PathBuf::from(path));
            }
            "--check" => check = true,
            "--help" | "-h" => usage(),
            other => return Err(format!("unknown argument `{other}`").into()),
        }
    }

    let root_pkg: Address = package_id.parse()?;
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()?;
    let default_out = repo_root
        .join("target")
        .join("bindings")
        .join(package_id.trim_start_matches("0x"));
    let out_dir = out_dir.unwrap_or(default_out);

    println!("grpc: {grpc}");
    println!("package: {package_id}");
    println!("out: {}", out_dir.display());

    let mut client = Client::new(grpc)?;
    let render_opts = RenderOptions::default();
    let ws_opts = WorkspaceOptions {
        move_binding_root: Some(repo_root),
        force_non_flattened: true,
    };

    generate_bindings_workspace(
        &mut client,
        root_pkg,
        &out_dir,
        &render_opts,
        externals,
        ws_opts,
    )
    .await?;

    println!("generated: {}", out_dir.display());
    if check {
        let status = Command::new("cargo")
            .arg("check")
            .current_dir(&out_dir)
            .status()?;
        if !status.success() {
            return Err(format!("cargo check failed with status {status}").into());
        }
    } else {
        println!("next: (optional) cd {} && cargo check", out_dir.display());
    }

    Ok(())
}

