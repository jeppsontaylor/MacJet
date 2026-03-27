/// MacJet MCP — RMCP Server Integration
/// Exposes system telemetry and process kill guards over JSON-RPC.
use crate::mcp::resources::*;
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::ErrorData as McpError;
use rmcp::ServiceExt;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct MacJetServer;

impl ServerHandler for MacJetServer {
    fn initialize(
        &self,
        _request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<InitializeResult, McpError>> + Send + '_ {
        std::future::ready(Ok(InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("macjet-rs", "0.1.0"))))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let input_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "pid": { "type": "integer" },
                "reason": { "type": "string" }
            },
            "required": ["pid", "reason"]
        });

        let schema_obj = input_schema.as_object().unwrap().clone();

        let tools = vec![Tool::new(
            "kill_process",
            "Terminates a process safely. Enforces PID rules.",
            Arc::new(schema_obj),
        )];

        std::future::ready(Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            match request.name.as_ref() {
                "kill_process" => {
                    let pid = request
                        .arguments
                        .as_ref()
                        .and_then(|args| args.get("pid"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;

                    let reason = request
                        .arguments
                        .as_ref()
                        .and_then(|args| args.get("reason"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("MCP Kill");

                    match crate::mcp::safety::send_signal(pid, 15, reason, "rmcp", "req") {
                        Ok(audit) => {
                            let text =
                                format!("{{\"success\": true, \"audit_id\": \"{}\"}}", audit);
                            Ok(CallToolResult::success(vec![Content::text(text)]))
                        }
                        Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
                    }
                }
                _ => Err(McpError::method_not_found::<CallToolRequestMethod>()),
            }
        }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        let sys = RawResource::new("system://overview", "System Overview")
            .with_description("High-level system topology")
            .with_mime_type("application/json");

        let top = RawResource::new("process://top", "Top Processes")
            .with_description("Process list summary")
            .with_mime_type("application/json");

        std::future::ready(Ok(ListResourcesResult {
            resources: vec![
                Annotated {
                    raw: sys,
                    annotations: None,
                },
                Annotated {
                    raw: top,
                    annotations: None,
                },
            ],
            next_cursor: None,
            meta: None,
        }))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        async move {
            let json_str = match request.uri.as_str() {
                "system://overview" => resource_system_overview().await,
                "process://top" => resource_processes_top().await,
                _ => return Err(McpError::invalid_params("Unknown resource URI", None)),
            };

            Ok(ReadResourceResult::new(vec![ResourceContents::text(
                json_str,
                request.uri,
            )
            .with_mime_type("application/json")]))
        }
    }
}

pub async fn run_mcp_server() {
    // std::io transport
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let transport = rmcp::transport::async_rw::AsyncRwTransport::new(stdin, stdout);
    let server = MacJetServer::default();
    if let Err(e) = server
        .serve_with_ct(transport, tokio_util::sync::CancellationToken::new())
        .await
    {
        eprintln!("MCP Event Loop Exception: {}", e);
    }

    // Fallback if transport features aren't enabled
}
