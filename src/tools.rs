use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as FmtWrite;
use std::path::Path;

use ast_grep_language::{LanguageExt, SupportLang};
use ignore::WalkBuilder;
use serde_json::{Map, Value};

use crate::store::CodebaseStore;

const P: &str = "http://repo.example.org/";

// --- Pre-built SPARQL query tools ---

pub fn find_symbol(store: &CodebaseStore, name: &str) -> Result<Value, String> {
    let sparql = format!(
        r#"SELECT ?subject ?type WHERE {{
            ?subject <{P}a> ?type .
            FILTER(CONTAINS(STR(?subject), "{name}"))
        }}"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn find_callers(store: &CodebaseStore, function_name: &str) -> Result<Value, String> {
    let sparql = format!(
        r#"SELECT ?caller WHERE {{
            ?caller <{P}calls> ?callee .
            FILTER(CONTAINS(STR(?callee), "{function_name}"))
        }}"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn find_callees(store: &CodebaseStore, function_name: &str) -> Result<Value, String> {
    let sparql = format!(
        r#"SELECT ?callee WHERE {{
            ?caller <{P}calls> ?callee .
            FILTER(CONTAINS(STR(?caller), "{function_name}"))
        }}"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn list_structures(
    store: &CodebaseStore,
    path_filter: Option<&str>,
    kind_filter: Option<&str>,
) -> Result<Value, String> {
    let mut filters = Vec::new();
    if let Some(p) = path_filter {
        filters.push(format!(r#"FILTER(CONTAINS(STR(?subject), "{p}"))"#));
    }
    if let Some(k) = kind_filter {
        filters.push(format!(r#"FILTER(STR(?type) = "{P}{k}")"#));
    }
    let filter_clause = filters.join("\n            ");
    let sparql = format!(
        r#"SELECT ?subject ?type WHERE {{
            ?subject <{P}a> ?type .
            {filter_clause}
        }} ORDER BY ?subject"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn file_stats(store: &CodebaseStore) -> Result<Value, String> {
    let sparql = format!(
        r#"SELECT ?type (COUNT(?subject) AS ?count) WHERE {{
            ?subject <{P}a> ?type .
        }} GROUP BY ?type ORDER BY DESC(?count)"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn find_dead_code(store: &CodebaseStore) -> Result<Value, String> {
    // Get all functions and all call targets, diff in Rust
    let funcs_sparql = format!(
        r#"SELECT ?func WHERE {{ ?func <{P}a> <{P}Function> }} ORDER BY ?func"#
    );
    let calls_sparql = format!(
        r#"SELECT DISTINCT ?callee WHERE {{ ?caller <{P}calls> ?callee }}"#
    );

    let funcs = store.query_to_json(&funcs_sparql).map_err(|e| e.to_string())?;
    let calls = store.query_to_json(&calls_sparql).map_err(|e| e.to_string())?;

    let call_targets: std::collections::HashSet<String> = calls
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|row| {
            row.get("callee")
                .and_then(|v| v.as_str())
                .map(|s| s.trim_start_matches(&format!("<{P}")).trim_end_matches('>').to_string())
        })
        .collect();

    let dead: Vec<Value> = funcs
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter(|row| {
            if let Some(func_iri) = row.get("func").and_then(|v| v.as_str()) {
                // Extract the short name after last /
                let short_name = func_iri
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('>');
                // Not called if no call target contains this name
                !call_targets.contains(short_name)
            } else {
                false
            }
        })
        .cloned()
        .collect();

    Ok(Value::Array(dead))
}

pub fn find_dependencies(store: &CodebaseStore) -> Result<Value, String> {
    let sparql = format!(
        r#"SELECT ?file ?dependency WHERE {{
            ?file <{P}dependsOn> ?dependency .
        }} ORDER BY ?file ?dependency"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn find_entry_points(store: &CodebaseStore) -> Result<Value, String> {
    // Look for common entry point patterns: main functions, index files, app files
    let sparql = format!(
        r#"SELECT ?entry ?type WHERE {{
            ?entry <{P}a> ?type .
            FILTER(
                CONTAINS(STR(?entry), "/main") ||
                CONTAINS(STR(?entry), "/index") ||
                CONTAINS(STR(?entry), "/app") ||
                CONTAINS(STR(?entry), "/server") ||
                CONTAINS(STR(?entry), "/cli") ||
                CONTAINS(STR(?entry), "/cmd")
            )
        }} ORDER BY ?entry"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

