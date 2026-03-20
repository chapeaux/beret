use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_core::Node;
use ast_grep_language::{LanguageExt, SupportLang};
use ignore::WalkBuilder;

use crate::store::CodebaseStore;

#[derive(Debug)]
struct Triple {
    subject: String,
    predicate: String,
    object: String,
}

// --- Language configurations for AST-based extraction ---

struct LangConfig {
    lang: SupportLang,
    func_kinds: &'static [&'static str],
    class_kinds: &'static [&'static str],
    call_kinds: &'static [&'static str],
    /// How to extract the name from a function/class node.
    name_strategy: NameStrategy,
    /// How to extract the callee name from a call node.
    call_strategy: CallStrategy,
}

#[derive(Clone, Copy)]
enum NameStrategy {
    /// Use the "name" field on the node.
    FieldName,
    /// Use the "declarator" field (C/C++ function definitions).
    FieldDeclarator,
    /// Use the first `simple_identifier` child (Kotlin).
    FirstIdentifierChild,
}

#[derive(Clone, Copy)]
enum CallStrategy {
    /// Use the "function" field on the call node.
    FieldFunction,
    /// Use the "name" field on the call node (Java method_invocation).
    FieldName,
    /// Use the "method" field on the call node (Ruby).
    FieldMethod,
    /// Use the first child of the call node (Kotlin, Swift).
    FirstChild,
}

macro_rules! lang {
    ($lang:ident, $funcs:expr, $classes:expr, $calls:expr) => {
        lang!($lang, $funcs, $classes, $calls, NameStrategy::FieldName, CallStrategy::FieldFunction)
    };
    ($lang:ident, $funcs:expr, $classes:expr, $calls:expr, $name:expr, $call:expr) => {
        LangConfig {
            lang: SupportLang::$lang,
            func_kinds: $funcs,
            class_kinds: $classes,
            call_kinds: $calls,
            name_strategy: $name,
            call_strategy: $call,
        }
    };
}

const LANG_CONFIGS: &[LangConfig] = &[
    lang!(Python,     &["function_definition"], &["class_definition"], &["call"]),
    lang!(Rust,       &["function_item"], &["struct_item", "impl_item"], &["call_expression"]),
    lang!(JavaScript, &["function_declaration"], &["class_declaration"], &["call_expression"]),
    lang!(TypeScript, &["function_declaration"], &["class_declaration", "interface_declaration"], &["call_expression"]),
    lang!(Tsx,        &["function_declaration"], &["class_declaration", "interface_declaration"], &["call_expression"]),
    lang!(Go,         &["function_declaration", "method_declaration"], &["type_spec"], &["call_expression"]),
    lang!(Java,       &["method_declaration"], &["class_declaration", "interface_declaration", "enum_declaration"], &["method_invocation"],
          NameStrategy::FieldName, CallStrategy::FieldName),
    lang!(C,          &["function_definition"], &["struct_specifier"], &["call_expression"],
          NameStrategy::FieldDeclarator, CallStrategy::FieldFunction),
    lang!(Cpp,        &["function_definition"], &["struct_specifier", "class_specifier"], &["call_expression"],
          NameStrategy::FieldDeclarator, CallStrategy::FieldFunction),
    lang!(CSharp,     &["method_declaration"], &["class_declaration", "interface_declaration", "struct_declaration", "enum_declaration"], &["invocation_expression"]),
    lang!(Ruby,       &["method"], &["class", "module"], &["call"],
          NameStrategy::FieldName, CallStrategy::FieldMethod),
    lang!(Php,        &["function_definition", "method_declaration"], &["class_declaration", "interface_declaration", "enum_declaration"], &["function_call_expression"]),
    lang!(Kotlin,     &["function_declaration"], &["class_declaration", "object_declaration"], &["call_expression"],
          NameStrategy::FirstIdentifierChild, CallStrategy::FirstChild),
    lang!(Swift,      &["function_declaration"], &["class_declaration", "struct_declaration"], &["call_expression"],
          NameStrategy::FieldName, CallStrategy::FirstChild),
    lang!(Scala,      &["function_definition"], &["class_definition", "object_definition", "trait_definition"], &["call_expression"]),
    lang!(Bash,       &["function_definition"], &[], &["command"]),
    lang!(Lua,        &["function_declaration"], &[], &["function_call"]),
];

fn lang_config_for_ext(ext: &str) -> Option<&'static LangConfig> {
    // Index-based lookup into LANG_CONFIGS
    let idx = match ext {
        "py"                         => 0,
        "rs"                         => 1,
        "js" | "mjs" | "cjs"        => 2,
        "ts"                         => 3,
        "tsx"                        => 4,
        "go"                         => 5,
        "java"                       => 6,
        "c"                          => 7,
        "cpp" | "cc" | "cxx" | "hpp" => 8,
        "cs"                         => 9,
        "rb"                         => 10,
        "php"                        => 11,
        "kt" | "kts"                 => 12,
        "swift"                      => 13,
        "scala" | "sc"               => 14,
        "sh" | "bash"                => 15,
        "lua"                        => 16,
        _ => return None,
    };
    Some(&LANG_CONFIGS[idx])
}

