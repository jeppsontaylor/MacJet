/// MacJet MCP — RMCP server: live collectors, tools, resources, prompts, subscriptions.
use crate::mcp::cache::AsyncTTLCache;
use crate::mcp::disk_index::{
    json_disk_directory, json_disk_duplicates, json_disk_summary, json_suggest_disk_cleanup,
    trash_paths_mcp,
};
use crate::mcp::elicit::KillProcessHumanConfirm;
use crate::mcp::models::{McpErrorDetail, ProcessListResult};
use crate::mcp::resources::{
    json_audit, json_audit_wrapped, json_chrome, json_energy, json_network, json_process_group,
    json_process_pid, json_processes_top, json_reclaim, json_system_overview,
};
use crate::mcp::runtime::McpCollectorState;
use crate::mcp::safety::audit_disk_trash;
use crate::mcp::snapshot::{
    explain_heat, sorted_groups, system_overview_extended, wrap, McpSnapshot,
};
use crate::mcp::snapshot::{group_to_detail, group_to_summary};
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::service::{ElicitationError, NotificationContext, Peer, RequestContext, RoleServer};
use rmcp::ErrorData as McpError;
use rmcp::ServiceExt;
use serde_json::json;
use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

const INSTRUCTIONS: &str = "MacJet exposes live macOS CPU, memory, processes, network, thermal (when root), Chrome CDP tabs, reclaim scoring, and disk index summaries (SQLite under the MacJet cache dir). Use get_system_overview then list_process_groups. Disk data is stale until the TUI Disk tab completes a scan. Responses include a meta object (schema_version, collected_at_unix, capabilities). Process cmdlines are omitted unless include_cmdline=true. Set MACJET_MCP_READONLY=1 to disable kill_process and trash_disk_paths. Audit log: ~/.macjet/mcp_audit.jsonl.";

