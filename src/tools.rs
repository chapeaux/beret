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

// --- Live ast-grep pattern search ---

pub fn search_pattern(root: &Path, pattern: &str, language: &str) -> Result<Value, String> {
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

            if results.len() >= 200 {
                return Ok(Value::Array(results));
            }
        }
    }

    Ok(Value::Array(results))
}