// --- Name extraction ---

fn extract_func_or_class_name(node: &Node<'_, StrDoc<SupportLang>>, strategy: NameStrategy) -> Option<String> {
    match strategy {
        NameStrategy::FieldName => node.field("name").map(|n| n.text().to_string()),
        NameStrategy::FieldDeclarator => {
            // C/C++: declarator may be a nested pointer_declarator or function_declarator
            let mut decl = node.field("declarator")?;
            // Unwrap nested declarators until we reach an identifier
            loop {
                if decl.kind() == "identifier" || decl.kind() == "field_identifier" {
                    return Some(decl.text().to_string());
                }
                if let Some(inner) = decl.field("declarator") {
                    decl = inner;
                } else {
                    return Some(leaf_name(&decl));
                }
            }
        }
        NameStrategy::FirstIdentifierChild => {
            // Kotlin: find first simple_identifier or type_identifier child
            for child in node.children() {
                let k = child.kind();
                if k == "simple_identifier" || k == "type_identifier" {
                    return Some(child.text().to_string());
                }
            }
            None
        }
    }
}

fn extract_call_name(node: &Node<'_, StrDoc<SupportLang>>, strategy: CallStrategy) -> Option<String> {
    let raw = match strategy {
        CallStrategy::FieldFunction => node.field("function").map(|n| n.text().to_string()),
        CallStrategy::FieldName => node.field("name").map(|n| n.text().to_string()),
        CallStrategy::FieldMethod => node.field("method").map(|n| n.text().to_string()),
        CallStrategy::FirstChild => {
            node.children().next().map(|n| n.text().to_string())
        }
    }?;

    Some(simplify_callee(&raw))
}

/// Strip leading qualifiers: `obj.method` → `method`, `mod::func` → `func`.
fn simplify_callee(text: &str) -> String {
    if let Some(pos) = text.rfind('.') {
        // Trim trailing parens/args that might be included
        let name = &text[pos + 1..];
        return strip_trailing(name);
    }
    if let Some(pos) = text.rfind("::") {
        return strip_trailing(&text[pos + 2..]);
    }
    strip_trailing(text)
}

fn strip_trailing(s: &str) -> String {
    // Take only the identifier part (stop at '(' or '<')
    s.split(['(', '<', '[', '{', ' ']).next().unwrap_or(s).to_string()
}

/// Get the text of the first named leaf node (fallback for complex declarators).
fn leaf_name(node: &Node<'_, StrDoc<SupportLang>>) -> String {
    for child in node.children() {
        if child.kind() == "identifier" || child.kind() == "field_identifier" {
            return child.text().to_string();
        }
    }
    // Last resort: take the whole text
    strip_trailing(&node.text())
}

// --- AST-based file processing ---

fn process_code_file(
    path: &Path,
    source: &str,
    config: &LangConfig,
    triples: &mut Vec<Triple>,
) {
    let root = config.lang.ast_grep(source);
    let root_node = root.root();
    let file_path = path.to_string_lossy();

    let mut func_stack: Vec<(String, std::ops::Range<usize>)> = Vec::new();

    for node in root_node.dfs() {
        let kind = node.kind();
        let range = node.range();

        while let Some(top) = func_stack.last() {
            if range.start >= top.1.end {
                func_stack.pop();
            } else {
                break;
            }
        }

        let is_func = config.func_kinds.iter().any(|k| *k == &*kind);
        if is_func {
            if let Some(name) = extract_func_or_class_name(&node, config.name_strategy) {
                let qualified = format!("{}/{}", file_path, name);
                triples.push(Triple {
                    subject: qualified.clone(),
                    predicate: "a".into(),
                    object: "Function".into(),
                });
                func_stack.push((qualified, node.range()));
                continue;
            }
        }

        let is_class = config.class_kinds.iter().any(|k| *k == &*kind);
        if is_class {
            if let Some(name) = extract_func_or_class_name(&node, config.name_strategy) {
                triples.push(Triple {
                    subject: format!("{}/{}", file_path, name),
                    predicate: "a".into(),
                    object: "Class".into(),
                });
                continue;
            }
        }

        let is_call = config.call_kinds.iter().any(|k| *k == &*kind);
        if is_call {
            if let Some(callee) = extract_call_name(&node, config.call_strategy) {
                let callee = iri_safe(&callee);
                if !callee.is_empty() {
                    let caller = func_stack
                        .last()
                        .map(|(name, _)| name.as_str())
                        .unwrap_or(&*file_path);
                    triples.push(Triple {
                        subject: caller.to_string(),
                        predicate: "calls".into(),
                        object: callee,
                    });
                }
            }
        }
    }
}