// --- Practice description ---

pub fn describe_practices(store: &CodebaseStore) -> Result<Value, String> {
    let categories = &[
        ("ci_cd", "usesCIPlatform"),
        ("testing", "usesTestFramework"),
        ("linting", "usesLinter"),
        ("formatting", "usesFormatter"),
        ("build_tools", "usesBuildTool"),
        ("containerization", "usesContainerization"),
        ("package_managers", "usesPackageManager"),
        ("type_checking", "usesTypeChecking"),
        ("architecture_layers", "hasLayer"),
        ("documentation", "hasDocumentation"),
        ("conventions", "followsConvention"),
    ];

    let mut result = Map::new();

    for (label, predicate) in categories {
        let sparql = format!(
            r#"SELECT ?value WHERE {{
                <{P}project> <{P}{predicate}> ?value .
            }} ORDER BY ?value"#
        );
        let rows = store.query_to_json(&sparql).map_err(|e| e.to_string())?;
        let values: Vec<Value> = rows
            .as_array()
            .map_or(&[] as &[Value], |v| v.as_slice())
            .iter()
            .filter_map(|row| {
                row.get("value")
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        Value::String(
                            s.strip_prefix(&format!("<{P}"))
                                .and_then(|s| s.strip_suffix('>'))
                                .unwrap_or(s)
                                .to_string(),
                        )
                    })
            })
            .collect();
        if !values.is_empty() {
            result.insert(label.to_string(), Value::Array(values));
        }
    }

    Ok(Value::Object(result))
}

/// Helper: query practice values for a single predicate.
fn query_practice_values(store: &CodebaseStore, predicate: &str) -> Result<Vec<String>, String> {
    let sparql = format!(
        r#"SELECT ?value WHERE {{
            <{P}project> <{P}{predicate}> ?value .
        }} ORDER BY ?value"#
    );
    let rows = store.query_to_json(&sparql).map_err(|e| e.to_string())?;
    Ok(rows
        .as_array()
        .map_or(&[] as &[Value], |v| v.as_slice())
        .iter()
        .filter_map(|row| {
            row.get("value")
                .and_then(|v| v.as_str())
                .map(|s| {
                    s.strip_prefix(&format!("<{P}"))
                        .and_then(|s| s.strip_suffix('>'))
                        .unwrap_or(s)
                        .to_string()
                })
        })
        .collect())
}

/// Helper: count structures of a given type, optionally filtered by path.
fn count_type(store: &CodebaseStore, kind: &str, path_filter: Option<&str>) -> Result<usize, String> {
    let mut filter = String::new();
    if let Some(p) = path_filter {
        filter = format!(r#"FILTER(CONTAINS(STR(?s), "{p}"))"#);
    }
    let sparql = format!(
        r#"SELECT (COUNT(?s) AS ?count) WHERE {{
            ?s <{P}a> <{P}{kind}> .
            {filter}
        }}"#
    );
    let rows = store.query_to_json(&sparql).map_err(|e| e.to_string())?;
    Ok(rows
        .as_array()
        .and_then(|a| a.first())
        .and_then(|r| r.get("count"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim_matches('"').parse::<usize>().ok())
        .unwrap_or(0))
}

pub fn describe_testing(store: &CodebaseStore) -> Result<Value, String> {
    let frameworks = query_practice_values(store, "usesTestFramework")?;
    let total_functions = count_type(store, "Function", None)?;
    let test_functions = count_type(store, "Function", Some("test"))?;
    let spec_functions = count_type(store, "Function", Some("spec"))?;
    let test_related = test_functions + spec_functions;

    let mut result = Map::new();
    result.insert("frameworks".into(), Value::Array(frameworks.into_iter().map(Value::String).collect()));
    result.insert("total_functions".into(), Value::Number(total_functions.into()));
    result.insert("test_functions".into(), Value::Number(test_related.into()));
    if total_functions > 0 {
        let ratio = (test_related as f64 / total_functions as f64 * 100.0).round() as u64;
        result.insert("test_ratio_percent".into(), Value::Number(ratio.into()));
    }
    Ok(Value::Object(result))
}

