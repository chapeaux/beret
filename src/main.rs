mod ingestor;
mod store;
mod tools;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use rust_mcp_sdk::mcp_server::{server_runtime, McpServerOptions, ServerHandler};
use rust_mcp_sdk::schema::{
    CallToolError, CallToolRequestParams, CallToolResult, Implementation, InitializeResult,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, RpcError, ServerCapabilities,
    ServerCapabilitiesTools, TextContent, Tool, ToolInputSchema,
};
use rust_mcp_sdk::{McpServer, StdioTransport, ToMcpServerHandler, TransportOptions};
use serde_json::{Map, Value};

use crate::ingestor::ingest;
use crate::store::CodebaseStore;

// --- CLI parsing ---

enum Mode {
    Stdio { root: PathBuf },
    Http { host: String, port: u16 },
}

fn print_usage() {
    eprintln!(
        "Usage: beret [OPTIONS] [PATH]

MCP server that builds a SPARQL-queryable knowledge graph of your codebase.

Arguments:
  [PATH]    Directory to index (defaults to current directory)

Options:
  --serve [HOST:]PORT   Start HTTP/SSE server instead of stdio
  --help                Show this help message
  --version             Show version

Examples:
  beret                           # stdio, index cwd
  beret /path/to/project          # stdio, index given path
  beret --serve 8080              # HTTP on 127.0.0.1:8080
  beret --serve 0.0.0.0:9090      # HTTP on all interfaces, port 9090"
    );
}

fn parse_args() -> std::result::Result<Mode, Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--version" | "-V" => {
                eprintln!("beret {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--serve" => {
                i += 1;
                let addr = args.get(i).ok_or("--serve requires HOST:PORT or PORT")?;
                let (host, port) = if let Some(colon) = addr.rfind(':') {
                    let host = addr[..colon].to_string();
                    let port: u16 = addr[colon + 1..].parse().map_err(|_| "invalid port")?;
                    (host, port)
                } else {
                    let port: u16 = addr.parse().map_err(|_| "invalid port")?;
                    ("127.0.0.1".to_string(), port)
                };
                return Ok(Mode::Http { host, port });
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}").into());
            }
            path => {
                return Ok(Mode::Stdio {
                    root: PathBuf::from(path),
                });
            }
        }
    }

    Ok(Mode::Stdio {
        root: std::env::current_dir()?,
    })
}

// --- Tool definitions ---

fn make_tool_input_schema(
    properties: &[(&str, &str, &str)],
    required: &[&str],
) -> ToolInputSchema {
    let mut props = BTreeMap::new();
    for &(name, ty, desc) in properties {
        let mut schema = Map::new();
        schema.insert("type".to_string(), Value::String(ty.to_string()));
        schema.insert("description".to_string(), Value::String(desc.to_string()));
        props.insert(name.to_string(), schema);
    }
    ToolInputSchema::new(
        required.iter().map(|s| s.to_string()).collect(),
        Some(props),
        None,
    )
}

fn tool(name: &str, title: &str, desc: &str, props: &[(&str, &str, &str)], required: &[&str]) -> Tool {
    Tool {
        name: name.to_string(),
        title: Some(title.to_string()),
        description: Some(desc.to_string()),
        input_schema: make_tool_input_schema(props, required),
        annotations: None,
        execution: None,
        icons: vec![],
        meta: None,
        output_schema: None,
    }
}

