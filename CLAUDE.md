# Beret — Architecture

## Overview
Beret is a high-performance Rust MCP server in the `chapeaux` project family. It combines an RDF knowledge graph (oxigraph), structural code parsing (ast-grep), async runtime (tokio), and Model Context Protocol (rust-mcp-sdk) to expose codebase intelligence via purpose-built tools, SPARQL queries, and LikeC4 architecture diagrams. It supports both stdio and HTTP/SSE transports.

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
    ├── tools.rs      # Query tools, live ast-grep search, LikeC4 diagram generation
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
  - **Stdio:** `beret [PATH]` — starts with empty graph, `PATH` sets default root for `refresh_index` (defaults to cwd)
  - **HTTP:** `beret --serve [HOST:]PORT` — starts HTTP/SSE server, indexes on demand via `refresh_index` or `index_repo`
- No startup indexing — graph is empty until `refresh_index` is called
- Tool definitions built via `all_tools(http_mode)` helper
- `BeretHandler` helper methods: `get_arg`, `require_arg`, `get_limit`, `get_offset`, `ok_json`, `ok_json_limited`, `ok_text`, `err`
- `ok_json_limited(value, limit, offset)` — pagination wrapper: returns `{results, total, returned, offset, has_more, next_offset, message}`
- `BeretHandler.root` is `RwLock<PathBuf>` — updated by `refresh_index(path)` and `index_repo`
- IMPORTANT: `rust_mcp_sdk::schema::*` exports a `Result` struct that shadows `std::result::Result` — always use explicit imports
- HTTP server: `hyper_server::create_server()` → `HyperRuntime::create()` → `.await_server()`

### tools.rs — Query Tools, Live Search, Diagram Generation
Pre-built SPARQL queries, live search, and visualization:
- `find_symbol(store, name)` — SPARQL FILTER CONTAINS on subject IRIs
- `find_callers(store, name)` — SPARQL on `calls` predicate, filter callee
- `find_callees(store, name)` — SPARQL on `calls` predicate, filter caller
- `list_structures(store, path, kind)` — SPARQL with optional FILTER clauses
- `file_stats(store)` — SPARQL COUNT/GROUP BY on types
- `find_dead_code(store)` — two-pass: get all functions + all call targets, diff in Rust
- `find_dependencies(store)` — SPARQL on `dependsOn` predicate
- `find_entry_points(store)` — SPARQL FILTER for main/index/app/server/cli patterns
- `search_pattern(root, pattern, language, limit)` — live ast-grep walk, returns file/line/text
- `generate_diagram(store, scope, depth, limit)` — queries graph, builds LikeC4 DSL with specification/model/views blocks; `depth` controls nesting (1=dirs, 2=+files, 3=+symbols)

### store.rs — CodebaseStore
- Wraps `oxigraph::store::Store` (in-memory RDF)
- `insert_triple(s, p, o)` — adds triples with `repo:` prefix, percent-encodes via `iri_escape()`
- `query_to_json(sparql)` — runs SELECT/ASK queries, returns `serde_json::Value`
- `clear()` — wipes store for re-indexing

### ingestor.rs — Parallel Ingestion
- Three extraction tiers:
  1. **AST** (17 languages): `LangConfig` with `NameStrategy`/`CallStrategy` enums
  2. **Non-code text**: JSON, YAML, Markdown, HTML, CSS
  3. **Binary metadata**: MIME type + file size for 30+ extensions
- All user-facing text sanitized via `iri_safe()` (allowlist approach for IRI characters)

## MCP Tools

All list-returning tools support `limit` and `offset` parameters for pagination. When results are paginated, the response includes `{results, total, returned, offset, has_more, next_offset, message}`.

### Always available
| Tool | Default limit | Purpose |
|------|--------------|---------|
| `refresh_index` | — | Index a directory (optional `path` param). Call this first. |
| `query_codebase` | 500 | Raw SPARQL queries against the knowledge graph |
| `find_symbol` | 100 | Find definitions by name (partial match) |
| `find_callers` | 100 | Reverse call graph: who calls function X? |
| `find_callees` | 100 | Forward call graph: what does function X call? |
| `list_structures` | 200 | List all structures, optionally filtered by path or kind |
| `file_stats` | — | Count of each type (Function, Class, Config, etc.) |
| `find_dead_code` | 100 | Functions defined but never called |
| `find_dependencies` | 200 | Package dependencies from config files |
| `find_entry_points` | 100 | Find main/index/app/server entry points |
| `search_pattern` | 200 | Live ast-grep structural pattern search |
| `generate_diagram` | 200 | Generate LikeC4 architecture diagram DSL |

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
