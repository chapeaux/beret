# Beret — Architecture

## Overview
Beret is a high-performance Rust MCP server in the `chapeaux` project family. It combines an RDF knowledge graph (oxigraph), structural code parsing (ast-grep), async runtime (tokio), and Model Context Protocol (rust-mcp-sdk) to expose codebase intelligence via purpose-built tools, SPARQL queries, practice detection, and LikeC4 architecture diagrams. It supports both stdio and HTTP/SSE transports.

The server starts with an empty knowledge graph and indexes on demand via `refresh_index`. This allows it to be launched once at a top-level directory and re-targeted contextually as the user works across different projects or subdirectories.

## Project Structure
```
beret/
├── Cargo.toml        # Published as chapeaux-beret, binary name: beret
├── CLAUDE.md         # This file — architecture notes
├── LICENSE           # MIT
├── npm/
│   ├── package.json  # @chapeaux/beret npm package
│   ├── jsr.json      # @chapeaux/beret JSR config
│   ├── install.js    # Binary download script
│   └── run.js        # Binary runner
└── src/
    ├── main.rs       # CLI + MCP server (stdio and HTTP modes)
    ├── store.rs      # CodebaseStore — oxigraph wrapper (with iri_escape)
    ├── tools.rs      # Query tools, live search, practice analysis, LikeC4 diagrams
    └── ingestor.rs   # Parallel file walker + extraction + practice detection → RDF triples
```

## Module Details

### main.rs — CLI + MCP Server
- **Crate name:** `chapeaux-beret`, **binary name:** `beret`
- Custom CLI parser with `--serve`, `--help`, `--version`
- Two modes: **Stdio** (`beret [PATH]`) and **HTTP** (`beret --serve [HOST:]PORT`)
- No startup indexing — graph is empty until `refresh_index` is called
- Helper methods: `get_arg`, `require_arg`, `get_limit`, `get_offset`, `get_exclude`, `ok_json`, `ok_json_limited`, `ok_text`, `err`
- `ok_json_limited(value, limit, offset)` — pagination: `{results, total, returned, offset, has_more, next_offset, message}`
- IMPORTANT: `rust_mcp_sdk::schema::*` exports `Result` that shadows `std::result::Result`

