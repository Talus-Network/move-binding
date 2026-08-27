#!/usr/bin/env bash

set -euo pipefail

release_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$release_root"

workspace_version="$(awk -F ' *= *' '$1 == "version" { gsub(/"/, "", $2); print $2; exit }' Cargo.toml)"
if [[ -z "$workspace_version" ]]; then
    echo "Could not read the workspace version." >&2
    exit 1
fi

packages=(
    talus-sui-move-derive
    talus-sui-move-codegen
    talus-sui-move
    talus-sui-move-call
    talus-sui-move-ptb
    talus-sui-move-runtime
)

# New packages cannot resolve one another through crates.io before the first
# publication. These local patches let Cargo verify the exact package archives.
cargo_config_args=(
    --config "patch.crates-io.talus-sui-move.path=\"$release_root/sui-move\""
    --config "patch.crates-io.talus-sui-move-derive.path=\"$release_root/sui-move-derive\""
    --config "patch.crates-io.talus-sui-move-call.path=\"$release_root/sui-move-call\""
    --config "patch.crates-io.talus-sui-move-ptb.path=\"$release_root/sui-move-ptb\""
    --config "patch.crates-io.talus-sui-move-runtime.path=\"$release_root/sui-move-runtime\""
    --config "patch.crates-io.talus-sui-move-codegen.path=\"$release_root/sui-move-codegen\""
)

package_args=(--quiet --locked --all-features)
if [[ "${ALLOW_DIRTY:-0}" == "1" || "${ALLOW_DIRTY:-0}" == "true" ]]; then
    package_args+=(--allow-dirty)
fi

target_root="${CARGO_TARGET_DIR:-$release_root/target}"

for package in "${packages[@]}"; do
    echo "Checking $package $workspace_version"
    cargo "${cargo_config_args[@]}" package \
        -p "$package" \
        "${package_args[@]}"

    archive="$target_root/package/$package-$workspace_version.crate"
    archive_manifest="$package-$workspace_version/Cargo.toml"
    if [[ ! -f "$archive" ]]; then
        echo "Cargo did not create $archive." >&2
        exit 1
    fi

    if tar -xOf "$archive" "$archive_manifest" | awk '
        /^\[/ {
            dependency_section = ($0 ~ /(^\[|\.)((build|dev)-)?dependencies(\.|\])/)
        }
        dependency_section && /^[[:space:]]*(git|path|registry)[[:space:]]*=/ {
            source_dependency = 1
        }
        END { exit(source_dependency ? 0 : 1) }
    '; then
        echo "$package contains a source dependency in its published manifest." >&2
        exit 1
    fi
done

echo "All package archives passed verification."