// --- Non-code file processing ---

/// Known non-code file types and how to handle them.
enum NonCodeKind {
    Json,
    Yaml,
    Markdown,
    Html,
    Css,
}

fn non_code_kind(ext: &str, file_name: &str) -> Option<NonCodeKind> {
    match ext {
        "json" => Some(NonCodeKind::Json),
        "yml" | "yaml" => Some(NonCodeKind::Yaml),
        "md" | "markdown" => Some(NonCodeKind::Markdown),
        "html" | "htm" => Some(NonCodeKind::Html),
        "css" => Some(NonCodeKind::Css),
        _ => {
            // Handle extensionless config files
            match file_name {
                "Makefile" | "Dockerfile" | "Jenkinsfile" => None, // skip these
                _ => None,
            }
        }
    }
}

/// Sanitize a string for safe use as an IRI path segment.
/// Only allows characters valid in IRI path segments per RFC 3987.
fn iri_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || "-._~:@!$&'()*+,;=/".contains(c) || c > '\x7F' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn process_json_file(path: &Path, source: &str, triples: &mut Vec<Triple>) {
    let file_path = path.to_string_lossy();
    triples.push(Triple {
        subject: file_path.to_string(),
        predicate: "a".into(),
        object: "Config".into(),
    });

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(source) else {
        return;
    };

    if let Some(obj) = parsed.as_object() {
        for key in obj.keys() {
            triples.push(Triple {
                subject: file_path.to_string(),
                predicate: "declares".into(),
                object: iri_safe(key),
            });
        }

        // Extract dependency names from package.json
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name == "package.json" {
            for dep_key in &["dependencies", "devDependencies", "peerDependencies"] {
                if let Some(deps) = obj.get(*dep_key).and_then(|v| v.as_object()) {
                    for pkg in deps.keys() {
                        triples.push(Triple {
                            subject: file_path.to_string(),
                            predicate: "dependsOn".into(),
                            object: iri_safe(pkg),
                        });
                    }
                }
            }
        }
    }
}

fn process_yaml_file(path: &Path, source: &str, triples: &mut Vec<Triple>) {
    let file_path = path.to_string_lossy();
    triples.push(Triple {
        subject: file_path.to_string(),
        predicate: "a".into(),
        object: "Config".into(),
    });

    // Extract top-level keys by simple line parsing (avoids a YAML dependency)
    for line in source.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        // A top-level key is a line starting with a non-space character followed by ':'
        if !line.starts_with(' ') && !line.starts_with('\t') && !line.starts_with('-') {
            if let Some(key) = line.split(':').next() {
                let key = key.trim();
                if !key.is_empty() && !key.starts_with('-') {
                    triples.push(Triple {
                        subject: file_path.to_string(),
                        predicate: "declares".into(),
                        object: iri_safe(key),
                    });
                }
            }
        }
    }
}

fn process_markdown_file(path: &Path, source: &str, triples: &mut Vec<Triple>) {
    let file_path = path.to_string_lossy();
    triples.push(Triple {
        subject: file_path.to_string(),
        predicate: "a".into(),
        object: "Document".into(),
    });

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            // Strip leading '#' chars and spaces
            let heading = trimmed.trim_start_matches('#').trim();
            if !heading.is_empty() {
                triples.push(Triple {
                    subject: format!("{}/{}", file_path, iri_safe(heading)),
                    predicate: "a".into(),
                    object: "Section".into(),
                });
            }
        }
    }
}

fn process_html_file(path: &Path, source: &str, triples: &mut Vec<Triple>) {
    let file_path = path.to_string_lossy();
    triples.push(Triple {
        subject: file_path.to_string(),
        predicate: "a".into(),
        object: "Document".into(),
    });

    // Extract id="..." and class="..." attributes via simple regex-free scanning
    for segment in source.split("id=\"") {
        if let Some(end) = segment.find('"') {
            let id = &segment[..end];
            if !id.is_empty() {
                triples.push(Triple {
                    subject: format!("{}/#{}", file_path, iri_safe(id)),
                    predicate: "a".into(),
                    object: "Element".into(),
                });
            }
        }
    }
    for segment in source.split("class=\"") {
        if let Some(end) = segment.find('"') {
            let classes = &segment[..end];
            for class in classes.split_whitespace() {
                if !class.is_empty() {
                    triples.push(Triple {
                        subject: format!("{}/.{}", file_path, iri_safe(class)),
                        predicate: "a".into(),
                        object: "Element".into(),
                    });
                }
            }
        }
    }
}