fn all_tools(http_mode: bool) -> Vec<Tool> {
    let mut t = vec![
        tool(
            "query_codebase", "Query Codebase",
            "Execute a SPARQL SELECT or ASK query against the codebase knowledge graph. \
             The graph uses the repo: prefix (http://repo.example.org/). \
             Triples include: <file/func> a repo:Function, <file/class> a repo:Class, \
             <caller> repo:calls <callee>, <file> repo:dependsOn <pkg>, etc.",
            &[("sparql", "string", "A SPARQL query string")],
            &["sparql"],
        ),
        tool(
            "refresh_index", "Refresh Index",
            "Scan a directory and rebuild the knowledge graph. If a path is provided, \
             indexes that directory. If omitted, re-indexes the last indexed directory \
             (or the current working directory if nothing has been indexed yet). \
             Call this before using any other tool to populate the graph.",
            &[("path", "string", "Directory path to index (defaults to last indexed path or cwd)")],
            &[],
        ),
        tool(
            "find_symbol", "Find Symbol",
            "Find where a function, class, struct, or module is defined by name. \
             Returns all definitions whose name contains the search term.",
            &[("name", "string", "Symbol name to search for (partial match)")],
            &["name"],
        ),
        tool(
            "find_callers", "Find Callers",
            "Find all functions that call a given function. \
             Answers: 'Who calls this function?' and 'What depends on this?'",
            &[("name", "string", "Function name to find callers of")],
            &["name"],
        ),
        tool(
            "find_callees", "Find Callees",
            "Find all functions called by a given function. \
             Answers: 'What does this function depend on?' and 'What does it call?'",
            &[("name", "string", "Function name to find callees of")],
            &["name"],
        ),
        tool(
            "list_structures", "List Structures",
            "List all functions, classes, configs, documents, and other structures in the codebase. \
             Optionally filter by file path and/or kind (Function, Class, Config, Document, \
             Binary, Stylesheet, Section, Style, Element).",
            &[
                ("path", "string", "Filter results to entries containing this path substring"),
                ("kind", "string", "Filter to a specific kind: Function, Class, Config, Document, Binary, Stylesheet, Section, Style, Element"),
            ],
            &[],
        ),
        tool(
            "file_stats", "File Statistics",
            "Get a summary of the indexed codebase: counts of functions, classes, configs, \
             documents, binaries, styles, etc. Useful for understanding the tech stack and \
             project structure at a glance.",
            &[], &[],
        ),
        tool(
            "find_dead_code", "Find Dead Code",
            "Find functions that are defined but never called anywhere in the codebase. \
             Helps identify unused code, deprecated functions, and refactoring candidates.",
            &[], &[],
        ),
        tool(
            "find_dependencies", "Find Dependencies",
            "List all external package dependencies declared in config files \
             (package.json dependencies, devDependencies, peerDependencies).",
            &[], &[],
        ),
        tool(
            "find_entry_points", "Find Entry Points",
            "Find likely application entry points: main functions, index files, app modules, \
             server files, and CLI handlers. Answers: 'Where does this application start?'",
            &[], &[],
        ),
        tool(
            "search_pattern", "Search Pattern",
            "Search the codebase for a structural code pattern using ast-grep syntax. \
             Unlike text search, this matches AST structure. Use $NAME for wildcards. \
             Examples: 'fn $NAME($$$ARGS)' finds all Rust functions, \
             'console.log($MSG)' finds all console.log calls in JS/TS. \
             Returns file, line number, and matched text (max 200 results).",
            &[
                ("pattern", "string", "ast-grep pattern to search for (use $NAME for wildcards)"),
                ("language", "string", "Language to search in: python, rust, javascript, typescript, tsx, go, java, c, cpp, csharp, ruby, php, kotlin, swift, scala, bash, lua"),
            ],
            &["pattern", "language"],
        ),
    ];

    if http_mode {
        t.push(tool(
            "index_repo", "Index Repository",
            "Clone a git repository and index it into the knowledge graph. \
             Clears any existing index first. Accepts a git URL (HTTPS or SSH).",
            &[("url", "string", "Git repository URL to clone and index")],
            &["url"],
        ));
    }

    t
}

// --- Handler ---

struct BeretHandler {
    store: Arc<CodebaseStore>,
    root: std::sync::RwLock<PathBuf>,
    http_mode: bool,
}

