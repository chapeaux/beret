mod ingestor;
mod store;

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

fn query_codebase_tool() -> Tool {
    Tool {
        name: "query_codebase".to_string(),
        title: Some("Query Codebase".to_string()),
        description: Some(
            "Execute a SPARQL SELECT or ASK query against the codebase knowledge graph. \
             The graph uses the repo: prefix (http://repo.example.org/). \
             Triples include: <file/func> a repo:Function, <file/class> a repo:Class, \
             <caller> repo:calls <callee>."
                .to_string(),
        ),
        input_schema: make_tool_input_schema(
            &[("sparql", "string", "A SPARQL query string to execute against the codebase graph")],
            &["sparql"],
        ),
        annotations: None,
        execution: None,
        icons: vec![],
        meta: None,
        output_schema: None,
    }
}

fn refresh_index_tool() -> Tool {
    Tool {
        name: "refresh_index".to_string(),
        title: Some("Refresh Index".to_string()),
        description: Some(
            "Re-scan the codebase and rebuild the knowledge graph. \
             Clears existing triples and re-ingests all supported files."
                .to_string(),
        ),
        input_schema: make_tool_input_schema(&[], &[]),
        annotations: None,
        execution: None,
        icons: vec![],
        meta: None,
        output_schema: None,
    }
}

fn index_repo_tool() -> Tool {
    Tool {
        name: "index_repo".to_string(),
        title: Some("Index Repository".to_string()),
        description: Some(
            "Clone a git repository and index it into the knowledge graph. \
             Clears any existing index first. Accepts a git URL (HTTPS or SSH)."
                .to_string(),
        ),
        input_schema: make_tool_input_schema(
            &[("url", "string", "Git repository URL to clone and index")],
            &["url"],
        ),
        annotations: None,
        execution: None,
        icons: vec![],
        meta: None,
        output_schema: None,
    }
}

// --- Handler ---

struct BeretHandler {
    store: Arc<CodebaseStore>,
    root: std::sync::RwLock<PathBuf>,
    http_mode: bool,
}

impl BeretHandler {
    fn do_refresh(&self) -> std::result::Result<String, String> {
        let root = self.root.read().unwrap().clone();
        self.store.clear().map_err(|e| e.to_string())?;
        let count = ingest(&root, &self.store).map_err(|e| e.to_string())?;
        Ok(format!("Indexed {} triples from {}", count, root.display()))
    }

    fn do_index_repo(&self, url: &str) -> std::result::Result<String, String> {
        let temp_dir = std::env::temp_dir().join(format!("beret-{}", hash_url(url)));

        if temp_dir.exists() {
            // Pull latest changes
            std::process::Command::new("git")
                .args(["pull", "--ff-only"])
                .current_dir(&temp_dir)
                .output()
                .map_err(|e| format!("git pull failed: {e}"))?;
        } else {
            // Clone fresh
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

    fn tools(&self) -> Vec<Tool> {
        let mut tools = vec![query_codebase_tool(), refresh_index_tool()];
        if self.http_mode {
            tools.push(index_repo_tool());
        }
        tools
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
            tools: self.tools(),
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
                let sparql = params
                    .arguments
                    .as_ref()
                    .and_then(|args| args.get("sparql"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::invalid_arguments(
                            "query_codebase",
                            Some("missing 'sparql' argument".into()),
                        )
                    })?;

                match self.store.query_to_json(sparql) {
                    Ok(result) => {
                        let text = serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| result.to_string());
                        Ok(CallToolResult::text_content(vec![
                            TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => Err(CallToolError::from_message(format!(
                        "SPARQL query error: {e}"
                    ))),
                }
            }
            "refresh_index" => match self.do_refresh() {
                Ok(msg) => Ok(CallToolResult::text_content(vec![
                    TextContent::new(msg, None, None),
                ])),
                Err(e) => Err(CallToolError::from_message(format!(
                    "Index refresh error: {e}"
                ))),
            },
            "index_repo" => {
                if !self.http_mode {
                    return Err(CallToolError::unknown_tool("index_repo"));
                }
                let url = params
                    .arguments
                    .as_ref()
                    .and_then(|args| args.get("url"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::invalid_arguments(
                            "index_repo",
                            Some("missing 'url' argument".into()),
                        )
                    })?;

                match self.do_index_repo(url) {
                    Ok(msg) => Ok(CallToolResult::text_content(vec![
                        TextContent::new(msg, None, None),
                    ])),
                    Err(e) => Err(CallToolError::from_message(format!(
                        "Repository indexing error: {e}"
                    ))),
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
            "Use query_codebase to run SPARQL queries against the indexed codebase graph. \
             Use refresh_index to re-scan files after changes."
                .to_string(),
        ),
        meta: None,
    }
}

async fn run_stdio(root: PathBuf) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(CodebaseStore::new()?);

    eprintln!("Beret: indexing {}...", root.display());
    let count = ingest(&root, &store).map_err(|e| e.to_string())?;
    eprintln!("Beret: indexed {} triples", count);

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
