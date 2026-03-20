# Beret

An MCP server that builds an in-memory RDF knowledge graph of your codebase and exposes it through purpose-built tools and SPARQL queries. It parses 17 programming languages using structural AST analysis, extracts metadata from config files and documents, records binary file information, and lets you explore everything through the Model Context Protocol.

## Installation

| Method | Command | Requirements |
|--------|---------|--------------|
| npx | `npx @chapeaux/beret` | Node.js |
| npx (JSR) | `npx jsr:@chapeaux/beret` | Node.js |
| Cargo | `cargo install chapeaux-beret` | Rust 1.85+ |
| Source | `cargo build --release` | Rust 1.85+ |

The release profile enables LTO and single codegen unit for maximum binary performance.

## Usage

The server starts with an empty knowledge graph and indexes on demand. Call `refresh_index` with a directory path to populate the graph, then use the other tools to explore. This lets you launch one server and re-target it at different projects or subdirectories as you work.

| Mode | Command | Transport | `index_repo` tool |
|------|---------|-----------|-------------------|
| Local | `beret` | stdio | No |
| Remote | `beret --serve 8080` | HTTP/SSE | Yes |
| Remote (bind all) | `beret --serve 0.0.0.0:9090` | HTTP/SSE | Yes |

An optional `[PATH]` argument (e.g., `beret /path/to/project`) sets the default directory for `refresh_index` when called without a path. If omitted, defaults to the current working directory.

## MCP client configuration

| Setup | Install required | Config |
|-------|-----------------|--------|
| Stdio | `cargo install chapeaux-beret` | `{"command": "beret"}` |
| Stdio via npx | None | `{"command": "npx", "args": ["-y", "@chapeaux/beret"]}` |
| Stdio via JSR | None | `{"command": "npx", "args": ["-y", "jsr:@chapeaux/beret"]}` |
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

After connecting, ask the agent to index a directory (e.g., "index the current project") or for remote HTTP, a repository (e.g., "index https://github.com/user/repo").

## Tools

All list-returning tools support `limit`, `offset`, and `exclude` parameters. `exclude` takes a comma-separated list of directory names to filter out (e.g., `"docs,stories,dist"`). When results exceed the limit, the response includes `has_more: true` and `next_offset` for pagination.

### Exploration tools

| Tool | Parameters | Default limit | What it does |
|------|-----------|--------------|-------------|
| `file_stats` | `exclude?` | — | Count of functions, classes, configs, documents, binaries, etc. Start here for a project overview. |
| `find_entry_points` | `exclude?`, `limit?`, `offset?` | 100 | Find main functions, index files, app/server/cli modules. Answers: "Where does this app start?" |
| `list_structures` | `path?`, `kind?`, `exclude?`, `limit?`, `offset?` | 200 | List all indexed structures. Filter by path substring and/or kind (`Function`, `Class`, `Config`, `Document`, `Binary`, `Stylesheet`, `Section`, `Style`, `Element`). |
| `find_symbol` | `name`, `exclude?`, `limit?`, `offset?` | 100 | Find where a function, class, struct, or module is defined. Partial name match. |

### Call graph tools

| Tool | Parameters | Default limit | What it does |
|------|-----------|--------------|-------------|
| `find_callers` | `name?`, `exclude?`, `limit?`, `offset?` | 100 | Who calls this function? Omit name to get all call edges. |
| `find_callees` | `name?`, `exclude?`, `limit?`, `offset?` | 100 | What does this function call? Omit name to get all call edges. |
| `find_dead_code` | `exclude?`, `limit?`, `offset?` | 100 | Functions defined but never called anywhere. Find unused code and refactoring candidates. |

### Dependency & config tools

| Tool | Parameters | Default limit | What it does |
|------|-----------|--------------|-------------|
| `find_dependencies` | `exclude?`, `limit?`, `offset?` | 200 | List all external package dependencies from package.json (dependencies, devDependencies, peerDependencies). |

### Code search tools

| Tool | Parameters | Default limit | What it does |
|------|-----------|--------------|-------------|
| `search_pattern` | `pattern`, `language`, `exclude?`, `limit?`, `offset?` | 200 | Structural AST search using ast-grep syntax. Unlike text search, matches code structure. Use `$NAME` for wildcards, `$$$ARGS` for variadic. |
| `query_codebase` | `sparql`, `limit?`, `offset?` | 500 | Raw SPARQL SELECT/ASK against the knowledge graph for advanced queries. |

### Visualization tools