### tools.rs — Query Tools, Search, Practices, Diagrams
- All SPARQL query tools accept `exclude: &[String]` for directory exclusion via `exclude_filters()` helper
- `find_symbol`, `find_callers` (optional name), `find_callees` (optional name), `list_structures`, `file_stats`, `find_dead_code`, `find_dependencies`, `find_entry_points` — SPARQL-backed
- `search_pattern(root, pattern, language, exclude, limit)` — live ast-grep walk
- `describe_project` — consolidated analysis with `generate_insights()` cross-cutting observations
- `describe_practices`, `describe_testing`, `describe_ci_cd`, `describe_code_quality`, `describe_architecture`, `describe_documentation`, `describe_dependencies` — individual practice analysis
- `describe_testing` also calls `detect_test_deps_from_graph()` to find test frameworks from `dependsOn` triples
- `generate_diagram(store, scope, depth, code_only, exclude, limit)` — LikeC4 DSL generation:
  - Auto-depth (0): picks 1/2/3 based on codebase size (≤100→3, ≤500→2, 500+→1)
  - `code_only=true` (default): SPARQL-level filter to Function/Class only + excludes 17 non-source dirs
  - Beret color theme: navy (#00005F), orange (#F5921B), gold (#FFCC17), teal (#37A3A3), blue (#0066CC)
  - Element styling: module=rectangle/navy, file=component/blue, func=rectangle/teal, cls=storage/navy, external=cylinder/gold
  - Relationship styling: calls=orange/solid, dependsOn=gold/dashed/diamond
  - Reserved word escaping: `to_id()` appends `El` suffix for 40+ LikeC4 keywords
  - Relationships emitted via `extend parent { }` blocks for valid nested references
  - Scoped `view of` per top-level directory

### store.rs — CodebaseStore
- Wraps `oxigraph::store::Store` (in-memory RDF)
- `insert_triple(s, p, o)` — percent-encodes via `iri_escape()`
- `query_to_json(sparql)` — runs SELECT/ASK queries
- `clear()` — wipes store

### ingestor.rs — Parallel Ingestion + Practice Detection
- `hidden(false)` — walks dotfiles (`.github/`, `.eslintrc`, etc.) for practice detection
- Four extraction tiers: **AST** (17 languages), **Build files** (20+ manifest types), **Non-code text** (JSON, YAML, Markdown, HTML, CSS, AsciiDoc, reStructuredText, man pages), **Binary metadata**
- **Build file extraction**: `process_build_file()` extracts `dependsOn` triples from: pom.xml, build.gradle(.kts), Cargo.toml, go.mod, Gemfile, Podfile, requirements.txt, pyproject.toml, composer.json, Pipfile, pubspec.yaml, Package.swift, build.sbt, mix.exs, .csproj/.fsproj, Dockerfile/Containerfile/\*.Dockerfile (FROM), docker-compose.yml (image), .spec (Requires/BuildRequires), debian/control (Depends/Build-Depends). Also detects Maven plugins as practice triples.
- **Practice detection**: `detect_practice(path, file_name)` matches 120+ file patterns → `<project>` triples. Predicates: `usesCIPlatform`, `usesContainerization`, `usesBuildTool`, `usesLinter`, `usesFormatter`, `usesTestFramework`, `usesTypeChecking`, `usesPackageManager`, `hasDocumentation`, `followsConvention`, `usesDeploymentPlatform`, `usesCodeAnalysis`, `usesPackagingFormat`, `usesConfigManagement`
- **Linux/Red Hat coverage**: RPM spec files, Containerfile/Podman, Packit/Zuul/Tekton CI, autotools/Kbuild, systemd units, SELinux policy, D-Bus/polkit/udev, tmt/FMF testing, Ansible/Puppet/Chef config management, Helm/Kustomize, OLM operators, Fedora gating, deb/arch/snap/flatpak packaging, AsciiDoc/RST/man page documentation
- **Layer detection**: `detect_layer(dir_name)` maps 30+ dir names → `hasLayer` triples
- Thread-local `HashSet` deduplication for practices; `iri_safe()` for text values

## MCP Tools

All list-returning tools support `limit`, `offset`, and `exclude` parameters.

### Always available
| Tool | Default limit | Purpose |
|------|--------------|---------|
| `refresh_index` | — | Index a directory (optional `path` param). Call this first. |
| `query_codebase` | 500 | Raw SPARQL queries |
| `find_symbol` | 100 | Find definitions by name (partial match) |
| `find_callers` | 100 | Reverse call graph (name optional — omit for all edges) |
| `find_callees` | 100 | Forward call graph (name optional) |
| `list_structures` | 200 | List structures, filter by path/kind |
| `file_stats` | — | Counts by type |
| `find_dead_code` | 100 | Uncalled functions |
| `find_dependencies` | 200 | Package dependencies |
| `find_entry_points` | 100 | Entry points (main/index/app/server/cli) |
| `search_pattern` | 200 | Live ast-grep search |
| `generate_diagram` | 500 | LikeC4 diagram (auto-depth, code_only, Beret theme) |
| `describe_project` | — | **Recommended first call.** Consolidated project analysis with cross-cutting insights |
| `describe_practices` | — | Summary of all detected practices |
| `describe_testing` | — | Test frameworks, test dependencies from graph, test ratio |
| `describe_ci_cd` | — | CI platforms, containers, build tools, deployment platforms |
| `describe_code_quality` | — | Linters, formatters, type checkers, conventions, code analysis |
| `describe_architecture` | — | Layers, monorepo detection, structure counts |
| `describe_documentation` | — | Doc artifacts, coverage |
| `describe_dependencies` | — | Package managers, dep count, auto-updates |

### HTTP mode only
| Tool | Purpose |
|------|---------|
| `index_repo` | Clone a git repo and index it |

## Distribution
- **crates.io:** `cargo install chapeaux-beret`
- **npm:** `npx @chapeaux/beret`
- **JSR:** `npx jsr:@chapeaux/beret`
- **CI:** `.github/workflows/release.yml` — 5 targets, GitHub Releases + crates.io + npm

## Build
- Release profile: `lto = true`, `codegen-units = 1`

## Conventions
- Edition 2024
- Error type: `Box<dyn std::error::Error>`
- `std::result::Result` always qualified (schema `Result` conflict)
