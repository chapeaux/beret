# Beret

An MCP server that builds an in-memory RDF knowledge graph of your codebase and exposes it via SPARQL queries. It parses 17 programming languages using structural AST analysis, extracts metadata from config files and documents, records binary file information, and lets you query everything through the Model Context Protocol.

## Installation

| Method | Command | Requirements |
|--------|---------|--------------|
| npx | `npx @chapeaux/beret` | Node.js |
| npx (JSR) | `npx jsr:@chapeaux/beret` | Node.js |
| Cargo | `cargo install chapeaux-beret` | Rust 1.85+ |
| Source | `cargo build --release` | Rust 1.85+ |

The release profile enables LTO and single codegen unit for maximum binary performance.

## Usage

| Mode | Command | Transport | Indexing | `index_repo` tool |
|------|---------|-----------|----------|--------------------|
| Local | `beret /path/to/project` | stdio | Indexes given path on startup | No |
| Local (cwd) | `beret` | stdio | Indexes current directory on startup | No |
| Remote | `beret --serve 8080` | HTTP/SSE | On demand via `index_repo` | Yes |
| Remote (bind all) | `beret --serve 0.0.0.0:9090` | HTTP/SSE | On demand via `index_repo` | Yes |

## MCP client configuration

| Setup | Install required | Config |
|-------|-----------------|--------|
| Stdio | `cargo install chapeaux-beret` | `{"command": "beret", "args": ["/path/to/project"]}` |
| Stdio via npx | None | `{"command": "npx", "args": ["-y", "@chapeaux/beret", "/path/to/project"]}` |
| Stdio via JSR | None | `{"command": "npx", "args": ["-y", "jsr:@chapeaux/beret", "/path/to/project"]}` |
| Remote HTTP | None (server runs elsewhere) | `{"url": "http://your-server:8080/sse"}` |

All entries are placed in 
```
{
  "mcpServers": { 
    "beret": ...
  }
}
``` 
in your client config (e.g., `mcp.json`, `claude_desktop_config.json`).

For remote HTTP, ask the agent to index a repository after connecting: "index https://github.com/user/repo".

## Tools

### `query_codebase`

Execute a SPARQL query against the knowledge graph.

**Parameters:**
- `sparql` (string, required) — A SPARQL SELECT or ASK query.

All IRIs use the `repo:` prefix, which expands to `http://repo.example.org/`.

**Example queries:**

List all functions:
```sparql
SELECT ?func WHERE {
  ?func <http://repo.example.org/a> <http://repo.example.org/Function>
}
```

List all classes:
```sparql
SELECT ?cls WHERE {
  ?cls <http://repo.example.org/a> <http://repo.example.org/Class>
}
```

Find what a specific function calls:
```sparql
SELECT ?callee WHERE {
  <http://repo.example.org/src/main.rs/main> <http://repo.example.org/calls> ?callee
}
```

Find all callers of a function:
```sparql
SELECT ?caller WHERE {
  ?caller <http://repo.example.org/calls> <http://repo.example.org/process>
}
```

Find all dependencies declared in package.json:
```sparql
SELECT ?file ?dep WHERE {
  ?file <http://repo.example.org/dependsOn> ?dep
}
```

List all binary files with their MIME types:
```sparql
SELECT ?file ?mime WHERE {
  ?file <http://repo.example.org/a> <http://repo.example.org/Binary> .
  ?file <http://repo.example.org/hasMimeType> ?mime
}
```

List all config file keys:
```sparql
SELECT ?file ?key WHERE {
  ?file <http://repo.example.org/a> <http://repo.example.org/Config> .
  ?file <http://repo.example.org/declares> ?key
}
```

Find all CSS selectors:
```sparql
SELECT ?selector WHERE {
  ?selector <http://repo.example.org/a> <http://repo.example.org/Style>
}
```

Check if any call relationships exist:
```sparql
ASK { ?a <http://repo.example.org/calls> ?b }
```

### `refresh_index`

Clear the knowledge graph and re-ingest all files. In stdio mode, re-reads the local directory. In HTTP mode, re-reads the last cloned repository.

**Parameters:** none.

### `index_repo` (HTTP mode only)

Clone a git repository and index it into the knowledge graph. Clears any existing index first. On subsequent calls with the same URL, pulls latest changes instead of re-cloning.

**Parameters:**
- `url` (string, required) — Git repository URL (HTTPS or SSH).

## Knowledge graph schema

### Code files

| Triple | Meaning |
|--------|---------|
| `<file/name> repo:a repo:Function` | A function or method definition |
| `<file/name> repo:a repo:Class` | A class, struct, interface, trait, or module definition |
| `<caller> repo:calls <callee>` | A function call relationship |

