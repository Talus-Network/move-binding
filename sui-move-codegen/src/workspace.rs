//! High-level helpers for generating bindings for a package and its dependencies.
//!
//! This module is intentionally small and opinionated:
//! - It discovers external package references by walking type signatures in the IR.
//! - It can fetch missing dependencies recursively over gRPC.
//! - It can optionally write a minimal Cargo workspace with one crate per package.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use sui_sdk_types::Address;

use crate::ir::{NormalizedPackage, TypeName, TypeRef};
use crate::render::{render_package_with_resolver, ExternalResolver, RenderOptions};
use crate::{fetch_package, Error};

/// How to treat an externally-provided bindings crate for a package.
#[derive(Clone, Debug)]
pub struct ExternalCrate {
    /// Rust package name from `Cargo.toml` (e.g. `my-bindings`).
    pub cargo_name: String,
    /// Directory containing `Cargo.toml`.
    pub crate_dir: PathBuf,
}

/// Options for generating a bindings workspace.
#[derive(Clone, Debug)]
pub struct WorkspaceOptions {
    /// Write `Cargo.toml` files for generated crates.
    ///
    /// If set, dependencies on the core `sui-move*` crates are expressed as path dependencies
    /// rooted at this directory (typically the `move-binding` repo root).
    pub move_binding_root: Option<PathBuf>,

    /// Force `RenderOptions::flatten = false` for all generated crates.
    ///
    /// This keeps generated module paths stable so cross-crate references use
    /// `dep_crate::module::Type`.
    pub force_non_flattened: bool,
}

impl Default for WorkspaceOptions {
    fn default() -> Self {
        Self {
            move_binding_root: None,
            force_non_flattened: true,
        }
    }
}

/// Errors that can occur during dependency discovery/fetching or workspace generation.
#[derive(thiserror::Error, Debug)]
pub enum WorkspaceError {
    /// A package id string was not a valid Sui address.
    #[error("invalid package id `{0}`")]
    InvalidPackageId(String),

    /// Failed to read or parse an external crate's `Cargo.toml`.
    #[error("invalid external crate at `{path}`: {message}")]
    InvalidExternalCrate {
        /// Path to the `Cargo.toml` that was read.
        path: String,
        /// Human-readable parse or IO error message.
        message: String,
    },

