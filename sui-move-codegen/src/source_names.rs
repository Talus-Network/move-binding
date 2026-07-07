//! Recover Move function parameter names from source files.

use {
    crate::ir::{FunctionParameterNameMismatch, NormalizedPackage},
    std::{
        collections::BTreeMap,
        io,
        path::{Path, PathBuf},
    },
};

/// Errors from recovering function parameter names from Move sources.
#[derive(Debug, thiserror::Error)]
pub enum SourceNameError {
    /// Reading the source directory failed.
    #[error("read Move source directory {}: {source}", path.display())]
    ReadDir {
        /// Directory that was read.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// Reading an entry from the source directory failed.
    #[error("read entry under {}: {source}", path.display())]
    ReadDirEntry {
        /// Directory being iterated.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// Reading a Move source file failed.
    #[error("read Move source {}: {source}", path.display())]
    ReadSource {
        /// Source file that was read.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },

    /// A source file did not contain a supported `module package::name;` header.
    #[error("Move source {} does not declare a `module package::name;` header", path.display())]
    MissingModuleHeader {
        /// Source file that was parsed.
        path: PathBuf,
    },

    /// Source names could not be applied because source and IR arities differ.
    #[error("{0}")]
    FunctionParameterNameMismatch(#[from] FunctionParameterNameMismatch),
}

/// Apply recovered source parameter names to synthesized `argN` names in a normalized package.
///
/// `source_dir` is the package `sources/` directory. Missing directories are treated as empty so
/// callers can invoke this uniformly for packages that may not have local sources.
pub fn apply_function_parameter_names_from_sources(
    package: &mut NormalizedPackage,
    source_dir: impl AsRef<Path>,
) -> Result<(), SourceNameError> {
    let names = function_parameter_names_from_sources(source_dir)?;
    if names.is_empty() {
        return Ok(());
    }

    package.apply_function_parameter_names(&names)?;
    Ok(())
}

/// Recover function parameter names from all `.move` files under a package `sources/` directory.
///
/// The returned map is keyed by `(module_name, function_name)`.
pub fn function_parameter_names_from_sources(
    source_dir: impl AsRef<Path>,
) -> Result<BTreeMap<(String, String), Vec<String>>, SourceNameError> {
    let source_dir = source_dir.as_ref();
    if !source_dir.is_dir() {
        return Ok(BTreeMap::new());
    }

    let mut names = BTreeMap::new();
    let entries = std::fs::read_dir(source_dir).map_err(|source| SourceNameError::ReadDir {
        path: source_dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| SourceNameError::ReadDirEntry {
            path: source_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("move") {
            continue;
        }

        let source =
            std::fs::read_to_string(&path).map_err(|source| SourceNameError::ReadSource {
                path: path.clone(),
                source,
            })?;
        let source = strip_move_comments(&source);
        let module_name = parse_move_module_name(&source)
            .ok_or_else(|| SourceNameError::MissingModuleHeader { path: path.clone() })?;

        for (function_name, parameter_names) in parse_move_function_parameter_names(&source) {
            names.insert((module_name.clone(), function_name), parameter_names);
        }
    }

    Ok(names)
}

fn strip_move_comments(source: &str) -> String {
    let mut stripped = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_block_comment = false;

    while let Some(ch) = chars.next() {
        if in_block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }

        if ch == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    stripped.push('\n');
                    break;
                }
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_block_comment = true;
            continue;
        }

        stripped.push(ch);
    }

    stripped
}

fn parse_move_module_name(source: &str) -> Option<String> {
    let marker = "module ";
    let start = source.find(marker)? + marker.len();
    let rest = &source[start..];
    let module_start = rest.find("::")? + 2;
    let module = rest[module_start..]
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>();
    if module.is_empty() {
        None
    } else {
        Some(module)
    }
}

fn parse_move_function_parameter_names(source: &str) -> Vec<(String, Vec<String>)> {
    let mut functions = Vec::new();
    let bytes = source.as_bytes();
    let mut offset = 0;

    while let Some(relative_fun) = source[offset..].find("fun") {
        let fun_start = offset + relative_fun;
        if !is_keyword_at(bytes, fun_start, b"fun") {
            offset = fun_start + 3;
            continue;
        }

        let mut cursor = fun_start + 3;
        cursor = skip_ascii_whitespace(source, cursor);
        let name_start = cursor;
        while cursor < source.len() && is_identifier_byte(source.as_bytes()[cursor]) {
            cursor += 1;
        }
        if cursor == name_start {
            offset = fun_start + 3;
            continue;
        }
        let function_name = source[name_start..cursor].to_string();
        cursor = skip_ascii_whitespace(source, cursor);
        if source[cursor..].starts_with('<') {
            let Some(after_type_parameters) = skip_balanced(source, cursor, '<', '>') else {
                offset = cursor;
                continue;
            };
            cursor = skip_ascii_whitespace(source, after_type_parameters);
        }
        if !source[cursor..].starts_with('(') {
            offset = cursor;
            continue;
        }
        let Some(params_end) = skip_balanced(source, cursor, '(', ')') else {
            offset = cursor + 1;
            continue;
        };
        let params = &source[cursor + 1..params_end - 1];
        functions.push((function_name, parse_move_parameter_names(params)));
        offset = params_end;
    }

    functions
}

fn parse_move_parameter_names(params: &str) -> Vec<String> {
    split_top_level_commas(params)
        .into_iter()
        .filter_map(|parameter| {
            let parameter = parameter.trim();
            if parameter.is_empty() {
                return None;
            }
            let colon = parameter.find(':')?;
            let name = parameter[..colon].trim();
            let name = name.strip_prefix("mut ").unwrap_or(name).trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut angle_depth = 0u64;
    let mut paren_depth = 0u64;

    for (index, ch) in input.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            ',' if angle_depth == 0 && paren_depth == 0 => {
                segments.push(&input[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    segments.push(&input[start..]);
    segments
}

fn skip_ascii_whitespace(source: &str, mut cursor: usize) -> usize {
    while cursor < source.len() && source.as_bytes()[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn skip_balanced(source: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0u64;
    for (relative, ch) in source[start..].char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(start + relative + ch.len_utf8());
            }
        }
    }
    None
}

fn is_keyword_at(bytes: &[u8], index: usize, keyword: &[u8]) -> bool {
    bytes[index..].starts_with(keyword)
        && index
            .checked_sub(1)
            .map(|prev| !is_identifier_byte(bytes[prev]))
            .unwrap_or(true)
        && bytes
            .get(index + keyword.len())
            .map(|next| !is_identifier_byte(*next))
            .unwrap_or(true)
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::ir::{Function, FunctionParam, NormalizedModule, TypeRef, Visibility},
        std::time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn parses_move_source_parameter_names() {
        let source = strip_move_comments(
            r#"
module nexus_workflow::execution_settlement;

// fun ignored(arg0: u64) {}
    public fun record_committed_tool_result_gas_charge_by_leader(
    execution: &mut DAGExecution,
    leader_cap: &CloneableOwnerCap<leader_cap::OverNetwork>,
    mut walk_index: u64,
    commit_tx_digest: vector<u8>,
    commit_gas_charge: u64,
    settlement_gas_charge: u64,
) {}
"#,
        );

        assert_eq!(
            parse_move_module_name(&source).as_deref(),
            Some("execution_settlement")
        );
        let functions = parse_move_function_parameter_names(&source);
        assert_eq!(functions.len(), 1);
        assert_eq!(
            functions[0].0,
            "record_committed_tool_result_gas_charge_by_leader"
        );
        assert_eq!(
            functions[0].1,
            vec![
                "execution",
                "leader_cap",
                "walk_index",
                "commit_tx_digest",
                "commit_gas_charge",
                "settlement_gas_charge"
            ]
        );
    }

    #[test]
    fn applies_source_parameter_names_to_package() {
        let temp = std::env::temp_dir().join(format!(
            "sui-move-codegen-source-names-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        let source_dir = temp.join("sources");
        std::fs::create_dir_all(&source_dir).expect("create source dir");
        std::fs::write(
            source_dir.join("execution_settlement.move"),
            r#"
module nexus_workflow::execution_settlement;

public fun record_committed_tool_result_gas_charge_by_leader(
    execution: &mut DAGExecution,
    leader_cap: &CloneableOwnerCap<leader_cap::OverNetwork>,
    walk_index: u64,
) {}
"#,
        )
        .expect("write source");

        let mut package = NormalizedPackage {
            storage_id: "0xa4".to_string(),
            original_id: None,
            version: 1,
            modules: BTreeMap::from([(
                "execution_settlement".to_string(),
                NormalizedModule {
                    name: "execution_settlement".to_string(),
                    datatypes: vec![],
                    functions: vec![Function {
                        name: "record_committed_tool_result_gas_charge_by_leader".to_string(),
                        visibility: Visibility::Public,
                        is_entry: false,
                        type_parameters: vec![],
                        parameters: vec![
                            FunctionParam {
                                name: "arg0".to_string(),
                                ty: TypeRef::U64,
                            },
                            FunctionParam {
                                name: "arg1".to_string(),
                                ty: TypeRef::U64,
                            },
                            FunctionParam {
                                name: "arg2".to_string(),
                                ty: TypeRef::U64,
                            },
                        ],
                        return_types: vec![],
                    }],
                },
            )]),
        };

        apply_function_parameter_names_from_sources(&mut package, &source_dir)
            .expect("apply names");

        let parameters = &package.modules["execution_settlement"].functions[0].parameters;
        assert_eq!(parameters[0].name, "execution");
        assert_eq!(parameters[1].name, "leader_cap");
        assert_eq!(parameters[2].name, "walk_index");
        std::fs::remove_dir_all(temp).expect("remove temp dir");
    }
}