Subjects are qualified with the file path (e.g., `src/main.rs/handle_request`). Call targets are the unqualified callee name. For method calls like `obj.method()`, only `method` is recorded. For namespaced calls like `mod::func()`, only `func` is recorded.

### Config files (JSON, YAML)

| Triple | Meaning |
|--------|---------|
| `<file> repo:a repo:Config` | A configuration file |
| `<file> repo:declares <key>` | A top-level key in the file |
| `<file> repo:dependsOn <pkg>` | A dependency (package.json only) |

### Documents (Markdown, HTML)

| Triple | Meaning |
|--------|---------|
| `<file> repo:a repo:Document` | A document file |
| `<file/heading> repo:a repo:Section` | A markdown heading |
| `<file/#id> repo:a repo:Element` | An HTML element with an id |
| `<file/.class> repo:a repo:Element` | An HTML element with a class |

### Stylesheets (CSS)

| Triple | Meaning |
|--------|---------|
| `<file> repo:a repo:Stylesheet` | A CSS file |
| `<file/selector> repo:a repo:Style` | A CSS selector rule |

### Binary files

| Triple | Meaning |
|--------|---------|
| `<file> repo:a repo:Binary` | A binary file |
| `<file> repo:hasMimeType <mime>` | The file's MIME type |
| `<file> repo:hasSize <bytes>` | The file size in bytes |

### Supported languages

| Language | Extensions | Functions | Classes/Types | Calls |
|----------|-----------|-----------|---------------|-------|
| Python | `.py` | `def` | `class` | calls |
| Rust | `.rs` | `fn` | `struct`, `impl` | call expressions |
| JavaScript | `.js`, `.mjs`, `.cjs` | `function` | `class` | call expressions |
| TypeScript | `.ts` | `function` | `class`, `interface` | call expressions |
| TSX | `.tsx` | `function` | `class`, `interface` | call expressions |
| Go | `.go` | `func` | type specs | call expressions |
| Java | `.java` | methods | `class`, `interface`, `enum` | method invocations |
| C | `.c` | functions | `struct` | call expressions |
| C++ | `.cpp`, `.cc`, `.cxx`, `.hpp` | functions | `struct`, `class` | call expressions |
| C# | `.cs` | methods | `class`, `interface`, `struct`, `enum` | invocations |
| Ruby | `.rb` | `def` | `class`, `module` | calls |
| PHP | `.php` | `function`, methods | `class`, `interface`, `enum` | calls |
| Kotlin | `.kt`, `.kts` | `fun` | `class`, `object` | call expressions |
| Swift | `.swift` | `func` | `class`, `struct` | call expressions |
| Scala | `.scala`, `.sc` | `def` | `class`, `object`, `trait` | call expressions |
| Bash | `.sh`, `.bash` | `function` | — | commands |
| Lua | `.lua` | `function` | — | function calls |

### Non-code files

| Type | Extensions | What is extracted |
|------|-----------|-------------------|
| JSON | `.json` | Top-level keys, package.json dependencies |
| YAML | `.yml`, `.yaml` | Top-level keys |
| Markdown | `.md`, `.markdown` | Headings |
| HTML | `.html`, `.htm` | Element IDs and classes |
| CSS | `.css` | Selectors |

### Binary files (metadata only)

Images (`.png`, `.jpg`, `.gif`, `.webp`, `.svg`, `.ico`, `.bmp`), audio (`.mp3`, `.wav`, `.ogg`, `.flac`, `.aac`), video (`.mp4`, `.webm`, `.avi`, `.mov`, `.mkv`), fonts (`.ttf`, `.otf`, `.woff`, `.woff2`), documents (`.pdf`), archives (`.zip`, `.gz`, `.tar`), executables (`.exe`, `.dll`, `.so`, `.dylib`, `.wasm`), databases (`.sqlite`, `.db`).

## Performance

The ingestor uses parallel file walking (via the `ignore` crate) and direct tree-sitter node kind matching for speed. Benchmarked at 5,000 files / 60,000 triples in under 1 second in debug mode.

## Publishing

### Crates.io

```sh
cargo publish
```

Publishes as `chapeaux-beret`. Users install with `cargo install chapeaux-beret`.

### npm

```sh
cd npm && npm publish --access public
```

Publishes as `@chapeaux/beret`. Requires pre-built binaries uploaded to GitHub releases.

### JSR

```sh
cd npm && npx jsr publish
```

Publishes as `@chapeaux/beret` on JSR. Users run with `npx jsr:@chapeaux/beret`.

## Testing

```sh
cargo test
```

## License

MIT