pub fn describe_ci_cd(store: &CodebaseStore) -> Result<Value, String> {
    let platforms = query_practice_values(store, "usesCIPlatform")?;
    let containerization = query_practice_values(store, "usesContainerization")?;
    let build_tools = query_practice_values(store, "usesBuildTool")?;
    let has_infra = query_practice_values(store, "hasLayer")?
        .iter()
        .any(|l| l == "infrastructure");

    let mut result = Map::new();
    result.insert("ci_platforms".into(), Value::Array(platforms.into_iter().map(Value::String).collect()));
    result.insert("containerization".into(), Value::Array(containerization.into_iter().map(Value::String).collect()));
    result.insert("build_tools".into(), Value::Array(build_tools.into_iter().map(Value::String).collect()));
    result.insert("has_infrastructure_as_code".into(), Value::Bool(has_infra));
    Ok(Value::Object(result))
}

pub fn describe_code_quality(store: &CodebaseStore) -> Result<Value, String> {
    let linters = query_practice_values(store, "usesLinter")?;
    let formatters = query_practice_values(store, "usesFormatter")?;
    let type_checkers = query_practice_values(store, "usesTypeChecking")?;
    let conventions = query_practice_values(store, "followsConvention")?;

    let mut result = Map::new();
    result.insert("linters".into(), Value::Array(linters.into_iter().map(Value::String).collect()));
    result.insert("formatters".into(), Value::Array(formatters.into_iter().map(Value::String).collect()));
    result.insert("type_checkers".into(), Value::Array(type_checkers.into_iter().map(Value::String).collect()));
    result.insert("conventions".into(), Value::Array(conventions.into_iter().map(Value::String).collect()));
    Ok(Value::Object(result))
}

pub fn describe_architecture(store: &CodebaseStore) -> Result<Value, String> {
    let layers = query_practice_values(store, "hasLayer")?;
    let pkg_managers = query_practice_values(store, "usesPackageManager")?;

    let function_count = count_type(store, "Function", None)?;
    let class_count = count_type(store, "Class", None)?;
    let config_count = count_type(store, "Config", None)?;
    let doc_count = count_type(store, "Document", None)?;
    let binary_count = count_type(store, "Binary", None)?;
    let is_monorepo = layers.iter().any(|l| l == "monorepo-packages");

    let mut result = Map::new();
    result.insert("layers".into(), Value::Array(layers.into_iter().map(Value::String).collect()));
    result.insert("package_managers".into(), Value::Array(pkg_managers.into_iter().map(Value::String).collect()));
    result.insert("is_monorepo".into(), Value::Bool(is_monorepo));

    let mut counts = Map::new();
    counts.insert("functions".into(), Value::Number(function_count.into()));
    counts.insert("classes".into(), Value::Number(class_count.into()));
    counts.insert("config_files".into(), Value::Number(config_count.into()));
    counts.insert("documents".into(), Value::Number(doc_count.into()));
    counts.insert("binary_assets".into(), Value::Number(binary_count.into()));
    result.insert("counts".into(), Value::Object(counts));

    Ok(Value::Object(result))
}

pub fn describe_documentation(store: &CodebaseStore) -> Result<Value, String> {
    let docs = query_practice_values(store, "hasDocumentation")?;
    let has_docs_layer = query_practice_values(store, "hasLayer")?
        .iter()
        .any(|l| l == "documentation");
    let doc_file_count = count_type(store, "Document", None)?;
    let section_count = count_type(store, "Section", None)?;

    let mut result = Map::new();
    result.insert("documentation_artifacts".into(), Value::Array(docs.into_iter().map(Value::String).collect()));
    result.insert("has_docs_directory".into(), Value::Bool(has_docs_layer));
    result.insert("document_files".into(), Value::Number(doc_file_count.into()));
    result.insert("total_sections".into(), Value::Number(section_count.into()));
    Ok(Value::Object(result))
}