fn mcp_readonly() -> bool {
    env::var("MACJET_MCP_READONLY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub struct MacJetServer {
    pub snapshot: Arc<RwLock<McpSnapshot>>,
    pub collector: Arc<Mutex<McpCollectorState>>,
    pub cache: Arc<AsyncTTLCache>,
    pub refresh_secs: u64,
    pub readonly: bool,
    pub ml_enabled: bool,
    peer_slot: Arc<RwLock<Option<Peer<RoleServer>>>>,
    subscriptions: Arc<Mutex<HashSet<String>>>,
}

impl MacJetServer {
    pub fn new(
        snapshot: Arc<RwLock<McpSnapshot>>,
        collector: Arc<Mutex<McpCollectorState>>,
        cache: Arc<AsyncTTLCache>,
        refresh_secs: u64,
        ml_enabled: bool,
        peer_slot: Arc<RwLock<Option<Peer<RoleServer>>>>,
        subscriptions: Arc<Mutex<HashSet<String>>>,
    ) -> Self {
        Self {
            snapshot,
            collector,
            cache,
            refresh_secs,
            readonly: mcp_readonly(),
            ml_enabled,
            peer_slot,
            subscriptions,
        }
    }

    fn tool_schemas(readonly: bool) -> Vec<Tool> {
        let mut tools = vec![
            tool(
                "get_system_overview",
                "Live system CPU, memory, swap, hostname, cores, optional thermal/fan/GPU from powermetrics.",
                json!({
                    "type": "object",
                    "properties": {
                        "include_swap": { "type": "boolean", "default": true },
                        "include_thermal": { "type": "boolean", "default": true }
                    }
                }),
            ),
            tool(
                "list_process_groups",
                "Top process groups with CPU/memory; filter and sort.",
                json!({
                    "type": "object",
                    "properties": {
                        "sort": { "type": "string", "enum": ["cpu", "mem", "memory", "name"], "default": "cpu" },
                        "limit": { "type": "integer", "default": 50, "minimum": 1, "maximum": 500 },
                        "offset": { "type": "integer", "default": 0, "minimum": 0 },
                        "filter": { "type": "string", "default": "" },
                        "include_system": { "type": "boolean", "default": false }
                    }
                }),
            ),
            tool(
                "get_process_group",
                "Detail for one app group; name match exact (case-insensitive) or first substring match.",
                json!({
                    "type": "object",
                    "required": ["name"],
                    "properties": {
                        "name": { "type": "string" },
                        "include_cmdline": { "type": "boolean", "default": false }
                    }
                }),
            ),
            tool(
                "get_process_by_pid",
                "Detail for the group containing this PID.",
                json!({
                    "type": "object",
                    "required": ["pid"],
                    "properties": {
                        "pid": { "type": "integer", "minimum": 1 },
                        "include_cmdline": { "type": "boolean", "default": false }
                    }
                }),
            ),
            tool(
                "get_reclaim_candidates",
                "Heuristic reclaim list (same engine as TUI Reclaim view).",
                json!({
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "default": 25, "minimum": 1, "maximum": 200 },
                        "min_score": { "type": "integer", "default": 5, "minimum": 0, "maximum": 100 }
                    }
                }),
            ),
            tool(
                "get_network_report",
                "System throughput plus top per-process cumulative bytes (from last process sample).",
                json!({
                    "type": "object",
                    "properties": {
                        "top_n": { "type": "integer", "default": 30, "minimum": 1, "maximum": 200 }
                    }
                }),
            ),
            tool(
                "get_energy_report",
                "Powermetrics-derived wakeups when running as root; otherwise available=false with reason.",
                json!({
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "default": 32, "minimum": 1, "maximum": 128 }
                    }
                }),
            ),
            tool(
                "list_chrome_tabs",
                "Chrome/Chromium pages from CDP localhost:9222 when enabled.",
                json!({ "type": "object", "properties": {} }),
            ),
            tool(
                "get_audit_log",
                "Last N lines of MCP audit JSONL (~/.macjet/mcp_audit.jsonl).",
                json!({
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "default": 50, "minimum": 1, "maximum": 500 }
                    }
                }),
            ),
            tool(
                "explain_system_heat",
                "Narrative summary of CPU load and top groups.",
                json!({
                    "type": "object",
                    "properties": {
                        "focus_pid": { "type": "integer", "minimum": 1 }
                    }
                }),
            ),
            tool(
                "get_prediction_stats",
                "Online CPU predictor stats when ML enabled; otherwise explains disabled state.",
                json!({ "type": "object", "properties": {} }),
            ),
            tool(
                "get_disk_summary",
                "Disk index summary from SQLite (root, counts, reclaimable bytes, dup groups). Optional db_path overrides default cache location.",
                json!({
                    "type": "object",
                    "properties": {
                        "db_path": { "type": "string", "description": "Optional path to disk_index.sqlite" }
                    }
                }),
            ),
            tool(
                "list_disk_duplicates",
                "Top duplicate file rows from the disk index (paginated by limit).",
                json!({
                    "type": "object",
                    "properties": {
                        "db_path": { "type": "string" },
                        "limit": { "type": "integer", "default": 50, "minimum": 1, "maximum": 500 },
                        "min_size": { "type": "integer", "default": 0, "minimum": 0 },
                        "only_reclaimable": { "type": "boolean", "default": false }
                    }
                }),
            ),
            tool(
                "suggest_disk_cleanup",
                "Structured safe/review/danger hints from LIKELY_DELETE duplicate flags in the index.",
                json!({
                    "type": "object",
                    "properties": {
                        "db_path": { "type": "string" }
                    }
                }),
            ),
        ];
        if !readonly {
            tools.push(tool(
                "kill_process",
                "SIGTERM after optional human elicitation (when client supports it). PID must be >= 500.",
                json!({
                    "type": "object",
                    "required": ["pid", "reason"],
                    "properties": {
                        "pid": { "type": "integer", "minimum": 1 },
                        "reason": { "type": "string" }
                    }
                }),
            ));
            tools.push(tool(
                "trash_disk_paths",
                "Move files to Trash and remove from disk index. Paths must be under the indexed root when root metadata is set. Logged to audit JSONL.",
                json!({
                    "type": "object",
                    "required": ["paths"],
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Absolute file paths"
                        },
                        "db_path": { "type": "string" }
                    }
                }),
            ));
        }
        tools
    }
}

fn tool(name: &'static str, description: &'static str, schema: serde_json::Value) -> Tool {
    Tool::new(
        name,
        description,
        Arc::new(schema.as_object().unwrap().clone()),
    )
}