    /// Fetching or normalizing package metadata failed.
    #[error(transparent)]
    Source(#[from] Error),

    /// An IO operation failed while writing the workspace.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Generate bindings for `root_package`, plus any referenced dependency packages.
///
/// - If a referenced package id exists in `externals`, codegen will reference it via that crate
///   name and *won't* generate a new crate for it.
/// - Otherwise, the dependency is fetched and bindings are generated for it as well.
///
/// The output directory will contain one folder per generated package crate. If
/// `workspace_opts.move_binding_root` is set, it will also contain a root `Cargo.toml` workspace.
pub async fn generate_bindings_workspace(
    client: &mut sui_rpc::Client,
    root_package: Address,
    out_dir: impl AsRef<Path>,
    render_opts: &RenderOptions,
    externals: BTreeMap<String, PathBuf>,
    workspace_opts: WorkspaceOptions,
) -> Result<(), WorkspaceError> {
    let out_dir = out_dir.as_ref();
    fs::create_dir_all(out_dir)?;

    let externals = load_external_crates(externals)?;

    let graph = fetch_dependency_closure(client, root_package, &externals).await?;

    // Decide crate names for all fetched packages (generated + external).
    let mut crate_by_storage: BTreeMap<String, String> = BTreeMap::new();
    for (storage_id, pkg) in &graph.packages_by_storage {
        if let Some(ext) = external_for_package(&externals, &graph.alias_to_storage, pkg) {
            crate_by_storage.insert(storage_id.clone(), ext.cargo_name.clone());
        } else {
            let base = pkg
                .original_id
                .as_deref()
                .unwrap_or(pkg.storage_id.as_str());
            crate_by_storage.insert(storage_id.clone(), default_crate_name(base));
        }
    }

    // Build a global resolver so each generated crate can reference every other package.
    let mut resolver = ExternalResolver::new();
    for (storage_id, pkg) in &graph.packages_by_storage {
        let crate_name = crate_by_storage
            .get(storage_id)
            .expect("crate name assigned");
        resolver.add_package(pkg, crate_name.clone());
    }

    // Generate crates for packages that are not provided externally.
    let mut generated_members = Vec::new();
    for (storage_id, pkg) in &graph.packages_by_storage {
        if external_for_package(&externals, &graph.alias_to_storage, pkg).is_some() {
            continue;
        }

        let crate_name = crate_by_storage
            .get(storage_id)
            .expect("crate name assigned");
        let crate_dir = out_dir.join(crate_name);
        write_generated_crate(
            &crate_dir,
            pkg,
            render_opts,
            &resolver,
            &crate_by_storage,
            &graph.alias_to_storage,
            &externals,
            &workspace_opts,
        )?;
        generated_members.push(crate_name.clone());
    }

    if workspace_opts.move_binding_root.is_some() {
        write_root_workspace_manifest(out_dir, &generated_members)?;
    }

    Ok(())
}

struct DependencyGraph {
    packages_by_storage: BTreeMap<String, NormalizedPackage>,
    alias_to_storage: BTreeMap<String, String>,
}

async fn fetch_dependency_closure(
    client: &mut sui_rpc::Client,
    root: Address,
    externals: &BTreeMap<String, ExternalCrate>,
) -> Result<DependencyGraph, WorkspaceError> {
    let root_norm = normalize_address(&root.to_string());
    let mut queue: VecDeque<String> = VecDeque::from([root_norm]);

    let mut packages_by_storage: BTreeMap<String, NormalizedPackage> = BTreeMap::new();
    let mut alias_to_storage: BTreeMap<String, String> = BTreeMap::new();

    while let Some(addr) = queue.pop_front() {
        if alias_to_storage.contains_key(&addr) {
            continue;
        }

        // Fetch this package id (may be a storage id or original id).
        let parsed: Address = addr
            .parse()
            .map_err(|_| WorkspaceError::InvalidPackageId(addr.clone()))?;
        let pkg = fetch_package(client, parsed).await?;

        let storage_id = pkg.storage_id.clone();
        let storage_norm = normalize_address(&storage_id);
        let original_norm = pkg.original_id.as_deref().map(normalize_address);

        packages_by_storage.insert(storage_norm.clone(), pkg.clone());
        alias_to_storage.insert(storage_norm.clone(), storage_norm.clone());
        if let Some(orig) = &original_norm {
            alias_to_storage.insert(orig.clone(), storage_norm.clone());
        }
        // Also mark the queried id as resolved (in case it was neither storage nor original
        // due to future RPC behavior changes).
        alias_to_storage.insert(addr.clone(), storage_norm.clone());

        // Enqueue dependency package ids referenced by types.
        let deps = referenced_external_packages(&pkg);
        for dep in deps {
            if is_framework_address(&dep) {
                continue;
            }
            if alias_to_storage.contains_key(&dep) {
                continue;
            }

            // Ensure we have metadata even for externally-provided crates, so we can determine
            // object abilities (`key`) for call generation.
            let _ = externals;
            queue.push_back(dep);
        }
    }

    Ok(DependencyGraph {
        packages_by_storage,
        alias_to_storage,
    })
}

fn referenced_external_packages(pkg: &NormalizedPackage) -> BTreeSet<String> {
    let mut out = BTreeSet::new();

    let mut visit = |type_name: &TypeName| {
        if is_local_type(type_name, pkg) {
            return;
        }
        let addr = normalize_address(&type_name.address);
        if is_framework_address(&addr) {
            return;
        }
        out.insert(addr);
    };

    for module in pkg.modules.values() {
        for dt in &module.datatypes {
            match &dt.kind {
                crate::ir::DatatypeKind::Struct { fields } => {
                    for f in fields {
                        visit_type_ref(&f.ty, &mut visit);
                    }
                }
                crate::ir::DatatypeKind::Enum { variants } => {
                    for v in variants {
                        for f in &v.fields {
                            visit_type_ref(&f.ty, &mut visit);
                        }
                    }
                }
            }
        }

        for f in &module.functions {
            for p in &f.parameters {
                visit_type_ref(&p.ty, &mut visit);
            }
            for r in &f.return_types {
                visit_type_ref(r, &mut visit);
            }
        }
    }

    out
}

fn visit_type_ref(ty: &TypeRef, visit: &mut impl FnMut(&TypeName)) {
    match ty {
        TypeRef::Vector(inner) => visit_type_ref(inner, visit),
        TypeRef::Ref { inner, .. } => visit_type_ref(inner, visit),
        TypeRef::Datatype {
            type_name,
            type_arguments,
        } => {
            visit(type_name);
            for a in type_arguments {
                visit_type_ref(a, visit);
            }
        }
        _ => {}
    }
}

fn is_framework_address(addr: &str) -> bool {
    addr == "0x1" || addr == "0x2"
}

fn is_local_type(type_name: &TypeName, pkg: &NormalizedPackage) -> bool {
    if normalize_address(&type_name.address) == normalize_address(&pkg.storage_id) {
        return true;
    }
    match &pkg.original_id {
        Some(orig) => normalize_address(&type_name.address) == normalize_address(orig),
        None => false,
    }
}

fn normalize_address(input: &str) -> String {
    let trimmed = input.trim();
    let addr = trimmed
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .trim_start_matches('0');
    let addr = if addr.is_empty() { "0" } else { addr };
    format!("0x{addr}")
}

fn default_crate_name(address: &str) -> String {
    // Use a short prefix of the address to keep names readable but deterministic.
    let hex = address.trim().trim_start_matches("0x");
    let short = &hex[..hex.len().min(16)];
    format!("move_pkg_{short}")
}

fn load_external_crates(
    externals: BTreeMap<String, PathBuf>,
) -> Result<BTreeMap<String, ExternalCrate>, WorkspaceError> {
    let mut out = BTreeMap::new();
    for (pkg_id, crate_dir) in externals {
        let pkg_id = normalize_address(&pkg_id);
        let cargo_name = read_cargo_package_name(&crate_dir)?;
        out.insert(
            pkg_id,
            ExternalCrate {
                cargo_name,
                crate_dir,
            },
        );
    }
    Ok(out)
}

fn external_for_package<'a>(
    externals: &'a BTreeMap<String, ExternalCrate>,
    alias_to_storage: &BTreeMap<String, String>,
    pkg: &NormalizedPackage,
) -> Option<&'a ExternalCrate> {
    let storage = normalize_address(&pkg.storage_id);
    let orig = pkg.original_id.as_deref().map(normalize_address);