fn process_css_file(path: &Path, source: &str, triples: &mut Vec<Triple>) {
    let file_path = path.to_string_lossy();
    triples.push(Triple {
        subject: file_path.to_string(),
        predicate: "a".into(),
        object: "Stylesheet".into(),
    });

    // Extract selectors: lines ending with '{' (simplified)
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with('{') {
            let selector = trimmed.trim_end_matches('{').trim();
            if !selector.is_empty()
                && !selector.starts_with('/')
                && !selector.starts_with('*')
            {
                triples.push(Triple {
                    subject: format!("{}/{}", file_path, iri_safe(selector)),
                    predicate: "a".into(),
                    object: "Style".into(),
                });
            }
        }
    }
}

fn process_non_code_file(path: &Path, source: &str, kind: NonCodeKind, triples: &mut Vec<Triple>) {
    match kind {
        NonCodeKind::Json => process_json_file(path, source, triples),
        NonCodeKind::Yaml => process_yaml_file(path, source, triples),
        NonCodeKind::Markdown => process_markdown_file(path, source, triples),
        NonCodeKind::Html => process_html_file(path, source, triples),
        NonCodeKind::Css => process_css_file(path, source, triples),
    }
}

// --- Binary file metadata ---

const BINARY_EXTENSIONS: &[(&str, &str)] = &[
    ("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"),
    ("gif", "image/gif"), ("webp", "image/webp"), ("svg", "image/svg+xml"),
    ("ico", "image/x-icon"), ("bmp", "image/bmp"),
    ("mp3", "audio/mpeg"), ("wav", "audio/wav"), ("ogg", "audio/ogg"),
    ("flac", "audio/flac"), ("aac", "audio/aac"),
    ("mp4", "video/mp4"), ("webm", "video/webm"), ("avi", "video/x-msvideo"),
    ("mov", "video/quicktime"), ("mkv", "video/x-matroska"),
    ("pdf", "application/pdf"), ("zip", "application/zip"),
    ("gz", "application/gzip"), ("tar", "application/x-tar"),
    ("wasm", "application/wasm"), ("exe", "application/x-executable"),
    ("dll", "application/x-sharedlib"), ("so", "application/x-sharedlib"),
    ("dylib", "application/x-sharedlib"),
    ("ttf", "font/ttf"), ("otf", "font/otf"), ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("sqlite", "application/x-sqlite3"), ("db", "application/x-sqlite3"),
];

fn binary_mime_type(ext: &str) -> Option<&'static str> {
    BINARY_EXTENSIONS.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

fn process_binary_file(path: &Path, triples: &mut Vec<Triple>) {
    let file_path = path.to_string_lossy();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    triples.push(Triple {
        subject: file_path.to_string(),
        predicate: "a".into(),
        object: "Binary".into(),
    });

    if let Some(mime) = binary_mime_type(ext) {
        triples.push(Triple {
            subject: file_path.to_string(),
            predicate: "hasMimeType".into(),
            object: mime.to_string(),
        });
    }

    if let Ok(meta) = std::fs::metadata(path) {
        triples.push(Triple {
            subject: file_path.to_string(),
            predicate: "hasSize".into(),
            object: meta.len().to_string(),
        });
    }
}

// --- Practice detection ---