pub fn describe_dependencies(store: &CodebaseStore) -> Result<Value, String> {
    let pkg_managers = query_practice_values(store, "usesPackageManager")?;
    let has_auto_updates = query_practice_values(store, "followsConvention")?
        .iter()
        .any(|c| c == "automated-dependency-updates");

    // Count declared dependencies
    let dep_sparql = format!(
        r#"SELECT (COUNT(?dep) AS ?count) WHERE {{
            ?file <{P}dependsOn> ?dep .
        }}"#
    );
    let dep_rows = store.query_to_json(&dep_sparql).map_err(|e| e.to_string())?;
    let dep_count = dep_rows
        .as_array()
        .and_then(|a| a.first())
        .and_then(|r| r.get("count"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim_matches('"').parse::<usize>().ok())
        .unwrap_or(0);

    let mut result = Map::new();
    result.insert("package_managers".into(), Value::Array(pkg_managers.into_iter().map(Value::String).collect()));
    result.insert("declared_dependencies".into(), Value::Number(dep_count.into()));
    result.insert("has_automated_updates".into(), Value::Bool(has_auto_updates));
    Ok(Value::Object(result))
}

// --- Live ast-grep pattern search ---

pub fn search_pattern(root: &Path, pattern: &str, language: &str, limit: usize) -> Result<Value, String> {
    let lang: SupportLang = language
        .parse()
        .map_err(|_| format!("unsupported language: {language}"))?;

    let extensions: &[&str] = match language {
        "python" | "py" => &["py"],
        "rust" | "rs" => &["rs"],
        "javascript" | "js" => &["js", "mjs", "cjs"],
        "typescript" | "ts" => &["ts"],
        "tsx" => &["tsx"],
        "go" => &["go"],
        "java" => &["java"],
        "c" => &["c"],
        "cpp" | "c++" => &["cpp", "cc", "cxx"],
        "csharp" | "c#" | "cs" => &["cs"],
        "ruby" | "rb" => &["rb"],
        "php" => &["php"],
        "kotlin" | "kt" => &["kt", "kts"],
        "swift" => &["swift"],
        "scala" => &["scala", "sc"],
        "bash" | "sh" => &["sh", "bash"],
        "lua" => &["lua"],
        _ => return Err(format!("unsupported language: {language}")),
    };

    let mut results = Vec::new();

    for entry in WalkBuilder::new(root).hidden(true).build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };
        if !extensions.contains(&ext) {
            continue;
        }
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let grep = lang.ast_grep(&source);
        let root_node = grep.root();

        for m in root_node.find_all(pattern) {
            let mut entry = Map::new();
            entry.insert("file".to_string(), Value::String(path.to_string_lossy().to_string()));
            entry.insert("line".to_string(), Value::Number((m.start_pos().line() + 1).into()));
            entry.insert("text".to_string(), Value::String(m.text().to_string()));
            results.push(Value::Object(entry));

            if results.len() >= limit {
                return Ok(Value::Array(results));
            }
        }
    }

    Ok(Value::Array(results))
}

// --- LikeC4 diagram generation ---

/// Convert a string to a valid LikeC4 identifier (alphanumeric + underscore).
fn to_id(s: &str) -> String {
    let id: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    // Ensure it starts with a letter (LikeC4 requirement)
    if id.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{id}")
    } else {
        id
    }
}

/// Strip the repo IRI prefix, returning just the local name.
fn strip_iri(s: &str) -> &str {
    s.strip_prefix(&format!("<{P}"))
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(s)
}