impl BeretHandler {
    fn get_arg<'a>(params: &'a CallToolRequestParams, key: &str) -> Option<&'a str> {
        params
            .arguments
            .as_ref()
            .and_then(|args| args.get(key))
            .and_then(|v| v.as_str())
    }

    fn require_arg<'a>(
        params: &'a CallToolRequestParams,
        tool_name: &str,
        key: &str,
    ) -> std::result::Result<&'a str, CallToolError> {
        Self::get_arg(params, key).ok_or_else(|| {
            CallToolError::invalid_arguments(tool_name, Some(format!("missing '{key}' argument")))
        })
    }

    fn ok_json(value: Value) -> std::result::Result<CallToolResult, CallToolError> {
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Ok(CallToolResult::text_content(vec![TextContent::new(
            text, None, None,
        )]))
    }

    fn ok_text(msg: String) -> std::result::Result<CallToolResult, CallToolError> {
        Ok(CallToolResult::text_content(vec![TextContent::new(
            msg, None, None,
        )]))
    }

    fn err(msg: String) -> CallToolError {
        CallToolError::from_message(msg)
    }

    fn do_refresh(&self, path: Option<&str>) -> std::result::Result<String, String> {
        if let Some(p) = path {
            let resolved = std::path::Path::new(p)
                .canonicalize()
                .map_err(|e| format!("invalid path '{}': {}", p, e))?;
            *self.root.write().unwrap() = resolved;
        }
        let root = self.root.read().unwrap().clone();
        self.store.clear().map_err(|e| e.to_string())?;
        let count = ingest(&root, &self.store).map_err(|e| e.to_string())?;
        Ok(format!("Indexed {} triples from {}", count, root.display()))
    }

    fn do_index_repo(&self, url: &str) -> std::result::Result<String, String> {
        let temp_dir = std::env::temp_dir().join(format!("beret-{}", hash_url(url)));

        if temp_dir.exists() {
            std::process::Command::new("git")
                .args(["pull", "--ff-only"])
                .current_dir(&temp_dir)
                .output()
                .map_err(|e| format!("git pull failed: {e}"))?;
        } else {
            let status = std::process::Command::new("git")
                .args(["clone", "--depth", "1", url])
                .arg(&temp_dir)
                .status()
                .map_err(|e| format!("git clone failed: {e}"))?;
            if !status.success() {
                return Err(format!("git clone exited with status {status}"));
            }
        }

        *self.root.write().unwrap() = temp_dir.clone();
        self.store.clear().map_err(|e| e.to_string())?;
        let count = ingest(&temp_dir, &self.store).map_err(|e| e.to_string())?;
        Ok(format!("Cloned and indexed {} triples from {}", count, url))
    }
}

fn hash_url(url: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    hasher.finish()
}