fn parse_uri_query(uri: &str) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    if let Some(qs) = uri.split('?').nth(1) {
        for pair in qs.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                m.insert(k.to_string(), v.to_string());
            }
        }
    }
    m
}

async fn read_resource_body(server: &MacJetServer, uri: &str) -> Result<String, McpError> {
    let snap = server.snapshot.read().await.clone();

    match uri {
        "macjet://system/overview" | "system://overview" => Ok(server
            .cache
            .get(uri, || async { json_system_overview(&snap, true, true) })
            .await),
        "macjet://processes/top" | "process://top" => Ok(server
            .cache
            .get(uri, || async { json_processes_top(&snap, 50, false) })
            .await),
        "macjet://network/current" => Ok(server
            .cache
            .get(uri, || async { json_network(&snap, 30) })
            .await),
        "macjet://reclaim/candidates" => {
            let raw = {
                let c = server.collector.lock().await;
                c.metrics_history
                    .get_reclaim_candidates(c.process_collector.groups())
            };
            Ok(json_reclaim(&snap, raw, 5, 25))
        }
        "macjet://chrome/tabs" => Ok(json_chrome(&snap)),
        "macjet://energy/latest" => Ok(json_energy(&snap, 32)),
        "macjet://audit/recent" => Ok(json_audit_wrapped(&snap, 50)),
        "macjet://disk/summary" => Ok(json_disk_summary(&snap, None)),
        _ if uri.starts_with("macjet://disk/duplicates") => {
            let q = parse_uri_query(uri);
            let limit = q
                .get("limit")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(50);
            let min_size = q
                .get("min_size")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let only = q
                .get("only_reclaimable")
                .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            Ok(json_disk_duplicates(&snap, None, limit, min_size, only))
        }
        _ if uri.starts_with("macjet://disk/directory") => {
            let q = parse_uri_query(uri);
            let path = q
                .get("path")
                .cloned()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| McpError::invalid_params("missing path query parameter", None))?;
            Ok(json_disk_directory(&snap, None, &path))
        }
        _ if uri.starts_with("macjet://process/pid/") => {
            let rest = uri.trim_start_matches("macjet://process/pid/");
            let pid: u32 = rest
                .parse()
                .map_err(|_| McpError::invalid_params("invalid pid in URI", None))?;
            json_process_pid(&snap, pid, false)
                .ok_or_else(|| McpError::invalid_params("PID not found in snapshot", None))
        }
        _ if uri.starts_with("macjet://process/group") => {
            let q = parse_uri_query(uri);
            let name = q
                .get("name")
                .cloned()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| McpError::invalid_params("missing name query parameter", None))?;
            json_process_group(&snap, &name, false)
                .ok_or_else(|| McpError::invalid_params("group not found", None))
        }
        _ => Err(McpError::invalid_params("Unknown resource URI", None)),
    }
}