fn tech_from_ext(file: &str) -> Option<&'static str> {
    let ext = file.rsplit('.').next()?;
    match ext {
        "rs" => Some("Rust"), "py" => Some("Python"),
        "js" | "mjs" | "cjs" => Some("JavaScript"),
        "ts" => Some("TypeScript"), "tsx" => Some("TSX"),
        "go" => Some("Go"), "java" => Some("Java"),
        "c" | "h" => Some("C"), "cpp" | "cc" | "cxx" | "hpp" => Some("C++"),
        "cs" => Some("C#"), "rb" => Some("Ruby"), "php" => Some("PHP"),
        "kt" | "kts" => Some("Kotlin"), "swift" => Some("Swift"),
        "scala" | "sc" => Some("Scala"), "sh" | "bash" => Some("Bash"),
        "lua" => Some("Lua"), "json" => Some("JSON"),
        "yml" | "yaml" => Some("YAML"), "md" | "markdown" => Some("Markdown"),
        "html" | "htm" => Some("HTML"), "css" => Some("CSS"),
        _ => None,
    }
}

pub fn generate_diagram(
    store: &CodebaseStore,
    scope: Option<&str>,
    depth: usize,
    code_only: bool,
    exclude: &[String],
    limit: usize,
) -> std::result::Result<String, String> {
    // 1. Query all structures
    let mut filters = Vec::new();
    if let Some(p) = scope {
        filters.push(format!(r#"FILTER(CONTAINS(STR(?subject), "{p}"))"#));
    }
    if code_only {
        // Only include Function and Class types
        filters.push(format!(
            r#"FILTER(?type IN (<{P}Function>, <{P}Class>))"#
        ));
        // Exclude common non-source directories
        for dir in &["/docs/", "/doc/", "/images/", "/stories/", "/__tests__/",
                     "/test/", "/tests/", "/spec/", "/fixtures/", "/examples/",
                     "/__mocks__/", "/__snapshots__/", "/coverage/", "/dist/",
                     "/build/", "/node_modules/", "/.storybook/"] {
            filters.push(format!(
                r#"FILTER(!CONTAINS(STR(?subject), "{dir}"))"#
            ));
        }
    }
    // User-specified directory exclusions
    for dir in exclude {
        filters.push(format!(
            r#"FILTER(!CONTAINS(STR(?subject), "/{dir}/"))"#
        ));
    }
    let filter_clause = filters.join("\n            ");
    let structs_sparql = format!(
        r#"SELECT ?subject ?type WHERE {{
            ?subject <{P}a> ?type .
            {filter_clause}
        }} ORDER BY ?subject"#
    );
    let structs = store.query_to_json(&structs_sparql).map_err(|e| e.to_string())?;

    // 2. Query call relationships
    let calls_sparql = format!(
        r#"SELECT ?caller ?callee WHERE {{ ?caller <{P}calls> ?callee . }}"#
    );
    let calls = store.query_to_json(&calls_sparql).map_err(|e| e.to_string())?;

    // 3. Query dependencies
    let deps_sparql = format!(
        r#"SELECT ?file ?dep WHERE {{ ?file <{P}dependsOn> ?dep . }}"#
    );
    let deps = store.query_to_json(&deps_sparql).map_err(|e| e.to_string())?;

    // 4. Parse structures into a directory tree
    struct FileEntry {
        symbols: Vec<(String, String)>, // (name, kind: "func" | "cls")
    }
    let mut dirs: BTreeMap<String, BTreeMap<String, FileEntry>> = BTreeMap::new();
    // Maps raw subject path → dot-separated LikeC4 ID
    let mut id_map: BTreeMap<String, String> = BTreeMap::new();
    // Maps short symbol name → list of full dot IDs (for callee resolution)
    let mut callee_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut element_count = 0;

    let rows = structs.as_array().map_or(&[] as &[Value], |v| v.as_slice());
    for row in rows {
        if element_count >= limit {
            break;
        }
        let subject = strip_iri(row.get("subject").and_then(|v| v.as_str()).unwrap_or(""));
        let kind_iri = strip_iri(row.get("type").and_then(|v| v.as_str()).unwrap_or(""));

        let kind = match kind_iri {
            "Function" => "func",
            "Class" => "cls",
            "Config" | "Document" | "Stylesheet" | "Binary" => {
                if code_only {
                    continue;
                }
                if depth < 2 {
                    if let Some(dir) = subject.rsplit('/').nth(1).or(Some(".")) {
                        dirs.entry(dir.to_string()).or_default();
                    }
                    element_count += 1;
                    continue;
                }
                let (dir, file) = split_path(subject);
                let dir_id = to_id(&dir);
                let file_id = to_id(file);
                let full_id = format!("{dir_id}.{file_id}");
                id_map.insert(subject.to_string(), full_id);
                dirs.entry(dir)
                    .or_default()
                    .entry(file.to_string())
                    .or_insert_with(|| FileEntry { symbols: vec![] });
                element_count += 1;
                continue;
            }
            _ => continue,
        };

        // subject is like "path/to/file.rs/func_name"
        let parts: Vec<&str> = subject.rsplitn(2, '/').collect();
        if parts.len() < 2 {
            continue;
        }
        let symbol_name = parts[0];
        let file_path = parts[1];
        let (dir, file) = split_path(file_path);

        let dir_id = to_id(&dir);

        if depth < 2 {
            dirs.entry(dir.clone()).or_default();
            // At depth 1, map to directory level
            id_map.insert(subject.to_string(), dir_id.clone());
            element_count += 1;
            continue;
        }

        let file_id = to_id(file);
        let full_file_id = format!("{dir_id}.{file_id}");

        let file_entry = dirs
            .entry(dir)
            .or_default()
            .entry(file.to_string())
            .or_insert_with(|| FileEntry { symbols: vec![] });

        if depth >= 3 {
            let sym_id = to_id(symbol_name);
            let full_sym_id = format!("{full_file_id}.{sym_id}");
            id_map.insert(subject.to_string(), full_sym_id.clone());
            callee_index
                .entry(symbol_name.to_string())
                .or_default()
                .push(full_sym_id);
            file_entry.symbols.push((symbol_name.to_string(), kind.to_string()));
        } else {
            // At depth 2, map symbols to their file
            id_map.insert(subject.to_string(), full_file_id.clone());
            callee_index
                .entry(symbol_name.to_string())
                .or_default()
                .push(full_file_id.clone());
        }
        element_count += 1;
    }

    // 5. Build LikeC4 DSL
    let mut out = String::new();

    // Specification
    writeln!(out, "specification {{").unwrap();
    writeln!(out, "  element module {{").unwrap();
    writeln!(out, "    style {{ shape rectangle }}").unwrap();
    writeln!(out, "  }}").unwrap();
    if depth >= 2 {
        writeln!(out, "  element file").unwrap();
    }
    if depth >= 3 {
        writeln!(out, "  element func").unwrap();
        writeln!(out, "  element cls").unwrap();
    }
    writeln!(out, "  element external {{").unwrap();
    writeln!(out, "    style {{ color muted }}").unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "  relationship calls").unwrap();
    writeln!(out, "  relationship dependsOn").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Model
    writeln!(out, "model {{").unwrap();

    let mut top_level_dirs: Vec<String> = Vec::new();

    for (dir, files) in &dirs {
        let dir_id = to_id(dir);
        top_level_dirs.push(dir_id.clone());
        let file_count = files.len();
        let func_count: usize = files.values().map(|f| f.symbols.len()).sum();

        writeln!(out, "  module {} '{}' {{", dir_id, dir).unwrap();
        writeln!(
            out,
            "    description '{} files, {} symbols'",
            file_count, func_count
        )
        .unwrap();

        if depth >= 2 {
            for (file, entry) in files {
                let file_id = to_id(file);
                if depth >= 3 && !entry.symbols.is_empty() {
                    write!(out, "    file {} '{}'", file_id, file).unwrap();
                    if let Some(tech) = tech_from_ext(file) {
                        writeln!(out, " {{").unwrap();
                        writeln!(out, "      technology '{}'", tech).unwrap();
                        for (sym_name, sym_kind) in &entry.symbols {
                            let sym_id = to_id(sym_name);
                            writeln!(out, "      {} {} '{}'", sym_kind, sym_id, sym_name).unwrap();
                        }
                        writeln!(out, "    }}").unwrap();
                    } else {
                        writeln!(out, " {{").unwrap();
                        for (sym_name, sym_kind) in &entry.symbols {
                            let sym_id = to_id(sym_name);
                            writeln!(out, "      {} {} '{}'", sym_kind, sym_id, sym_name).unwrap();
                        }
                        writeln!(out, "    }}").unwrap();
                    }
                } else {
                    write!(out, "    file {} '{}'", file_id, file).unwrap();
                    if let Some(tech) = tech_from_ext(file) {
                        writeln!(out, " {{").unwrap();
                        writeln!(out, "      technology '{}'", tech).unwrap();
                        writeln!(out, "    }}").unwrap();
                    } else {
                        writeln!(out).unwrap();
                    }
                }
            }
        }

        writeln!(out, "  }}").unwrap();
    }

    // External dependencies as top-level elements
    let mut external_ids: BTreeSet<String> = BTreeSet::new();
    let dep_rows = deps.as_array().map_or(&[] as &[Value], |v| v.as_slice());
    for row in dep_rows {
        let dep = strip_iri(row.get("dep").and_then(|v| v.as_str()).unwrap_or(""));
        if !dep.is_empty() {
            let dep_id = to_id(dep);
            if external_ids.insert(dep_id.clone()) {
                writeln!(out, "  external {} '{}'", dep_id, dep).unwrap();
            }
        }
    }

    writeln!(out).unwrap();

    // Relationships — calls
    let call_rows = calls.as_array().map_or(&[] as &[Value], |v| v.as_slice());
    let mut rel_count = 0;
    let mut emitted_rels: BTreeSet<(String, String)> = BTreeSet::new();

    for row in call_rows {
        if rel_count >= limit {
            break;
        }
        let caller_raw = strip_iri(row.get("caller").and_then(|v| v.as_str()).unwrap_or(""));
        let callee_raw = strip_iri(row.get("callee").and_then(|v| v.as_str()).unwrap_or(""));

        // Look up caller in id_map
        let caller_id = match id_map.get(caller_raw) {
            Some(id) => id.clone(),
            None => continue,
        };

        // Look up callee by short name in callee_index
        let callee_ids = match callee_index.get(callee_raw) {
            Some(ids) => ids.clone(),
            None => continue,
        };

        for callee_id in callee_ids {
            if caller_id == callee_id {
                continue; // skip self-calls
            }
            let rel_key = (caller_id.clone(), callee_id.clone());
            if emitted_rels.insert(rel_key) {
                writeln!(out, "  {} -[calls]-> {} 'calls'", caller_id, callee_id).unwrap();
                rel_count += 1;
                if rel_count >= limit {
                    break;
                }
            }
        }
    }

    // Relationships — dependencies
    for row in dep_rows {
        if rel_count >= limit {
            break;
        }
        let file_raw = strip_iri(row.get("file").and_then(|v| v.as_str()).unwrap_or(""));
        let dep = strip_iri(row.get("dep").and_then(|v| v.as_str()).unwrap_or(""));

        let file_id = match id_map.get(file_raw) {
            Some(id) => id.clone(),
            None => continue,
        };
        let dep_id = to_id(dep);

        let rel_key = (file_id.clone(), dep_id.clone());
        if emitted_rels.insert(rel_key) {
            writeln!(out, "  {} -[dependsOn]-> {} 'depends on'", file_id, dep_id).unwrap();
            rel_count += 1;
        }
    }

    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Views
    writeln!(out, "views {{").unwrap();
    writeln!(out, "  view index {{").unwrap();
    writeln!(out, "    title 'Codebase Architecture'").unwrap();
    writeln!(out, "    include *").unwrap();
    writeln!(out, "  }}").unwrap();

    // Scoped views per top-level directory
    for dir_id in &top_level_dirs {
        writeln!(out, "  view of {} {{", dir_id).unwrap();
        writeln!(out, "    include *").unwrap();
        writeln!(out, "  }}").unwrap();
    }

    writeln!(out, "}}").unwrap();

    Ok(out)
}

fn split_path(path: &str) -> (String, &str) {
    if let Some(pos) = path.rfind('/') {
        let dir = &path[..pos];
        let file = &path[pos + 1..];
        let dir_short = dir
            .rsplit('/')
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("/");
        (dir_short, file)
    } else {
        (".".to_string(), path)
    }
}