/// Detect engineering practices from file presence and path patterns.
/// Returns (predicate, object) pairs for the `<project>` subject.
fn detect_practice(path: &Path, file_name: &str) -> Option<(&'static str, &'static str)> {
    let path_str = path.to_string_lossy();

    // CI/CD platforms
    if path_str.contains(".github/workflows/") && file_name.ends_with(".yml") {
        return Some(("usesCIPlatform", "github-actions"));
    }
    match file_name {
        ".gitlab-ci.yml" => return Some(("usesCIPlatform", "gitlab-ci")),
        "Jenkinsfile" => return Some(("usesCIPlatform", "jenkins")),
        ".travis.yml" => return Some(("usesCIPlatform", "travis")),
        _ => {}
    }
    if path_str.contains(".circleci/") && file_name == "config.yml" {
        return Some(("usesCIPlatform", "circleci"));
    }

    // Containerization
    match file_name {
        "Dockerfile" | ".dockerignore" => return Some(("usesContainerization", "docker")),
        "docker-compose.yml" | "docker-compose.yaml" => return Some(("usesContainerization", "docker-compose")),
        _ => {}
    }

    // Build tools
    match file_name {
        "Makefile" | "makefile" | "GNUmakefile" => return Some(("usesBuildTool", "make")),
        "build.gradle" | "build.gradle.kts" => return Some(("usesBuildTool", "gradle")),
        "pom.xml" => return Some(("usesBuildTool", "maven")),
        "CMakeLists.txt" => return Some(("usesBuildTool", "cmake")),
        _ => {}
    }

    // Linters
    if file_name.starts_with(".eslintrc") || file_name == ".eslintignore" {
        return Some(("usesLinter", "eslint"));
    }
    match file_name {
        "biome.json" | "biome.jsonc" => return Some(("usesLinter", "biome")),
        "ruff.toml" | ".ruff.toml" => return Some(("usesLinter", "ruff")),
        ".rubocop.yml" => return Some(("usesLinter", "rubocop")),
        ".stylelintrc" | "stylelint.config.js" => return Some(("usesLinter", "stylelint")),
        _ => {}
    }

    // Formatters
    if file_name.starts_with(".prettierrc") || file_name.starts_with("prettier.config") {
        return Some(("usesFormatter", "prettier"));
    }
    if file_name == ".editorconfig" {
        return Some(("usesFormatter", "editorconfig"));
    }

    // Test frameworks
    if file_name.starts_with("jest.config") {
        return Some(("usesTestFramework", "jest"));
    }
    if file_name.starts_with("vitest.config") {
        return Some(("usesTestFramework", "vitest"));
    }
    if file_name.starts_with("cypress.config") {
        return Some(("usesTestFramework", "cypress"));
    }
    if file_name.starts_with("playwright.config") {
        return Some(("usesTestFramework", "playwright"));
    }
    match file_name {
        "pytest.ini" | "conftest.py" | "setup.cfg" => return Some(("usesTestFramework", "pytest")),
        ".mocharc.yml" | ".mocharc.json" => return Some(("usesTestFramework", "mocha")),
        _ => {}
    }
    if file_name.starts_with("karma.conf") {
        return Some(("usesTestFramework", "karma"));
    }

    // Type checking
    match file_name {
        "tsconfig.json" => return Some(("usesTypeChecking", "typescript")),
        "jsconfig.json" => return Some(("usesTypeChecking", "javascript-jsdoc")),
        "mypy.ini" | ".mypy.ini" => return Some(("usesTypeChecking", "mypy")),
        _ => {}
    }

    // Package managers
    match file_name {
        "package.json" => return Some(("usesPackageManager", "npm")),
        "yarn.lock" => return Some(("usesPackageManager", "yarn")),
        "pnpm-lock.yaml" => return Some(("usesPackageManager", "pnpm")),
        "bun.lockb" | "bun.lock" => return Some(("usesPackageManager", "bun")),
        "Cargo.toml" => return Some(("usesPackageManager", "cargo")),
        "go.mod" => return Some(("usesPackageManager", "go-modules")),
        "requirements.txt" | "Pipfile" => return Some(("usesPackageManager", "pip")),
        "poetry.lock" => return Some(("usesPackageManager", "poetry")),
        "Gemfile" => return Some(("usesPackageManager", "bundler")),
        "composer.json" => return Some(("usesPackageManager", "composer")),
        _ => {}
    }

    // Documentation
    match file_name {
        "CONTRIBUTING.md" | "CONTRIBUTING" => return Some(("hasDocumentation", "contributing-guide")),
        "SECURITY.md" | "SECURITY.txt" | "SECURITY" => return Some(("hasDocumentation", "security-policy")),
        "CHANGELOG.md" | "CHANGELOG" | "CHANGES.md" => return Some(("hasDocumentation", "changelog")),
        "LICENSE" | "LICENSE.md" | "LICENSE.txt" => return Some(("hasDocumentation", "license")),
        "CODEOWNERS" => return Some(("hasDocumentation", "codeowners")),
        "CODE_OF_CONDUCT.md" => return Some(("hasDocumentation", "code-of-conduct")),
        _ => {}
    }
    if path_str.contains(".github/ISSUE_TEMPLATE") {
        return Some(("hasDocumentation", "issue-templates"));
    }
    if path_str.contains("PULL_REQUEST_TEMPLATE") {
        return Some(("hasDocumentation", "pr-template"));
    }

    // Conventions
    if file_name.starts_with(".commitlintrc") || file_name.starts_with("commitlint.config") {
        return Some(("followsConvention", "conventional-commits"));
    }
    if path_str.contains(".husky/") {
        return Some(("followsConvention", "git-hooks"));
    }
    if file_name.starts_with(".lintstagedrc") || file_name.starts_with("lint-staged.config") {
        return Some(("followsConvention", "lint-staged"));
    }
    match file_name {
        "renovate.json" | ".renovaterc" => return Some(("followsConvention", "automated-dependency-updates")),
        _ => {}
    }
    if path_str.contains(".dependabot/") || file_name == "dependabot.yml" {
        return Some(("followsConvention", "automated-dependency-updates"));
    }

    None
}