impl ServerHandler for MacJetServer {
    fn get_info(&self) -> ServerInfo {
        let caps = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .enable_prompts()
            .enable_logging()
            .enable_resources_subscribe()
            .enable_resources_list_changed()
            .enable_completions()
            .build();
        ServerInfo::new(caps)
            .with_server_info(Implementation::new("macjet", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS.to_string())
    }

    fn on_initialized(
        &self,
        context: NotificationContext<RoleServer>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let slot = self.peer_slot.clone();
        let peer = context.peer.clone();
        async move {
            *slot.write().await = Some(peer);
        }
    }

    fn subscribe(
        &self,
        request: SubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send + '_ {
        let subs = self.subscriptions.clone();
        let uri = request.uri;
        async move {
            subs.lock().await.insert(uri);
            Ok(())
        }
    }

    fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send + '_ {
        let subs = self.subscriptions.clone();
        let uri = request.uri;
        async move {
            subs.lock().await.remove(&uri);
            Ok(())
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = Self::tool_schemas(self.readonly);
        std::future::ready(Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        let this = self.clone_shallow();
        async move { dispatch_tool(this, request, context).await }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        let resources = vec![
            res("macjet://system/overview", "System overview"),
            res("macjet://processes/top", "Top process groups"),
            res("macjet://network/current", "Network snapshot"),
            res("macjet://reclaim/candidates", "Reclaim candidates"),
            res("macjet://chrome/tabs", "Chrome CDP tabs"),
            res("macjet://energy/latest", "Energy / powermetrics top"),
            res("macjet://audit/recent", "MCP audit log excerpt"),
            res("macjet://disk/summary", "Disk index summary (SQLite)"),
        ];
        std::future::ready(Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        }))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourceTemplatesResult, McpError>> + Send + '_
    {
        let t1 = Annotated {
            raw: RawResourceTemplate::new(
                "macjet://process/group?name={name}",
                "Process group by name",
            )
            .with_description("Query parameter name= app name (substring match)")
            .with_mime_type("application/json"),
            annotations: None,
        };
        let t2 = Annotated {
            raw: RawResourceTemplate::new("macjet://process/pid/{pid}", "Process group by PID")
                .with_description("Path segment pid = process id")
                .with_mime_type("application/json"),
            annotations: None,
        };
        let t3 = Annotated {
            raw: RawResourceTemplate::new(
                "macjet://disk/directory?path={path}",
                "Disk index children under path",
            )
            .with_description("path = parent directory as stored in the index (often absolute)")
            .with_mime_type("application/json"),
            annotations: None,
        };
        let t4 = Annotated {
            raw: RawResourceTemplate::new(
                "macjet://disk/duplicates?limit={limit}",
                "Top duplicate rows from disk index",
            )
            .with_mime_type("application/json"),
            annotations: None,
        };
        std::future::ready(Ok(ListResourceTemplatesResult {
            resource_templates: vec![t1, t2, t3, t4],
            next_cursor: None,
            meta: None,
        }))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        let server = self.clone_shallow();
        let uri = request.uri.clone();
        async move {
            let json_str = read_resource_body(&server, uri.as_str()).await?;
            Ok(ReadResourceResult::new(vec![ResourceContents::text(
                json_str, uri,
            )
            .with_mime_type("application/json")]))
        }
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListPromptsResult, McpError>> + Send + '_ {
        let prompts = vec![
            Prompt::new(
                "diagnose_cpu_spike",
                Some("Workflow: overview then top CPU groups"),
                None,
            )
            .with_title("Diagnose CPU spike"),
            Prompt::new(
                "memory_pressure_checklist",
                Some("Steps for memory pressure on macOS"),
                None,
            )
            .with_title("Memory pressure checklist"),
            Prompt::new(
                "safe_kill_workflow",
                Some("Human approval, PID rules, audit trail"),
                None,
            )
            .with_title("Safe process termination"),
        ];
        std::future::ready(Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
            meta: None,
        }))
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetPromptResult, McpError>> + Send + '_ {
        let name = request.name;
        let body = match name.as_str() {
            "diagnose_cpu_spike" => "You are diagnosing elevated CPU on macOS via MacJet MCP.\n\
                 1) Call get_system_overview with include_thermal true.\n\
                 2) Call list_process_groups with sort=cpu and limit=20.\n\
                 3) If a browser dominates, call list_chrome_tabs.\n\
                 4) Summarize likely causes and next steps for the user."
                .to_string(),
            "memory_pressure_checklist" => "Use MacJet to assess memory pressure:\n\
                 1) get_system_overview — note mem_percent and swap_used_gb.\n\
                 2) list_process_groups sort=mem.\n\
                 3) get_reclaim_candidates for heuristic targets.\n\
                 4) Advise closing heavy apps or tabs before suggesting kill_process."
                .to_string(),
            "safe_kill_workflow" => "Before kill_process:\n\
                 - PIDs below 500 are refused.\n\
                 - The MCP server cannot kill itself.\n\
                 - Clients with elicitation show a human confirmation form.\n\
                 - Every attempt is logged to ~/.macjet/mcp_audit.jsonl.\n\
                 - Prefer SIGTERM; escalate only with explicit user request."
                .to_string(),
            _ => {
                return std::future::ready(Err(McpError::invalid_params("Unknown prompt", None)));
            }
        };
        std::future::ready(Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description(name)))
    }

    fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CompleteResult, McpError>> + Send + '_ {
        let values = match &request.r#ref {
            Reference::Prompt(p)
                if p.name == "list_process_groups" && request.argument.name == "sort" =>
            {
                vec!["cpu".into(), "mem".into(), "name".into()]
            }
            Reference::Resource(r) if r.uri.starts_with("macjet://") => {
                vec![
                    "macjet://system/overview".into(),
                    "macjet://processes/top".into(),
                    "macjet://network/current".into(),
                ]
            }
            _ => vec![],
        };
        let completion = CompletionInfo::new(values).unwrap_or_default();
        std::future::ready(Ok(CompleteResult::new(completion)))
    }

    fn set_level(
        &self,
        request: SetLevelRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send + '_ {
        tracing::info!(target: "macjet_mcp", "logging/setLevel: {:?}", request.level);
        std::future::ready(Ok(()))
    }
}