#[async_trait]
impl ServerHandler for BeretHandler {
    async fn handle_list_tools_request(
        &self,
        _params: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: all_tools(self.http_mode),
            next_cursor: None,
            meta: None,
        })
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        match params.name.as_str() {
            "query_codebase" => {
                let sparql = Self::require_arg(&params, "query_codebase", "sparql")?;
                match self.store.query_to_json(sparql) {
                    Ok(v) => Self::ok_json(v),
                    Err(e) => Err(Self::err(format!("SPARQL query error: {e}"))),
                }
            }

            "refresh_index" => {
                let path = Self::get_arg(&params, "path");
                match self.do_refresh(path) {
                    Ok(msg) => Self::ok_text(msg),
                    Err(e) => Err(Self::err(format!("Refresh error: {e}"))),
                }
            }

            "find_symbol" => {
                let name = Self::require_arg(&params, "find_symbol", "name")?;
                tools::find_symbol(&self.store, name)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "find_callers" => {
                let name = Self::require_arg(&params, "find_callers", "name")?;
                tools::find_callers(&self.store, name)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "find_callees" => {
                let name = Self::require_arg(&params, "find_callees", "name")?;
                tools::find_callees(&self.store, name)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "list_structures" => {
                let path = Self::get_arg(&params, "path");
                let kind = Self::get_arg(&params, "kind");
                tools::list_structures(&self.store, path, kind)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "file_stats" => {
                tools::file_stats(&self.store)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "find_dead_code" => {
                tools::find_dead_code(&self.store)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "find_dependencies" => {
                tools::find_dependencies(&self.store)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "find_entry_points" => {
                tools::find_entry_points(&self.store)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "search_pattern" => {
                let pattern = Self::require_arg(&params, "search_pattern", "pattern")?;
                let language = Self::require_arg(&params, "search_pattern", "language")?;
                let root = self.root.read().unwrap().clone();
                tools::search_pattern(&root, pattern, language)
                    .map_or_else(|e| Err(Self::err(e)), Self::ok_json)
            }

            "index_repo" => {
                if !self.http_mode {
                    return Err(CallToolError::unknown_tool("index_repo"));
                }
                let url = Self::require_arg(&params, "index_repo", "url")?;
                match self.do_index_repo(url) {
                    Ok(msg) => Self::ok_text(msg),
                    Err(e) => Err(Self::err(format!("Repository indexing error: {e}"))),
                }
            }

            other => Err(CallToolError::unknown_tool(other)),
        }
    }
}

// --- Server setup ---

fn make_server_details() -> InitializeResult {
    InitializeResult {
        server_info: Implementation {
            name: "Beret".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            title: Some("Beret".to_string()),
            description: Some(
                "MCP server providing SPARQL-based codebase queries over an RDF knowledge graph"
                    .to_string(),
            ),
            icons: vec![],
            website_url: None,
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            ..Default::default()
        },
        protocol_version: ProtocolVersion::V2025_11_25.into(),
        instructions: Some(
            "Beret builds an RDF knowledge graph of a codebase. Call refresh_index with a \
             directory path to index it first, then use the other tools to explore. \
             Start with file_stats for an overview, find_entry_points to locate where \
             the app starts, find_symbol to locate definitions, find_callers/find_callees \
             to trace the call graph, find_dead_code for unused functions, and \
             search_pattern for structural AST matching. Use query_codebase for raw SPARQL."
                .to_string(),
        ),
        meta: None,
    }
}

async fn run_stdio(root: PathBuf) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(CodebaseStore::new()?);

    eprintln!("Beret: ready (use refresh_index to scan a directory)");

    let handler = BeretHandler {
        store,
        root: std::sync::RwLock::new(root),
        http_mode: false,
    };

    let transport = StdioTransport::new(TransportOptions::default())?;
    let server = server_runtime::create_server(McpServerOptions {
        transport,
        handler: handler.to_mcp_server_handler(),
        server_details: make_server_details(),
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    server.start().await?;
    Ok(())
}

async fn run_http(
    host: String,
    port: u16,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    use rust_mcp_sdk::mcp_server::{hyper_server, hyper_runtime::HyperRuntime, HyperServerOptions};

    let store = Arc::new(CodebaseStore::new()?);

    let handler = BeretHandler {
        store,
        root: std::sync::RwLock::new(std::env::temp_dir()),
        http_mode: true,
    };

    let options = HyperServerOptions {
        host: host.clone(),
        port,
        sse_support: true,
        ..Default::default()
    };

    let server =
        hyper_server::create_server(make_server_details(), handler.to_mcp_server_handler(), options);

    eprintln!(
        "Beret: HTTP/SSE server listening on http://{}:{}",
        host, port
    );
    eprintln!("Beret: use index_repo tool to clone and index a repository");

    let runtime = HyperRuntime::create(server).await?;
    runtime.await_server().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    match parse_args()? {
        Mode::Stdio { root } => run_stdio(root).await,
        Mode::Http { host, port } => run_http(host, port).await,
    }
}