/// Map directory names to architecture layer labels.
fn detect_layer(dir_name: &str) -> Option<&'static str> {
    match dir_name {
        "src" | "lib" | "app" => Some("source"),
        "test" | "tests" | "__tests__" | "spec" | "specs" => Some("tests"),
        "docs" | "doc" | "documentation" => Some("documentation"),
        "scripts" | "bin" | "tools" => Some("scripts"),
        "config" | "configs" => Some("configuration"),
        "packages" | "modules" | "crates" | "workspaces" => Some("monorepo-packages"),
        "api" | "routes" | "endpoints" => Some("api"),
        "components" | "views" | "pages" => Some("ui"),
        "models" | "entities" | "domain" => Some("domain"),
        "services" | "providers" => Some("services"),
        "middleware" | "interceptors" => Some("middleware"),
        "utils" | "helpers" | "common" | "shared" => Some("utilities"),
        "migrations" | "seeds" => Some("database"),
        "deploy" | "infra" | "terraform" | "k8s" | "kubernetes" => Some("infrastructure"),
        _ => None,
    }
}

// --- Main ingestion pipeline ---

pub fn ingest(root: &Path, store: &CodebaseStore) -> Result<usize, Box<dyn std::error::Error>> {
    let all_triples: Mutex<Vec<Triple>> = Mutex::new(Vec::new());

    WalkBuilder::new(root)
        .hidden(false)
        .build_parallel()
        .visit(&mut TripleVisitorBuilder {
            triples: &all_triples,
        });

    let triples = all_triples.into_inner().unwrap();
    let count = triples.len();

    for triple in &triples {
        store.insert_triple(&triple.subject, &triple.predicate, &triple.object)?;
    }

    Ok(count)
}

struct TripleVisitorBuilder<'a> {
    triples: &'a Mutex<Vec<Triple>>,
}

impl<'a> ignore::ParallelVisitorBuilder<'a> for TripleVisitorBuilder<'a> {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 'a> {
        Box::new(TripleVisitor {
            shared: self.triples,
            local: Vec::new(),
            seen_practices: HashSet::new(),
            seen_layers: HashSet::new(),
        })
    }
}

struct TripleVisitor<'a> {
    shared: &'a Mutex<Vec<Triple>>,
    local: Vec<Triple>,
    seen_practices: HashSet<(&'static str, &'static str)>,
    seen_layers: HashSet<&'static str>,
}

impl Drop for TripleVisitor<'_> {
    fn drop(&mut self) {
        if !self.local.is_empty() {
            self.shared.lock().unwrap().append(&mut self.local);
        }
    }
}

