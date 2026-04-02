use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as FmtWrite;
use std::path::Path;

use ast_grep_language::{LanguageExt, SupportLang};
use ignore::WalkBuilder;
use serde_json::{Map, Value};

use crate::store::CodebaseStore;

const P: &str = "http://repo.example.org/";

/// Build SPARQL FILTER clauses to exclude directories from a given variable.
fn exclude_filters(var: &str, exclude: &[String]) -> String {
    exclude
        .iter()
        .map(|dir| format!(r#"FILTER(!CONTAINS(STR(?{var}), "/{dir}/"))"#))
        .collect::<Vec<_>>()
        .join("\n            ")
}

// --- Pre-built SPARQL query tools ---

pub fn find_symbol(store: &CodebaseStore, name: &str, exclude: &[String]) -> Result<Value, String> {
    let excl = exclude_filters("subject", exclude);
    let sparql = format!(
        r#"SELECT ?subject ?type WHERE {{
            ?subject <{P}a> ?type .
            FILTER(CONTAINS(STR(?subject), "{name}"))
            {excl}
        }}"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn find_callers(store: &CodebaseStore, function_name: Option<&str>, exclude: &[String]) -> Result<Value, String> {
    let excl = exclude_filters("caller", exclude);
    let name_filter = function_name
        .map(|n| format!(r#"FILTER(CONTAINS(STR(?callee), "{n}"))"#))
        .unwrap_or_default();
    let sparql = format!(
        r#"SELECT ?caller ?callee WHERE {{
            ?caller <{P}calls> ?callee .
            {name_filter}
            {excl}
        }}"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn find_callees(store: &CodebaseStore, function_name: Option<&str>, exclude: &[String]) -> Result<Value, String> {
    let excl = exclude_filters("caller", exclude);
    let name_filter = function_name
        .map(|n| format!(r#"FILTER(CONTAINS(STR(?caller), "{n}"))"#))
        .unwrap_or_default();
    let sparql = format!(
        r#"SELECT ?caller ?callee WHERE {{
            ?caller <{P}calls> ?callee .
            {name_filter}
            {excl}
        }}"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn list_structures(
    store: &CodebaseStore,
    path_filter: Option<&str>,
    kind_filter: Option<&str>,
    exclude: &[String],
) -> Result<Value, String> {
    let mut filters = Vec::new();
    if let Some(p) = path_filter {
        filters.push(format!(r#"FILTER(CONTAINS(STR(?subject), "{p}"))"#));
    }
    if let Some(k) = kind_filter {
        filters.push(format!(r#"FILTER(STR(?type) = "{P}{k}")"#));
    }
    let excl = exclude_filters("subject", exclude);
    if !excl.is_empty() {
        filters.push(excl);
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

pub fn file_stats(store: &CodebaseStore, exclude: &[String]) -> Result<Value, String> {
    let excl = exclude_filters("subject", exclude);
    let sparql = format!(
        r#"SELECT ?type (COUNT(?subject) AS ?count) WHERE {{
            ?subject <{P}a> ?type .
            {excl}
        }} GROUP BY ?type ORDER BY DESC(?count)"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn find_dead_code(store: &CodebaseStore, exclude: &[String]) -> Result<Value, String> {
    let excl = exclude_filters("func", exclude);
    let funcs_sparql = format!(
        r#"SELECT ?func WHERE {{
            ?func <{P}a> <{P}Function> .
            {excl}
        }} ORDER BY ?func"#
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
                let short_name = func_iri
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('>');
                !call_targets.contains(short_name)
            } else {
                false
            }
        })
        .cloned()
        .collect();

    Ok(Value::Array(dead))
}

pub fn find_dependencies(store: &CodebaseStore, exclude: &[String]) -> Result<Value, String> {
    let excl = exclude_filters("file", exclude);
    let sparql = format!(
        r#"SELECT ?file ?dependency WHERE {{
            ?file <{P}dependsOn> ?dependency .
            {excl}
        }} ORDER BY ?file ?dependency"#
    );
    store.query_to_json(&sparql).map_err(|e| e.to_string())
}

pub fn find_entry_points(store: &CodebaseStore, exclude: &[String]) -> Result<Value, String> {
    let excl = exclude_filters("entry", exclude);
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
            {excl}
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
        ("deployment_platforms", "usesDeploymentPlatform"),
        ("code_analysis", "usesCodeAnalysis"),
        ("packaging_formats", "usesPackagingFormat"),
        ("config_management", "usesConfigManagement"),
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
    let test_deps = detect_test_deps_from_graph(store)?;
    let total_functions = count_type(store, "Function", None)?;
    let test_functions = count_type(store, "Function", Some("test"))?;
    let spec_functions = count_type(store, "Function", Some("spec"))?;
    let test_related = test_functions + spec_functions;

    let mut result = Map::new();
    result.insert("frameworks".into(), Value::Array(frameworks.into_iter().map(Value::String).collect()));
    if !test_deps.is_empty() {
        result.insert("test_dependencies".into(), Value::Array(test_deps.into_iter().map(Value::String).collect()));
    }
    result.insert("total_functions".into(), Value::Number(total_functions.into()));
    result.insert("test_functions".into(), Value::Number(test_related.into()));
    if total_functions > 0 {
        let ratio = (test_related as f64 / total_functions as f64 * 100.0).round() as u64;
        result.insert("test_ratio_percent".into(), Value::Number(ratio.into()));
    }
    Ok(Value::Object(result))
}

/// Detect test framework dependencies from the `dependsOn` graph.
fn detect_test_deps_from_graph(store: &CodebaseStore) -> Result<Vec<String>, String> {
    let sparql = format!(
        r#"SELECT DISTINCT ?dep WHERE {{
            ?file <{P}dependsOn> ?dep .
        }}"#
    );
    let rows = store.query_to_json(&sparql).map_err(|e| e.to_string())?;

    const TEST_PATTERNS: &[(&str, &str)] = &[
        // Java/JVM
        ("testng", "testng"),
        ("junit-jupiter", "junit"),
        (":junit", "junit"),
        ("mockito", "mockito"),
        ("assertj", "assertj"),
        ("hamcrest", "hamcrest"),
        ("cucumber", "cucumber"),
        ("rest-assured", "rest-assured"),
        ("selenium", "selenium"),
        ("arquillian", "arquillian"),
        ("spock", "spock"),
        // JavaScript/TypeScript
        (":jest", "jest"),
        (":vitest", "vitest"),
        (":mocha", "mocha"),
        (":cypress", "cypress"),
        (":playwright", "playwright"),
        // Python
        ("pytest", "pytest"),
        // Ruby
        ("rspec", "rspec"),
        ("minitest", "minitest"),
        // Go
        ("testify", "testify"),
        ("ginkgo", "ginkgo"),
        ("gomega", "gomega"),
    ];

    let mut found: Vec<String> = Vec::new();
    let empty = vec![];
    let dep_rows = rows.as_array().unwrap_or(&empty);

    for row in dep_rows {
        let dep = row
            .get("dep")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let dep_clean = dep
            .strip_prefix(&format!("<{P}"))
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(dep)
            .to_lowercase();

        for &(pattern, label) in TEST_PATTERNS {
            if dep_clean.contains(pattern) && !found.contains(&label.to_string()) {
                found.push(label.to_string());
            }
        }
    }
    Ok(found)
}