    // Prefer a direct match first.
    if let Some(ext) = externals.get(&storage) {
        return Some(ext);
    }
    if let Some(orig) = &orig {
        if let Some(ext) = externals.get(orig) {
            return Some(ext);
        }
    }

    // Then look for alias matches (if the user specified an original id but the graph resolved
    // a storage id, or vice versa).
    for (ext_id, ext) in externals {
        if let Some(storage_for_ext) = alias_to_storage.get(ext_id) {
            if storage_for_ext == &storage {
                return Some(ext);
            }
        }
    }
    None
}

fn external_for_address<'a>(
    externals: &'a BTreeMap<String, ExternalCrate>,
    alias_to_storage: &BTreeMap<String, String>,
    address: &str,
) -> Option<&'a ExternalCrate> {
    if let Some(ext) = externals.get(address) {
        return Some(ext);
    }

    let storage = alias_to_storage
        .get(address)
        .map(|s| s.as_str())
        .unwrap_or(address);
    if let Some(ext) = externals.get(storage) {
        return Some(ext);
    }

    externals.iter().find_map(|(ext_id, ext)| {
        alias_to_storage
            .get(ext_id)
            .is_some_and(|resolved| resolved == storage)
            .then_some(ext)
    })
}

fn read_cargo_package_name(crate_dir: &Path) -> Result<String, WorkspaceError> {
    let cargo_toml = crate_dir.join("Cargo.toml");
    let contents =
        fs::read_to_string(&cargo_toml).map_err(|e| WorkspaceError::InvalidExternalCrate {
            path: cargo_toml.display().to_string(),
            message: e.to_string(),
        })?;

    let mut in_package = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim_start();
            if !rest.starts_with('=') {
                continue;
            }
            let value = rest.trim_start_matches('=').trim();
            if let Some(stripped) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                if !stripped.is_empty() {
                    return Ok(stripped.to_string());
                }
            }
        }
    }

    Err(WorkspaceError::InvalidExternalCrate {
        path: cargo_toml.display().to_string(),
        message: "missing [package] name".to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
fn write_generated_crate(
    crate_dir: &Path,
    pkg: &NormalizedPackage,
    render_opts: &RenderOptions,
    resolver: &ExternalResolver,
    crate_by_storage: &BTreeMap<String, String>,
    alias_to_storage: &BTreeMap<String, String>,
    externals: &BTreeMap<String, ExternalCrate>,
    workspace_opts: &WorkspaceOptions,
) -> Result<(), WorkspaceError> {
    fs::create_dir_all(crate_dir.join("src"))?;

    let mut opts = render_opts.clone();
    if workspace_opts.force_non_flattened {
        opts.flatten = false;
    }

    let code = render_package_with_resolver(pkg, &opts, resolver);
    fs::write(crate_dir.join("src").join("lib.rs"), code)?;

    if let Some(move_binding_root) = &workspace_opts.move_binding_root {
        let deps = referenced_external_packages(pkg);
        let mut dep_specs: BTreeMap<String, String> = BTreeMap::new();

        for dep in deps {
            // Skip deps that are actually local aliases.
            if dep == normalize_address(&pkg.storage_id)
                || pkg
                    .original_id
                    .as_deref()
                    .is_some_and(|o| dep == normalize_address(o))
            {
                continue;
            }

            let storage = alias_to_storage
                .get(&dep)
                .cloned()
                .unwrap_or_else(|| dep.clone());
            let dep_crate = crate_by_storage
                .get(&storage)
                .cloned()
                .unwrap_or_else(|| default_crate_name(&dep));

            let dep_path =
                if let Some(ext) = external_for_address(externals, alias_to_storage, &dep) {
                    ext.crate_dir.display().to_string()
                } else {
                    format!("../{dep_crate}")
                };
            dep_specs.insert(dep_crate, dep_path);
        }

        let cargo_toml = render_cargo_toml(
            crate_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("move_pkg"),
            move_binding_root,
            &opts,
            &dep_specs,
        );
        fs::write(crate_dir.join("Cargo.toml"), cargo_toml)?;
    }

    Ok(())
}

fn render_cargo_toml(
    package_name: &str,
    move_binding_root: &Path,
    opts: &RenderOptions,
    deps: &BTreeMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str("[package]\n");
    out.push_str(&format!("name = \"{package_name}\"\n"));
    out.push_str("version = \"0.1.0\"\n");
    out.push_str("edition = \"2021\"\n\n");

    out.push_str("[dependencies]\n");
    out.push_str(&format!(
        "sui-move = {{ path = \"{}\", features = [\"derive\"] }}\n",
        move_binding_root.join("sui-move").display()
    ));
    out.push_str(&format!(
        "sui-move-call = {{ path = \"{}\" }}\n",
        move_binding_root.join("sui-move-call").display()
    ));
    if opts.emit_tx_ext {
        out.push_str(&format!(
            "sui-move-runtime = {{ path = \"{}\" }}\n",
            move_binding_root.join("sui-move-runtime").display()
        ));
    }

    for (name, path) in deps {
        out.push_str(&format!("{name} = {{ path = \"{path}\" }}\n"));
    }

    out
}

fn write_root_workspace_manifest(out_dir: &Path, members: &[String]) -> Result<(), WorkspaceError> {
    let mut out = String::new();
    out.push_str("[workspace]\n");
    out.push_str("resolver = \"3\"\n");
    out.push_str("members = [\n");
    for m in members {
        out.push_str(&format!("  \"{m}\",\n"));
    }
    out.push_str("]\n");
    fs::write(out_dir.join("Cargo.toml"), out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    #[test]
    fn referenced_external_packages_skips_framework_and_local() {
        let pkg = NormalizedPackage {
            storage_id: "0xa".into(),
            original_id: Some("0xa0".into()),
            version: 0,
            modules: BTreeMap::from([(
                "m".into(),
                NormalizedModule {
                    name: "m".into(),
                    datatypes: vec![Datatype {
                        type_name: TypeName::parse("0xa::m::S").unwrap(),
                        module: "m".into(),
                        name: "S".into(),
                        abilities: vec![Ability::Store],
                        type_parameters: vec![],
                        kind: DatatypeKind::Struct {
                            fields: vec![
                                // framework builtin (skipped)
                                Field {
                                    name: "uid".into(),
                                    position: 0,
                                    ty: TypeRef::Datatype {
                                        type_name: TypeName::parse("0x2::object::UID").unwrap(),
                                        type_arguments: vec![],
                                    },
                                },
                                // local type via storage id (skipped)
                                Field {
                                    name: "local".into(),
                                    position: 1,
                                    ty: TypeRef::Datatype {
                                        type_name: TypeName::parse("0xa::m::S").unwrap(),
                                        type_arguments: vec![],
                                    },
                                },
                                // local type via original id (skipped)
                                Field {
                                    name: "local_orig".into(),
                                    position: 2,
                                    ty: TypeRef::Datatype {
                                        type_name: TypeName::parse("0xa0::m::S").unwrap(),
                                        type_arguments: vec![],
                                    },
                                },
                                // external package (included)
                                Field {
                                    name: "ext".into(),
                                    position: 3,
                                    ty: TypeRef::Datatype {
                                        type_name: TypeName::parse("0xb::dep::Obj").unwrap(),
                                        type_arguments: vec![],
                                    },
                                },
                            ],
                        },
                    }],
                    functions: vec![],
                },
            )]),
        };

        let deps = referenced_external_packages(&pkg);
        assert_eq!(deps, BTreeSet::from(["0xb".to_string()]));
    }
}
