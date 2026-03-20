# Beret — Architecture

## Overview
Beret is a high-performance Rust MCP server in the `chapeaux` project family. It combines an RDF knowledge graph (oxigraph), structural code parsing (ast-grep), async runtime (tokio), and Model Context Protocol (rust-mcp-sdk) to expose codebase intelligence via SPARQL queries and purpose-built tools. It supports both stdio and HTTP/SSE transports.

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
    ├── store.rs      # CodebaseStore — oxigraph wrapper
    ├── tools.rs      # Pre-built query tools + live ast-grep pattern search
    └── ingestor.rs   # Parallel file walker + extraction → RDF triples
```

## Key Dependencies
| Crate | Purpose |
|-------|---------|
| oxigraph | SPARQL-capable RDF store for knowledge graph operations |
| tokio (full) | Async runtime for concurrent I/O |
| rust-mcp-sdk | MCP server SDK (stdio + hyper-server for HTTP/SSE) |
| async-trait | Async trait support (required by ServerHandler) |
| ast-grep-core | Structural code search via AST patterns (library) |
| ast-grep-language | Language definitions (17 languages) |
| ignore | .gitignore-aware parallel filesystem traversal |
| serde / serde_json | Serialization layer |

## Module Details

### main.rs — CLI + MCP Server
- **Crate name:** `chapeaux-beret`, **binary name:** `beret`
- Custom CLI parser (no clap dependency) with `--serve`, `--help`, `--version`
- Two modes:
  - **Stdio:** `beret [PATH]` — indexes local dir, serves over stdio
  - **HTTP:** `beret --serve [HOST:]PORT` — starts HTTP/SSE server via `HyperServer`/`HyperRuntime`
- Tool definitions built via `all_tools(http_mode)` helper
- `BeretHandler` helper methods: `get_arg`, `require_arg`, `ok_json`, `ok_text`, `err`
- `BeretHandler.root` is `RwLock<PathBuf>` — mutable for `index_repo`
- IMPORTANT: `rust_mcp_sdk::schema::*` exports a `Result` struct that shadows `std::result::Result` — always use explicit imports
- HTTP server: `hyper_server::create_server()` → `HyperRuntime::create()` → `.await_server()`

### tools.rs — Query Tools + Live Search
Pre-built SPARQL queries and live search functions called by the handler:
- `find_symbol(store, name)` — SPARQL FILTER CONTAINS on subject IRIs
- `find_callers(store, name)` — SPARQL on `calls` predicate, filter callee
- `find_callees(store, name)` — SPARQL on `calls` predicate, filter caller
- `list_structures(store, path, kind)` — SPARQL with optional FILTER clauses
- `file_stats(store)` — SPARQL COUNT/GROUP BY on types
- `find_dead_code(store)` — two-pass: get all functions + all call targets, diff in Rust
- `find_dependencies(store)` — SPARQL on `dependsOn` predicate
- `find_entry_points(store)` — SPARQL FILTER for main/index/app/server/cli patterns
- `search_pattern(root, pattern, language)` — live ast-grep walk, returns file/line/text (max 200 results)

### store.rs — CodebaseStore
- Wraps `oxigraph::store::Store` (in-memory RDF)
- `insert_triple(s, p, o)` — adds triples with `repo:` prefix (`http://repo.example.org/`)
- `query_to_json(sparql)` — runs SELECT/ASK queries, returns `serde_json::Value`
- `clear()` — wipes store for re-indexing

### ingestor.rs — Parallel Ingestion
- Three extraction tiers:
  1. **AST** (17 languages): `LangConfig` with `NameStrategy`/`CallStrategy` enums
  2. **Non-code text**: JSON, YAML, Markdown, HTML, CSS
  3. **Binary metadata**: MIME type + file size for 30+ extensions
- All user-facing text sanitized via `iri_safe()` (allowlist approach for IRI characters)

## MCP Tools

### Always available
| Tool | Purpose |
|------|---------|
| `query_codebase` | Raw SPARQL queries against the knowledge graph |
| `refresh_index` | Clear and re-ingest the codebase |
| `find_symbol` | Find definitions by name (partial match) |
| `find_callers` | Reverse call graph: who calls function X? |
| `find_callees` | Forward call graph: what does function X call? |
| `list_structures` | List all structures, optionally filtered by path or kind |
| `file_stats` | Count of each type (Function, Class, Config, etc.) |
| `find_dead_code` | Functions defined but never called |
| `find_dependencies` | Package dependencies from config files |
| `find_entry_points` | Find main/index/app/server entry points |
| `search_pattern` | Live ast-grep structural pattern search |

### HTTP mode only
| Tool | Purpose |
|------|---------|
| `index_repo` | Clone a git repo and index it |

## Distribution
- **crates.io:** `cargo install chapeaux-beret` (published as `chapeaux-beret`)
- **npm:** `npx @chapeaux/beret` (downloads platform binary from GitHub releases)
- **JSR:** `npx jsr:@chapeaux/beret` (same mechanism)
- **CI:** `.github/workflows/release.yml` builds for 5 targets, publishes to GitHub Releases + crates.io + npm

## Build
- Release profile: `lto = true`, `codegen-units = 1`

## Conventions
- Edition 2024
- Error type: `Box<dyn std::error::Error>`
- `std::result::Result` always qualified (schema `Result` conflict)
