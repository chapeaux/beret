# Beret — Architecture

## Overview
Beret is a high-performance Rust MCP server in the `chapeaux` project family. It combines an RDF knowledge graph (oxigraph), structural code parsing (ast-grep), async runtime (tokio), and Model Context Protocol (rust-mcp-sdk) to expose codebase intelligence via SPARQL queries. It supports both stdio and HTTP/SSE transports.

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
- Three tools:
  - **`query_codebase`**: SPARQL queries → JSON results
  - **`refresh_index`**: clears + re-ingests
  - **`index_repo`** (HTTP mode only): `git clone --depth 1` → index (reuses via `git pull`)
- `BeretHandler.root` is `RwLock<PathBuf>` — mutable for `index_repo`
- IMPORTANT: `rust_mcp_sdk::schema::*` exports a `Result` struct that shadows `std::result::Result` — always use explicit imports
- HTTP server: `hyper_server::create_server()` → `HyperRuntime::create()` → `.await_server()`

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

## Distribution
- **crates.io:** `cargo install chapeaux-beret` (published as `chapeaux-beret`)
- **npm:** `npx @chapeaux/beret` (downloads platform binary from GitHub releases)
- **JSR:** `npx jsr:@chapeaux/beret` (same mechanism)

## Build
- Release profile: `lto = true`, `codegen-units = 1`

## Conventions
- Edition 2024
- Error type: `Box<dyn std::error::Error>`
- `std::result::Result` always qualified (schema `Result` conflict)