pub fn describe_ci_cd(store: &CodebaseStore) -> Result<Value, String> {
    let platforms = query_practice_values(store, "usesCIPlatform")?;
    let containerization = query_practice_values(store, "usesContainerization")?;
    let build_tools = query_practice_values(store, "usesBuildTool")?;
    let deployment_platforms = query_practice_values(store, "usesDeploymentPlatform")?;
    let has_infra = query_practice_values(store, "hasLayer")?
        .iter()
        .any(|l| l == "infrastructure");

    let mut result = Map::new();
    result.insert("ci_platforms".into(), Value::Array(platforms.into_iter().map(Value::String).collect()));
    result.insert("containerization".into(), Value::Array(containerization.into_iter().map(Value::String).collect()));
    result.insert("build_tools".into(), Value::Array(build_tools.into_iter().map(Value::String).collect()));
    if !deployment_platforms.is_empty() {
        result.insert("deployment_platforms".into(), Value::Array(deployment_platforms.into_iter().map(Value::String).collect()));
    }
    result.insert("has_infrastructure_as_code".into(), Value::Bool(has_infra));
    Ok(Value::Object(result))
}

pub fn describe_code_quality(store: &CodebaseStore) -> Result<Value, String> {
    let linters = query_practice_values(store, "usesLinter")?;
    let formatters = query_practice_values(store, "usesFormatter")?;
    let type_checkers = query_practice_values(store, "usesTypeChecking")?;
    let conventions = query_practice_values(store, "followsConvention")?;
    let code_analysis = query_practice_values(store, "usesCodeAnalysis")?;

    let mut result = Map::new();
    result.insert("linters".into(), Value::Array(linters.into_iter().map(Value::String).collect()));
    result.insert("formatters".into(), Value::Array(formatters.into_iter().map(Value::String).collect()));
    result.insert("type_checkers".into(), Value::Array(type_checkers.into_iter().map(Value::String).collect()));
    result.insert("conventions".into(), Value::Array(conventions.into_iter().map(Value::String).collect()));
    if !code_analysis.is_empty() {
        result.insert("code_analysis".into(), Value::Array(code_analysis.into_iter().map(Value::String).collect()));
    }
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

// --- Unified project description ---

pub fn describe_project(store: &CodebaseStore) -> Result<Value, String> {
    let mut result = Map::new();

    result.insert("practices".into(), describe_practices(store)?);
    result.insert("testing".into(), describe_testing(store)?);
    result.insert("ci_cd".into(), describe_ci_cd(store)?);
    result.insert("code_quality".into(), describe_code_quality(store)?);
    result.insert("architecture".into(), describe_architecture(store)?);
    result.insert("documentation".into(), describe_documentation(store)?);
    result.insert("dependencies".into(), describe_dependencies(store)?);

    let insights = generate_insights(&result);
    if !insights.is_empty() {
        result.insert(
            "insights".into(),
            Value::Array(insights.into_iter().map(Value::String).collect()),
        );
    }

    Ok(Value::Object(result))
}

fn generate_insights(data: &Map<String, Value>) -> Vec<String> {
    let mut insights = Vec::new();

    let has_val = |section: &str, key: &str, needle: &str| -> bool {
        data.get(section)
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_array())
            .map_or(false, |arr| {
                arr.iter()
                    .any(|v| v.as_str().map_or(false, |s| s.contains(needle)))
            })
    };
    let get_num = |section: &str, key: &str| -> Option<u64> {
        data.get(section)
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_u64())
    };
    let get_bool = |section: &str, key: &str| -> bool {
        data.get(section)
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    let arr_len = |section: &str, key: &str| -> usize {
        data.get(section)
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_array())
            .map_or(0, |a| a.len())
    };
    let arr_vals = |section: &str, key: &str| -> Vec<String> {
        data.get(section)
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_array())
            .map_or(vec![], |arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
    };

    // Dogfooding: devfile + containerization
    if has_val("code_quality", "conventions", "devfile") {
        if has_val("ci_cd", "containerization", "docker") {
            insights.push(
                "Uses devfile.yaml with containerized builds — the team dogfoods \
                 their own development environment."
                    .into(),
            );
        } else {
            insights.push(
                "Uses devfile.yaml — standardized, reproducible development environment.".into(),
            );
        }
    }

    // Testing maturity
    let fw_count = arr_len("testing", "frameworks") + arr_len("testing", "test_dependencies");
    let test_ratio = get_num("testing", "test_ratio_percent").unwrap_or(0);
    if fw_count > 0 && test_ratio > 20 {
        let all_fws: Vec<String> = arr_vals("testing", "frameworks")
            .into_iter()
            .chain(arr_vals("testing", "test_dependencies"))
            .collect();
        insights.push(format!(
            "{}% test ratio with {} — indicates a solid testing practice.",
            test_ratio,
            all_fws.join(", ")
        ));
    } else if fw_count > 0 && test_ratio > 0 {
        insights.push(format!(
            "Test frameworks detected but test ratio is {}% — testing may be \
             concentrated in specific modules.",
            test_ratio
        ));
    }

    // Unit + integration separation
    if has_val("ci_cd", "build_tools", "maven-surefire")
        && has_val("ci_cd", "build_tools", "maven-failsafe")
    {
        insights.push(
            "Uses Maven Surefire + Failsafe — unit and integration tests are \
             separated with different execution phases."
                .into(),
        );
    }

    // CI + containerization = deployment pipeline
    let ci = arr_vals("ci_cd", "ci_platforms");
    let containers = arr_vals("ci_cd", "containerization");
    if !ci.is_empty() && !containers.is_empty() {
        insights.push(format!(
            "CI via {} with {} — automated build and deployment pipeline.",
            ci.join(", "),
            containers.join(", ")
        ));
    }

    // Open source health
    let doc_artifacts = arr_vals("documentation", "documentation_artifacts");
    let health_signals: Vec<&str> = [
        "contributing-guide",
        "security-policy",
        "code-of-conduct",
        "codeowners",
        "issue-templates",
        "pr-template",
    ]
    .iter()
    .filter(|&&s| doc_artifacts.iter().any(|d| d == s))
    .copied()
    .collect();
    if health_signals.len() >= 3 {
        insights.push(format!(
            "Strong open-source health: {} — set up for community contributions.",
            health_signals.join(", ")
        ));
    }

    // Architecture maturity
    let layers = arr_vals("architecture", "layers");
    if layers.len() >= 5 {
        insights.push(format!(
            "Well-layered architecture with {} layers ({}) — mature, modular codebase.",
            layers.len(),
            layers.join(", ")
        ));
    }

    // Monorepo
    if get_bool("architecture", "is_monorepo") {
        let pms = arr_vals("architecture", "package_managers");
        insights.push(format!(
            "Monorepo structure with {} — multiple packages managed together.",
            pms.join(", ")
        ));
    }

    // Dependency management
    if get_bool("dependencies", "has_automated_updates") {
        insights.push(
            "Uses automated dependency updates (Renovate/Dependabot) — proactive \
             about security patches."
                .into(),
        );
    }

    // Deployment platforms
    let deploy = arr_vals("ci_cd", "deployment_platforms");
    if !deploy.is_empty() {
        insights.push(format!(
            "Deploys to {} — cloud-native deployment target(s).",
            deploy.join(", ")
        ));
    }

    // Code quality tooling
    let linter_count = arr_len("code_quality", "linters");
    let formatter_count = arr_len("code_quality", "formatters");
    let analysis_count = arr_len("code_quality", "code_analysis");
    if linter_count + formatter_count + analysis_count >= 3 {
        let linters = arr_vals("code_quality", "linters");
        let formatters = arr_vals("code_quality", "formatters");
        let analysis = arr_vals("code_quality", "code_analysis");
        let all: Vec<String> = linters
            .into_iter()
            .chain(formatters)
            .chain(analysis)
            .collect();
        insights.push(format!(
            "Strong code quality tooling: {} — enforced code standards.",
            all.join(", ")
        ));
    }

    // Version pinning
    if has_val("code_quality", "conventions", "version-pinning") {
        insights.push(
            "Uses version pinning (.nvmrc, .tool-versions, etc.) — reproducible \
             builds across environments."
                .into(),
        );
    }

    // Linux packaging
    let pkg_formats = arr_vals("practices", "packaging_formats");
    if !pkg_formats.is_empty() {
        insights.push(format!(
            "Packages for {} — distributes via Linux package manager(s).",
            pkg_formats.join(", ")
        ));
    }

    // Config management
    let config_mgmt = arr_vals("practices", "config_management");
    if !config_mgmt.is_empty() {
        insights.push(format!(
            "Uses {} for infrastructure automation.",
            config_mgmt.join(", ")
        ));
    }

    // Red Hat / Fedora ecosystem signals
    if has_val("ci_cd", "ci_platforms", "packit") {
        insights.push(
            "Uses Packit — automated Fedora/CentOS Stream package maintenance.".into(),
        );
    }
    if has_val("code_quality", "conventions", "fedora-gating") {
        insights.push("Has Fedora gating tests — packages gate on CI results.".into());
    }

    // systemd + SELinux = Linux system integration
    if has_val("code_quality", "conventions", "systemd")
        && has_val("code_quality", "conventions", "selinux")
    {
        insights.push(
            "Includes systemd units and SELinux policy — deep Linux system integration.".into(),
        );
    } else if has_val("code_quality", "conventions", "systemd") {
        insights.push("Ships systemd service units — designed for Linux system integration.".into());
    }

    // Operator pattern
    if has_val("code_quality", "conventions", "olm-operator")
        || has_val("code_quality", "conventions", "ansible-operator")
    {
        insights.push(
            "Implements the Kubernetes Operator pattern with OLM lifecycle management.".into(),
        );
    }

    // Cap at 12
    insights.truncate(12);
    insights
}