impl MacJetServer {
    fn clone_shallow(&self) -> MacJetServer {
        MacJetServer {
            snapshot: self.snapshot.clone(),
            collector: self.collector.clone(),
            cache: self.cache.clone(),
            refresh_secs: self.refresh_secs,
            readonly: self.readonly,
            ml_enabled: self.ml_enabled,
            peer_slot: self.peer_slot.clone(),
            subscriptions: self.subscriptions.clone(),
        }
    }
}

fn res(uri: &str, title: &str) -> Annotated<RawResource> {
    Annotated {
        raw: RawResource::new(uri, title).with_mime_type("application/json"),
        annotations: None,
    }
}

async fn dispatch_tool(
    server: MacJetServer,
    request: CallToolRequestParams,
    context: RequestContext<RoleServer>,
) -> Result<CallToolResult, McpError> {
    let args = request.arguments.as_ref();
    let name = request.name.as_ref();
    let snap = server.snapshot.read().await.clone();

    macro_rules! json_ok {
        ($s:expr) => {
            return Ok(CallToolResult::success(vec![Content::text($s)]))
        };
    }

    match name {
        "get_system_overview" => {
            let inc_swap = arg_bool(args, "include_swap", true);
            let inc_th = arg_bool(args, "include_thermal", true);
            let data = system_overview_extended(&snap, inc_swap, inc_th);
            json_ok!(serde_json::to_string(&wrap(&snap, data)).unwrap_or_default());
        }
        "list_process_groups" => {
            let sort = arg_str(args, "sort", "cpu");
            let limit = arg_u64(args, "limit", 50).min(500).max(1) as usize;
            let offset = arg_u64(args, "offset", 0) as usize;
            let filter = arg_str(args, "filter", "");
            let inc_sys = arg_bool(args, "include_system", false);
            let sorted = sorted_groups(&snap.groups, &sort, &filter, inc_sys);
            let total = sorted.len();
            let page: Vec<_> = sorted
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|g| group_to_summary(&g))
                .collect();
            let data = ProcessListResult {
                groups: page,
                total_groups: total,
                sort_by: sort,
                filter_applied: filter,
            };
            json_ok!(serde_json::to_string(&wrap(&snap, data)).unwrap_or_default());
        }
        "get_process_group" => {
            let n = arg_str(args, "name", "");
            if n.is_empty() {
                json_ok!(err_json("missing_name", "name is required"));
            }
            let include_cmdline = arg_bool(args, "include_cmdline", false);
            match crate::mcp::snapshot::find_group_by_name(&snap.groups, &n) {
                Some(g) => {
                    let d = group_to_detail(g, include_cmdline);
                    json_ok!(serde_json::to_string(&wrap(&snap, d)).unwrap_or_default());
                }
                None => json_ok!(err_json("not_found", "no matching process group")),
            }
        }
        "get_process_by_pid" => {
            let pid = arg_u64(args, "pid", 0) as u32;
            if pid == 0 {
                json_ok!(err_json("bad_pid", "pid required"));
            }
            let inc_cmd = arg_bool(args, "include_cmdline", false);
            match crate::mcp::snapshot::find_group_by_pid(&snap.groups, pid) {
                Some(g) => {
                    let d = group_to_detail(g, inc_cmd);
                    json_ok!(serde_json::to_string(&wrap(&snap, d)).unwrap_or_default());
                }
                None => json_ok!(err_json("not_found", "PID not in snapshot")),
            }
        }
        "get_reclaim_candidates" => {
            let limit = arg_u64(args, "limit", 25).min(200).max(1) as usize;
            let min_score = arg_u64(args, "min_score", 5).min(100) as u8;
            let raw = {
                let c = server.collector.lock().await;
                c.metrics_history
                    .get_reclaim_candidates(c.process_collector.groups())
            };
            json_ok!(json_reclaim(&snap, raw, min_score, limit));
        }
        "get_network_report" => {
            let top_n = arg_u64(args, "top_n", 30).min(200).max(1) as usize;
            json_ok!(json_network(&snap, top_n));
        }
        "get_energy_report" => {
            let lim = arg_u64(args, "limit", 32).min(128).max(1) as usize;
            json_ok!(json_energy(&snap, lim));
        }
        "list_chrome_tabs" => {
            json_ok!(json_chrome(&snap));
        }
        "get_audit_log" => {
            let lim = arg_u64(args, "limit", 50).min(500).max(1) as usize;
            let text = json_audit(lim);
            json_ok!(serde_json::to_string(&serde_json::json!({
                "meta": snap.meta,
                "data": { "log_text": text }
            }))
            .unwrap_or_default());
        }
        "explain_system_heat" => {
            let focus = args
                .and_then(|a| a.get("focus_pid"))
                .and_then(|v| v.as_u64())
                .map(|u| u as u32);
            let heat = {
                let c = server.collector.lock().await;
                explain_heat(&snap, focus, &c.metrics_history)
            };
            json_ok!(serde_json::to_string(&wrap(&snap, heat)).unwrap_or_default());
        }
        "get_prediction_stats" => {
            if !server.ml_enabled {
                json_ok!(serde_json::to_string(&wrap(
                    &snap,
                    serde_json::json!({ "enabled": false, "hint": "Start macjet without --no-ml / enable ML for MCP." })
                ))
                .unwrap_or_default());
            }
            let stats = {
                let c = server.collector.lock().await;
                c.cpu_predictor.stats()
            };
            json_ok!(serde_json::to_string(&wrap(&snap, stats)).unwrap_or_default());
        }
        "get_disk_summary" => {
            let db = arg_opt_path(args, "db_path");
            json_ok!(json_disk_summary(&snap, db));
        }
        "list_disk_duplicates" => {
            let db = arg_opt_path(args, "db_path");
            let limit = arg_u64(args, "limit", 50);
            let min_size = arg_u64(args, "min_size", 0);
            let only = arg_bool(args, "only_reclaimable", false);
            json_ok!(json_disk_duplicates(&snap, db, limit, min_size, only));
        }
        "suggest_disk_cleanup" => {
            let db = arg_opt_path(args, "db_path");
            json_ok!(json_suggest_disk_cleanup(&snap, db));
        }
        "kill_process" => {
            if server.readonly {
                return Ok(CallToolResult::error(vec![Content::text(
                    serde_json::to_string(&McpErrorDetail {
                        code: "readonly".into(),
                        message: "MACJET_MCP_READONLY is set".into(),
                        pid: None,
                    })
                    .unwrap_or_default(),
                )]));
            }
            let pid = args
                .and_then(|a| a.get("pid"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let reason = args
                .and_then(|a| a.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("MCP kill");
            let peer = context.peer.clone();
            let msg = format!(
                "PID {} — terminate with SIGTERM? Reason from agent: {}. Confirm only if the human approved.",
                pid, reason
            );
            let proceed = match peer
                .elicit_with_timeout::<KillProcessHumanConfirm>(
                    msg,
                    Some(std::time::Duration::from_secs(120)),
                )
                .await
            {
                Ok(Some(c)) => c.confirm_terminate,
                Ok(None) => {
                    return Ok(CallToolResult::error(vec![Content::text(
                        serde_json::to_string(&McpErrorDetail {
                            code: "elicit_no_content".into(),
                            message: "No confirmation received".into(),
                            pid: Some(pid),
                        })
                        .unwrap_or_default(),
                    )]));
                }
                Err(ElicitationError::CapabilityNotSupported) => true,
                Err(ElicitationError::UserDeclined) | Err(ElicitationError::UserCancelled) => {
                    return Ok(CallToolResult::error(vec![Content::text(
                        serde_json::to_string(&McpErrorDetail {
                            code: "user_cancelled".into(),
                            message: "User declined or cancelled confirmation".into(),
                            pid: Some(pid),
                        })
                        .unwrap_or_default(),
                    )]));
                }
                Err(e) => {
                    return Ok(CallToolResult::error(vec![Content::text(
                        serde_json::to_string(&McpErrorDetail {
                            code: "elicit_error".into(),
                            message: e.to_string(),
                            pid: Some(pid),
                        })
                        .unwrap_or_default(),
                    )]));
                }
            };
            if !proceed {
                return Ok(CallToolResult::error(vec![Content::text(
                    serde_json::to_string(&McpErrorDetail {
                        code: "not_confirmed".into(),
                        message: "confirm_terminate was false".into(),
                        pid: Some(pid),
                    })
                    .unwrap_or_default(),
                )]));
            }
            match crate::mcp::safety::send_signal(pid, 15, reason, "rmcp", "req") {
                Ok(audit) => {
                    server.cache.invalidate(None).await;
                    let text = format!("{{\"success\": true, \"audit_id\": \"{}\"}}", audit);
                    Ok(CallToolResult::success(vec![Content::text(text)]))
                }
                Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
            }
        }
        "trash_disk_paths" => {
            if server.readonly {
                return Ok(CallToolResult::error(vec![Content::text(
                    serde_json::to_string(&McpErrorDetail {
                        code: "readonly".into(),
                        message: "MACJET_MCP_READONLY is set".into(),
                        pid: None,
                    })
                    .unwrap_or_default(),
                )]));
            }
            let Some(arr) = args.and_then(|a| a.get("paths")).and_then(|v| v.as_array()) else {
                json_ok!(err_json("bad_args", "paths array required"));
            };
            let paths: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            let db = arg_opt_path(args, "db_path");
            match trash_paths_mcp(&paths, db) {
                Ok(n) => {
                    audit_disk_trash(&paths, true, "");
                    json_ok!(serde_json::to_string(&wrap(
                        &snap,
                        serde_json::json!({ "trashed": n, "paths": paths })
                    ))
                    .unwrap_or_default());
                }
                Err(e) => {
                    audit_disk_trash(&paths, false, &e);
                    json_ok!(err_json("trash_failed", &e));
                }
            }
        }
        _ => Err(McpError::method_not_found::<CallToolRequestMethod>()),
    }
}

fn err_json(code: &str, msg: &str) -> String {
    serde_json::json!({ "error": { "code": code, "message": msg } }).to_string()
}

fn arg_bool(
    args: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
    def: bool,
) -> bool {
    args.and_then(|a| a.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(def)
}

fn arg_str(
    args: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
    def: &str,
) -> String {
    args.and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| def.to_string())
}

fn arg_u64(args: Option<&serde_json::Map<String, serde_json::Value>>, key: &str, def: u64) -> u64 {
    args.and_then(|a| a.get(key))
        .and_then(|v| v.as_u64())
        .unwrap_or(def)
}

fn arg_opt_path(
    args: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<std::path::PathBuf> {
    args.and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
}

pub async fn run_mcp_server(refresh_secs: u64, ml_enabled: bool) {
    let refresh_secs = refresh_secs.max(1);
    let snapshot = Arc::new(RwLock::new(McpSnapshot::default()));
    let collector = Arc::new(Mutex::new(McpCollectorState::new(ml_enabled)));
    {
        let mut c = collector.lock().await;
        c.step();
        *snapshot.write().await = c.build_snapshot(refresh_secs);
    }
    let peer_slot: Arc<RwLock<Option<Peer<RoleServer>>>> = Arc::new(RwLock::new(None));
    let subscriptions: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let _collector_task = crate::mcp::runtime::spawn_collector_loop(
        collector.clone(),
        snapshot.clone(),
        refresh_secs,
        peer_slot.clone(),
        subscriptions.clone(),
    );
    let cache = Arc::new(AsyncTTLCache::new(refresh_secs as f64));
    let server = MacJetServer::new(
        snapshot,
        collector,
        cache,
        refresh_secs,
        ml_enabled,
        peer_slot,
        subscriptions,
    );
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let transport = rmcp::transport::async_rw::AsyncRwTransport::new(stdin, stdout);
    if let Err(e) = server
        .serve_with_ct(transport, tokio_util::sync::CancellationToken::new())
        .await
    {
        eprintln!("MCP Event Loop Exception: {}", e);
    }
}