| Tool | Parameters | Default limit | What it does |
|------|-----------|--------------|-------------|
| `generate_diagram` | `path?`, `depth?`, `code_only?`, `exclude?`, `limit?` | 500 elements | Generate a [LikeC4](https://likec4.dev) architecture diagram. Auto-adjusts depth based on codebase size. |

- **`depth`**: `0` = auto (default), `1` = directories, `2` = +files, `3` = +functions/classes
- **`code_only`**: `true` (default) — excludes docs, configs, binaries, test dirs, stories, etc.
- **`exclude`**: comma-separated directory names to additionally exclude
- **Beret theme**: custom colors (navy `#00005F`, orange `#F5921B`, gold `#FFCC17`, teal `#37A3A3`, blue `#0066CC`), distinct shapes per element type, styled relationship arrows
- **Output**: paste into [playground.likec4.dev](https://playground.likec4.dev/), save as `.c4`, or render with `npx likec4`

### Practice analysis tools

| Tool | Parameters | What it does |
|------|-----------|-------------|
| `describe_practices` | — | High-level summary of all detected engineering practices across 11 categories. |
| `describe_testing` | — | Test frameworks, test function count, test-to-code ratio. |
| `describe_ci_cd` | — | CI platforms, containerization, build tools, infrastructure-as-code. |
| `describe_code_quality` | — | Linters, formatters, type checkers, conventions (git hooks, lint-staged, etc.). |
| `describe_architecture` | — | Architecture layers, monorepo detection, structure counts. |
| `describe_documentation` | — | Documentation artifacts (license, changelog, contributing, codeowners, etc.). |
| `describe_dependencies` | — | Package managers, dependency count, automated update tools. |

### Index management tools

| Tool | Parameters | What it does |
|------|-----------|-------------|
| `refresh_index` | `path?` | Index a directory and build the knowledge graph. Call this first before using other tools. |
| `index_repo` *(HTTP only)* | `url` | Clone a git repo (or pull if already cloned) and index it. |

### Pagination

When results are paginated, the response wraps results with metadata:

```json
{
  "results": [...],
  "total": 1847,
  "returned": 200,
  "offset": 0,
  "has_more": true,
  "next_offset": 200,
  "message": "Showing 1–200 of 1847 results. Use offset: 200 to see the next page."
}
```

### `search_pattern` examples

Find all Rust functions:
```
pattern: "fn $NAME($$$ARGS) { $$$BODY }"
language: "rust"
```

Find all console.log calls in JavaScript:
```
pattern: "console.log($MSG)"
language: "javascript"
```

Find Python classes inheriting from a base:
```
pattern: "class $NAME($BASE): $$$BODY"
language: "python"
```

### `query_codebase` SPARQL examples

All IRIs use the `repo:` prefix, which expands to `http://repo.example.org/`.

List all functions:
```sparql
SELECT ?func WHERE {
  ?func <http://repo.example.org/a> <http://repo.example.org/Function>
}
```

Find all callers of a function:
```sparql
SELECT ?caller WHERE {
  ?caller <http://repo.example.org/calls> <http://repo.example.org/process>
}
```

Find all dependencies:
```sparql
SELECT ?file ?dep WHERE {
  ?file <http://repo.example.org/dependsOn> ?dep
}
```

List binary files with MIME types:
```sparql
SELECT ?file ?mime WHERE {
  ?file <http://repo.example.org/a> <http://repo.example.org/Binary> .
  ?file <http://repo.example.org/hasMimeType> ?mime
}
```

Check if any call relationships exist:
```sparql
ASK { ?a <http://repo.example.org/calls> ?b }
```

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

### Engineering practices

Detected from file presence and directory structure. All use `<project>` as subject.

| Predicate | Example values | Detected from |
|-----------|---------------|---------------|
| `usesCIPlatform` | github-actions, gitlab-ci, jenkins | `.github/workflows/`, `.gitlab-ci.yml`, `Jenkinsfile` |
| `usesTestFramework` | jest, pytest, cypress, playwright | `jest.config.*`, `pytest.ini`, `cypress.config.*` |
| `usesLinter` | eslint, biome, ruff, rubocop | `.eslintrc*`, `biome.json`, `ruff.toml` |
| `usesFormatter` | prettier, editorconfig | `.prettierrc*`, `.editorconfig` |
| `usesBuildTool` | make, gradle, maven, cmake | `Makefile`, `build.gradle`, `pom.xml` |
| `usesContainerization` | docker, docker-compose | `Dockerfile`, `docker-compose.yml` |
| `usesPackageManager` | npm, yarn, pnpm, cargo, pip | `package.json`, `yarn.lock`, `Cargo.toml` |
| `usesTypeChecking` | typescript, mypy | `tsconfig.json`, `mypy.ini` |
| `hasLayer` | source, tests, api, ui, domain, services | Directory names: `src/`, `tests/`, `api/`, etc. |
| `hasDocumentation` | license, changelog, contributing-guide, codeowners | `LICENSE`, `CHANGELOG.md`, `CONTRIBUTING.md` |
| `followsConvention` | conventional-commits, git-hooks, lint-staged | `.commitlintrc*`, `.husky/`, `renovate.json` |

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