// --- Live ast-grep pattern search ---

pub fn search_pattern(root: &Path, pattern: &str, language: &str, exclude: &[String], limit: usize) -> Result<Value, String> {
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
        // Skip excluded directories
        if !exclude.is_empty() {
            let path_str = path.to_string_lossy();
            if exclude.iter().any(|dir| path_str.contains(&format!("/{dir}/"))) {
                continue;
            }
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
/// LikeC4 reserved words that cannot be used as identifiers.
const LIKEC4_RESERVED: &[&str] = &[
    // DSL structure keywords
    "specification", "element", "relationship", "tag", "color", "model",
    "views", "view", "style", "extend", "include", "exclude",
    "it", "this", "navigate", "dynamic", "parallel",
    // Element/relationship properties
    "title", "description", "technology", "notation", "metadata", "link",
    "summary", "links",
    // Styling keywords
    "shape", "opacity", "border", "icon", "icons",
    "rectangle", "queue", "person", "cylinder", "storage", "browser", "mobile",
    "autoLayout", "animation",
    // Our element kind names (can't reuse as identifiers)
    "module", "file", "func", "cls", "external",
    // Common codebase names that are LikeC4 keywords or cause conflicts
    "interactive", "async", "default", "import", "export", "where", "of",
    "component", "container", "system", "context", "deployment",
    "content", "media", "table", "utility",
];

fn to_id(s: &str) -> String {
    let id: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    // Ensure it starts with a letter
    let id = if id.starts_with(|c: char| c.is_ascii_digit()) {
        format!("n{id}")
    } else {
        id
    };
    // Escape reserved words with a readable suffix
    if LIKEC4_RESERVED.contains(&id.as_str()) {
        format!("{id}El")
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
    // Auto-depth: if depth is 0, pick based on result count
    let depth = if depth == 0 {
        // Quick count of code structures
        let count_sparql = format!(
            r#"SELECT (COUNT(?s) AS ?count) WHERE {{ ?s <{P}a> ?type . FILTER(?type IN (<{P}Function>, <{P}Class>)) }}"#
        );
        let count = store.query_to_json(&count_sparql)
            .ok()
            .and_then(|v| v.as_array()?.first()?.get("count")?.as_str()?.trim_matches('"').parse::<usize>().ok())
            .unwrap_or(0);
        if count <= 100 { 3 }       // Small: show everything
        else if count <= 500 { 2 }  // Medium: files only
        else { 1 }                  // Large: directories only
    } else {
        depth
    };

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

    // Specification — Beret theme
    writeln!(out, "specification {{").unwrap();

    // Custom Beret palette
    writeln!(out, "  color beret-navy #00005F").unwrap();
    writeln!(out, "  color beret-orange #F5921B").unwrap();
    writeln!(out, "  color beret-gold #FFCC17").unwrap();
    writeln!(out, "  color beret-teal #37A3A3").unwrap();
    writeln!(out, "  color beret-blue #0066CC").unwrap();

    // Module: large translucent navy container
    writeln!(out, "  element module {{").unwrap();
    writeln!(out, "    style {{").unwrap();
    writeln!(out, "      shape rectangle").unwrap();
    writeln!(out, "      color beret-navy").unwrap();
    writeln!(out, "      opacity 10%").unwrap();
    writeln!(out, "      border solid").unwrap();
    writeln!(out, "      size large").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "  }}").unwrap();

    if depth >= 2 {
        // File: blue component shape
        writeln!(out, "  element file {{").unwrap();
        writeln!(out, "    style {{").unwrap();
        writeln!(out, "      shape component").unwrap();
        writeln!(out, "      color beret-blue").unwrap();
        writeln!(out, "      size medium").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "  }}").unwrap();
    }

    if depth >= 3 {
        // Function: small teal rectangle
        writeln!(out, "  element func {{").unwrap();
        writeln!(out, "    style {{").unwrap();
        writeln!(out, "      shape rectangle").unwrap();
        writeln!(out, "      color beret-teal").unwrap();
        writeln!(out, "      size small").unwrap();
        writeln!(out, "      textSize xsmall").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "  }}").unwrap();

        // Class: navy storage shape
        writeln!(out, "  element cls {{").unwrap();
        writeln!(out, "    style {{").unwrap();
        writeln!(out, "      shape storage").unwrap();
        writeln!(out, "      color beret-navy").unwrap();
        writeln!(out, "      size medium").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "  }}").unwrap();
    }

    // External dependency: gold, dashed border
    writeln!(out, "  element external {{").unwrap();
    writeln!(out, "    style {{").unwrap();
    writeln!(out, "      shape cylinder").unwrap();
    writeln!(out, "      color beret-gold").unwrap();
    writeln!(out, "      border dashed").unwrap();
    writeln!(out, "      size small").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "  }}").unwrap();

    // Relationship styles
    writeln!(out, "  relationship calls {{").unwrap();
    writeln!(out, "    color beret-orange").unwrap();
    writeln!(out, "    line solid").unwrap();
    writeln!(out, "    head normal").unwrap();
    writeln!(out, "  }}").unwrap();
    writeln!(out, "  relationship dependsOn {{").unwrap();
    writeln!(out, "    color beret-gold").unwrap();
    writeln!(out, "    line dashed").unwrap();
    writeln!(out, "    head diamond").unwrap();
    writeln!(out, "  }}").unwrap();

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

    // Relationships — collect per source element, emit inside parent via extend
    let call_rows = calls.as_array().map_or(&[] as &[Value], |v| v.as_slice());
    let mut rel_count = 0;
    let mut emitted_rels: BTreeSet<(String, String)> = BTreeSet::new();
    // Group relationships by the source element's parent module
    // key = parent module ID, value = vec of "child -> target 'label'" lines
    let mut rels_by_parent: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for row in call_rows {
        if rel_count >= limit {
            break;
        }
        let caller_raw = strip_iri(row.get("caller").and_then(|v| v.as_str()).unwrap_or(""));
        let callee_raw = strip_iri(row.get("callee").and_then(|v| v.as_str()).unwrap_or(""));

        let caller_id = match id_map.get(caller_raw) {
            Some(id) => id.clone(),
            None => continue,
        };
        let callee_ids = match callee_index.get(callee_raw) {
            Some(ids) => ids.clone(),
            None => continue,
        };

        for callee_id in callee_ids {
            if caller_id == callee_id {
                continue;
            }
            let rel_key = (caller_id.clone(), callee_id.clone());
            if emitted_rels.insert(rel_key) {
                // Find the parent module of the caller (first segment of dot ID)
                let parent = caller_id.split('.').next().unwrap_or(&caller_id).to_string();
                // Use relative ID within the parent for the caller
                let caller_rel = if let Some(rest) = caller_id.strip_prefix(&format!("{parent}.")) {
                    rest.to_string()
                } else {
                    caller_id.clone()
                };
                rels_by_parent
                    .entry(parent)
                    .or_default()
                    .push(format!("{caller_rel} -[calls]-> {callee_id} 'calls'"));
                rel_count += 1;
                if rel_count >= limit {
                    break;
                }
            }
        }
    }

    // Dependencies
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
            let parent = file_id.split('.').next().unwrap_or(&file_id).to_string();
            let file_rel = if let Some(rest) = file_id.strip_prefix(&format!("{parent}.")) {
                rest.to_string()
            } else {
                file_id.clone()
            };
            rels_by_parent
                .entry(parent)
                .or_default()
                .push(format!("{file_rel} -[dependsOn]-> {dep_id} 'depends on'"));
            rel_count += 1;
        }
    }

    // Emit relationships using extend blocks inside each parent module
    for (parent_id, rels) in &rels_by_parent {
        writeln!(out, "  extend {} {{", parent_id).unwrap();
        for rel in rels {
            writeln!(out, "    {}", rel).unwrap();
        }
        writeln!(out, "  }}").unwrap();
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
        writeln!(out, "  view view_{} of {} {{", dir_id, dir_id).unwrap();
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