impl ignore::ParallelVisitor for TripleVisitor<'_> {
    fn visit(&mut self, entry: Result<ignore::DirEntry, ignore::Error>) -> ignore::WalkState {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => return ignore::WalkState::Continue,
        };

        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            return ignore::WalkState::Continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // 0. Detect engineering practices from file presence
        if let Some(practice) = detect_practice(path, file_name) {
            if self.seen_practices.insert(practice) {
                self.local.push(Triple {
                    subject: "project".into(),
                    predicate: practice.0.into(),
                    object: practice.1.into(),
                });
            }
        }

        // Detect architecture layers from directory names
        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                if let Some(name_str) = name.to_str() {
                    if let Some(layer) = detect_layer(name_str) {
                        if self.seen_layers.insert(layer) {
                            self.local.push(Triple {
                                subject: "project".into(),
                                predicate: "hasLayer".into(),
                                object: layer.into(),
                            });
                        }
                    }
                }
            }
        }

        // 1. Try AST-based code extraction
        if let Some(config) = lang_config_for_ext(ext) {
            if let Ok(source) = std::fs::read_to_string(path) {
                process_code_file(path, &source, config, &mut self.local);
            }
        }
        // 2. Try non-code text extraction
        else if let Some(kind) = non_code_kind(ext, file_name) {
            if let Ok(source) = std::fs::read_to_string(path) {
                process_non_code_file(path, &source, kind, &mut self.local);
            }
        }
        // 3. Try binary metadata
        else if binary_mime_type(ext).is_some() {
            process_binary_file(path, &mut self.local);
        }

        if self.local.len() >= 1024 {
            self.shared.lock().unwrap().append(&mut self.local);
        }

        ignore::WalkState::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();

        fs::write(
            dir.path().join("main.py"),
            "def hello():\n    print(\"hello\")\n\ndef greet(name):\n    hello()\n    print(name)\n\nclass Greeter:\n    def say(self):\n        hello()\n",
        ).unwrap();

        fs::write(
            dir.path().join("lib.rs"),
            "fn process() {\n    helper();\n}\n\nfn helper() {\n    println!(\"help\");\n}\n\nstruct Config {\n    name: String,\n}\n",
        ).unwrap();

        fs::write(
            dir.path().join("app.js"),
            "function render() {\n    update();\n    console.log(\"rendered\");\n}\n\nfunction update() {}\n\nclass App {\n    constructor() {}\n}\n",
        ).unwrap();

        dir
    }

    #[test]
    fn ingest_finds_functions() {
        let dir = setup_test_dir();
        let store = CodebaseStore::new().unwrap();
        let count = ingest(dir.path(), &store).unwrap();
        assert!(count > 0, "should have found triples");

        let json = store
            .query_to_json(
                "SELECT ?func WHERE { ?func <http://repo.example.org/a> <http://repo.example.org/Function> }",
            )
            .unwrap();
        let funcs = json.as_array().unwrap();
        assert!(funcs.len() >= 5, "expected at least 5 functions, got {}", funcs.len());
    }

    #[test]
    fn ingest_finds_classes() {
        let dir = setup_test_dir();
        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let json = store
            .query_to_json(
                "SELECT ?cls WHERE { ?cls <http://repo.example.org/a> <http://repo.example.org/Class> }",
            )
            .unwrap();
        let classes = json.as_array().unwrap();
        assert!(classes.len() >= 2, "expected at least 2 classes, got {}", classes.len());
    }

    #[test]
    fn ingest_finds_calls() {
        let dir = setup_test_dir();
        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let json = store
            .query_to_json(
                "SELECT ?caller ?callee WHERE { ?caller <http://repo.example.org/calls> ?callee }",
            )
            .unwrap();
        let calls = json.as_array().unwrap();
        assert!(calls.len() >= 3, "expected at least 3 call edges, got {}", calls.len());
    }

    #[test]
    fn ingest_is_idempotent_after_clear() {
        let dir = setup_test_dir();
        let store = CodebaseStore::new().unwrap();

        let count1 = ingest(dir.path(), &store).unwrap();
        store.clear().unwrap();
        let count2 = ingest(dir.path(), &store).unwrap();
        assert_eq!(count1, count2);
    }

    #[test]
    fn ingest_typescript() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("app.ts"),
            "function greet(name: string): void {\n    console.log(name);\n}\n\ninterface Config {\n    port: number;\n}\n\nclass Server {\n    start() { greet(\"hi\"); }\n}\n",
        ).unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let funcs = store.query_to_json("SELECT ?f WHERE { ?f <http://repo.example.org/a> <http://repo.example.org/Function> }").unwrap();
        assert!(funcs.as_array().unwrap().len() >= 1);

        let classes = store.query_to_json("SELECT ?c WHERE { ?c <http://repo.example.org/a> <http://repo.example.org/Class> }").unwrap();
        assert!(classes.as_array().unwrap().len() >= 2, "expected Server + Config interface");
    }

    #[test]
    fn ingest_go() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("main.go"),
            "package main\n\nfunc main() {\n    helper()\n}\n\nfunc helper() {}\n",
        ).unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let funcs = store.query_to_json("SELECT ?f WHERE { ?f <http://repo.example.org/a> <http://repo.example.org/Function> }").unwrap();
        assert!(funcs.as_array().unwrap().len() >= 2, "expected main + helper");
    }

    #[test]
    fn ingest_java() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("App.java"),
            "public class App {\n    public void run() {\n        helper();\n    }\n    private void helper() {}\n}\n",
        ).unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let classes = store.query_to_json("SELECT ?c WHERE { ?c <http://repo.example.org/a> <http://repo.example.org/Class> }").unwrap();
        assert!(classes.as_array().unwrap().len() >= 1, "expected App class");

        let funcs = store.query_to_json("SELECT ?f WHERE { ?f <http://repo.example.org/a> <http://repo.example.org/Function> }").unwrap();
        assert!(funcs.as_array().unwrap().len() >= 2, "expected run + helper");
    }

    #[test]
    fn ingest_c() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("main.c"),
            "void helper() {}\n\nint main() {\n    helper();\n    return 0;\n}\n",
        ).unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let funcs = store.query_to_json("SELECT ?f WHERE { ?f <http://repo.example.org/a> <http://repo.example.org/Function> }").unwrap();
        assert!(funcs.as_array().unwrap().len() >= 2, "expected main + helper");
    }

    #[test]
    fn ingest_json_config() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name": "myapp", "version": "1.0.0", "dependencies": {"express": "^4.0", "lodash": "^4.0"}, "devDependencies": {"jest": "^29"}}"#,
        ).unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let configs = store.query_to_json("SELECT ?c WHERE { ?c <http://repo.example.org/a> <http://repo.example.org/Config> }").unwrap();
        assert_eq!(configs.as_array().unwrap().len(), 1);

        let deps = store.query_to_json("SELECT ?dep WHERE { ?f <http://repo.example.org/dependsOn> ?dep }").unwrap();
        assert!(deps.as_array().unwrap().len() >= 3, "expected express, lodash, jest");
    }

    #[test]
    fn ingest_markdown() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("README.md"),
            "# My Project\n\n## Installation\n\nSome text.\n\n## Usage\n\nMore text.\n",
        ).unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let sections = store.query_to_json("SELECT ?s WHERE { ?s <http://repo.example.org/a> <http://repo.example.org/Section> }").unwrap();
        assert!(sections.as_array().unwrap().len() >= 3, "expected 3 headings");
    }

    #[test]
    fn ingest_binary_metadata() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("logo.png"), b"fake png data").unwrap();
        fs::write(dir.path().join("data.pdf"), b"fake pdf data").unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let binaries = store.query_to_json("SELECT ?b WHERE { ?b <http://repo.example.org/a> <http://repo.example.org/Binary> }").unwrap();
        assert_eq!(binaries.as_array().unwrap().len(), 2);

        let mimes = store.query_to_json("SELECT ?b ?m WHERE { ?b <http://repo.example.org/hasMimeType> ?m }").unwrap();
        assert_eq!(mimes.as_array().unwrap().len(), 2);

        let sizes = store.query_to_json("SELECT ?b ?s WHERE { ?b <http://repo.example.org/hasSize> ?s }").unwrap();
        assert_eq!(sizes.as_array().unwrap().len(), 2);
    }

    #[test]
    fn ingest_css() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("style.css"),
            "body {\n  margin: 0;\n}\n\n.container {\n  max-width: 1200px;\n}\n\n#app {\n  display: flex;\n}\n",
        ).unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        let styles = store.query_to_json("SELECT ?s WHERE { ?s <http://repo.example.org/a> <http://repo.example.org/Style> }").unwrap();
        assert!(styles.as_array().unwrap().len() >= 3, "expected body, .container, #app");
    }

    #[test]
    fn ingest_5000_files_under_2_seconds() {
        let dir = tempfile::tempdir().unwrap();

        let file_content = "def setup():\n    configure()\n    validate()\n\ndef configure():\n    load_defaults()\n\ndef validate():\n    check_input()\n\nclass Handler:\n    def handle(self):\n        setup()\n        self.process()\n\n    def process(self):\n        pass\n";
        for i in 0..5000 {
            let subdir = dir.path().join(format!("pkg_{}", i / 100));
            fs::create_dir_all(&subdir).unwrap();
            fs::write(subdir.join(format!("mod_{}.py", i)), file_content).unwrap();
        }

        let store = CodebaseStore::new().unwrap();
        let start = std::time::Instant::now();
        let count = ingest(dir.path(), &store).unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs_f64() < 2.0,
            "ingestion took {:.2}s, expected < 2s",
            elapsed.as_secs_f64()
        );
        assert!(count >= 5000 * 5, "expected at least 25000 triples, got {}", count);
        eprintln!("Ingested {} triples from 5000 files in {:.3}s", count, elapsed.as_secs_f64());
    }

    #[test]
    fn ingest_detects_practices() {
        let dir = tempfile::tempdir().unwrap();

        // Create practice signal files
        let workflows = dir.path().join(".github/workflows");
        fs::create_dir_all(&workflows).unwrap();
        fs::write(workflows.join("ci.yml"), "name: CI").unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        fs::write(dir.path().join(".eslintrc.json"), "{}").unwrap();
        fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
        fs::write(dir.path().join("Dockerfile"), "FROM node").unwrap();
        fs::write(dir.path().join("LICENSE"), "MIT").unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("tests")).unwrap();
        fs::write(dir.path().join("src/app.js"), "function main() {}").unwrap();
        fs::write(dir.path().join("tests/app.test.js"), "test('it', () => {})").unwrap();

        let store = CodebaseStore::new().unwrap();
        ingest(dir.path(), &store).unwrap();

        // Check CI/CD detection
        let ci = store.query_to_json(
            "SELECT ?v WHERE { <http://repo.example.org/project> <http://repo.example.org/usesCIPlatform> ?v }"
        ).unwrap();
        assert!(!ci.as_array().unwrap().is_empty(), "should detect github-actions");

        // Check package manager
        let pm = store.query_to_json(
            "SELECT ?v WHERE { <http://repo.example.org/project> <http://repo.example.org/usesPackageManager> ?v }"
        ).unwrap();
        assert!(!pm.as_array().unwrap().is_empty(), "should detect npm");

        // Check linter
        let lint = store.query_to_json(
            "SELECT ?v WHERE { <http://repo.example.org/project> <http://repo.example.org/usesLinter> ?v }"
        ).unwrap();
        assert!(!lint.as_array().unwrap().is_empty(), "should detect eslint");

        // Check architecture layers
        let layers = store.query_to_json(
            "SELECT ?v WHERE { <http://repo.example.org/project> <http://repo.example.org/hasLayer> ?v }"
        ).unwrap();
        let layer_count = layers.as_array().unwrap().len();
        assert!(layer_count >= 2, "should detect source + tests layers, got {}", layer_count);

        // Check containerization
        let docker = store.query_to_json(
            "SELECT ?v WHERE { <http://repo.example.org/project> <http://repo.example.org/usesContainerization> ?v }"
        ).unwrap();
        assert!(!docker.as_array().unwrap().is_empty(), "should detect docker");
    }
}
