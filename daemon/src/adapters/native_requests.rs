//! Installed-version codecs only. Task2 must connect these to an ordered,
//! generation-bound pending store before any newly decoded family is usable.
//! Native inputs/answers are private: no Serialize or payload-bearing Debug.

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Codex0158,
    Claude21288,
}

#[derive(Clone, PartialEq, Eq)]
pub enum NativeId {
    Integer(i64),
    String(String),
}
impl std::fmt::Debug for NativeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Integer(_) => "Integer [private]",
            Self::String(_) => "String [private]",
        })
    }
}
impl NativeId {
    fn parse(v: &Value) -> Result<Self, CodecError> {
        match v {
            Value::String(s) => Ok(Self::String(s.clone())),
            Value::Number(n) => n.as_i64().map(Self::Integer).ok_or(CodecError::InvalidId),
            _ => Err(CodecError::InvalidId),
        }
    }
    pub fn value(&self) -> Value {
        match self {
            Self::Integer(n) => json!(n),
            Self::String(s) => json!(s),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Context {
    pub thread: Option<String>,
    pub turn: Option<String>,
    pub item: Option<String>,
    pub approval: Option<String>,
    pub environment: Option<String>,
    pub tool_use: Option<String>,
}
impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Context [private native identity]")
    }
}
impl Context {
    fn read(p: &Value) -> Self {
        let text = |k: &str| p[k].as_str().map(str::to_owned);
        Self {
            thread: text("threadId").or_else(|| text("conversationId")),
            turn: text("turnId"),
            item: text("itemId").or_else(|| text("callId")),
            approval: text("approvalId"),
            environment: text("environmentId"),
            tool_use: text("tool_use_id"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    LegacyCommand,
    LegacyFile,
    Command,
    File,
    Permissions,
    Questions,
    Tool,
    Form,
    ExternalForm,
    Url,
    Verification,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineFamily {
    DynamicTool,
    AuthRefresh,
    Attestation,
    Time,
    Hook,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    InvalidId,
    Malformed,
    Unsupported,
    Machine,
    WrongAnswer,
    Unoffered,
    Enlarged,
    NativeVeto,
    Unqualified,
    StaleContext,
}

#[derive(Clone)]
pub struct NativeRequest {
    protocol: Protocol,
    id: NativeId,
    method: String,
    family: Family,
    context: Context,
    envelope: Value,
    params: Value,
    default_to_no: bool,
    suppress_always: bool,
}
impl std::fmt::Debug for NativeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeRequest")
            .field("protocol", &self.protocol)
            .field("family", &self.family)
            .field("payload", &"[private]")
            .finish()
    }
}
impl NativeRequest {
    pub fn id(&self) -> &NativeId {
        &self.id
    }
    pub fn family(&self) -> Family {
        self.family
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
    pub fn envelope(&self) -> &Value {
        &self.envelope
    }
    pub fn default_to_no(&self) -> bool {
        self.default_to_no
    }
    pub fn suppress_always(&self) -> bool {
        self.suppress_always
    }
}

// Machine/unknown frames remain observable without constructing a human answer.
// Their raw payload is retained privately and never included in Debug.
pub enum NativeMessage {
    Owner(NativeRequest),
    Resolved {
        id: NativeId,
        context: Context,
    },
    Machine {
        family: MachineFamily,
        request: PrivateEnvelope,
    },
    Unsupported(PrivateEnvelope),
}
#[derive(Clone)]
pub struct PrivateEnvelope(Value);
impl PrivateEnvelope {
    pub fn value(&self) -> &Value {
        &self.0
    }
}
impl std::fmt::Debug for PrivateEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[private native envelope]")
    }
}
impl std::fmt::Debug for NativeMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Owner(r) => r.fmt(f),
            Self::Resolved { .. } => f.write_str("Resolved [private identity]"),
            Self::Machine { family, .. } => f.debug_tuple("Machine").field(family).finish(),
            Self::Unsupported(_) => f.write_str("Unsupported [private envelope]"),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Answer {
    Decision {
        decision: Value,
    },
    Permissions {
        permissions: Value,
        scope: Scope,
        #[serde(rename = "strictAutoReview")]
        strict_auto_review: Option<bool>,
    },
    Questions {
        answers: BTreeMap<String, Vec<String>>,
    },
    Tool {
        allow: bool,
        message: Option<String>,
        #[serde(rename = "updatedPermissions")]
        updated_permissions: Option<Value>,
    },
    Elicitation {
        action: Action,
        content: Option<Value>,
    },
}
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Turn,
    Session,
}
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Accept,
    Decline,
    Cancel,
}

pub fn decode(protocol: Protocol, envelope: &Value) -> Result<NativeMessage, CodecError> {
    if !envelope.is_object() {
        return Err(CodecError::Malformed);
    }
    let (id, method, params, family) = match protocol {
        Protocol::Codex0158 => {
            let method = envelope["method"].as_str().ok_or(CodecError::Malformed)?;
            let p = &envelope["params"];
            if method == "serverRequest/resolved" {
                validate_native("ServerRequestResolvedNotification", p)?;
                return Ok(NativeMessage::Resolved {
                    id: NativeId::parse(&p["requestId"])?,
                    context: Context::read(p),
                });
            }
            let id = NativeId::parse(&envelope["id"])?;
            let (family, schema) = match method {
                "execCommandApproval" => (Family::LegacyCommand, "ExecCommandApprovalParams"),
                "applyPatchApproval" => (Family::LegacyFile, "ApplyPatchApprovalParams"),
                "item/commandExecution/requestApproval" => {
                    (Family::Command, "CommandExecutionRequestApprovalParams")
                }
                "item/fileChange/requestApproval" => {
                    (Family::File, "FileChangeRequestApprovalParams")
                }
                "item/permissions/requestApproval" => {
                    (Family::Permissions, "PermissionsRequestApprovalParams")
                }
                "item/tool/requestUserInput" => (Family::Questions, "ToolRequestUserInputParams"),
                "mcpServer/elicitation/request" => {
                    let mode = p["mode"].as_str().ok_or(CodecError::Malformed)?;
                    let family = match elicitation_family(mode) {
                        Ok(family) => family,
                        Err(_) => {
                            return Ok(NativeMessage::Unsupported(PrivateEnvelope(
                                envelope.clone(),
                            )))
                        }
                    };
                    (family, "McpServerElicitationRequestParams")
                }
                _ => {
                    let machine = match method {
                        "item/tool/call" => {
                            Some((MachineFamily::DynamicTool, "DynamicToolCallParams"))
                        }
                        "account/chatgptAuthTokens/refresh" => {
                            Some((MachineFamily::AuthRefresh, "ChatgptAuthTokensRefreshParams"))
                        }
                        "attestation/generate" => {
                            Some((MachineFamily::Attestation, "AttestationGenerateParams"))
                        }
                        "currentTime/read" => Some((MachineFamily::Time, "CurrentTimeReadParams")),
                        _ => None,
                    };
                    if let Some((family, schema)) = machine {
                        validate_native(schema, p)?;
                        return Ok(NativeMessage::Machine {
                            family,
                            request: PrivateEnvelope(envelope.clone()),
                        });
                    }
                    return Ok(NativeMessage::Unsupported(PrivateEnvelope(
                        envelope.clone(),
                    )));
                }
            };
            validate_native(schema, p)?;
            (id, method.to_owned(), p.clone(), family)
        }
        Protocol::Claude21288 => {
            let id = match &envelope["request_id"] {
                Value::String(s) => NativeId::String(s.clone()),
                _ => return Err(CodecError::InvalidId),
            };
            if envelope["type"] == "control_cancel_request" {
                return Ok(NativeMessage::Resolved {
                    id,
                    context: Context::read(&Value::Null),
                });
            }
            if envelope["type"] != "control_request" {
                return Err(CodecError::Malformed);
            }
            let p = &envelope["request"];
            let method = p["subtype"].as_str().ok_or(CodecError::Malformed)?;
            let family = match method {
                "can_use_tool" => {
                    if !p["tool_name"].is_string() || !p["input"].is_object() {
                        return Err(CodecError::Malformed);
                    }
                    for key in ["default_to_no", "suppress_always_allow_rule"] {
                        if p.get(key).is_some_and(|v| !v.is_boolean()) {
                            return Err(CodecError::Malformed);
                        }
                    }
                    if p["tool_name"] == "AskUserQuestion" {
                        validate_questions(&p["input"]["questions"], true)?;
                        Family::Questions
                    } else {
                        Family::Tool
                    }
                }
                "elicitation" => {
                    if !p["mcp_server_name"].is_string() || !p["message"].is_string() {
                        return Err(CodecError::Malformed);
                    }
                    let mode = p["mode"].as_str().ok_or(CodecError::Malformed)?;
                    let family = match elicitation_family(mode) {
                        Ok(family) => family,
                        Err(_) => {
                            return Ok(NativeMessage::Unsupported(PrivateEnvelope(
                                envelope.clone(),
                            )))
                        }
                    };
                    if family == Family::Url
                        && (!p["url"].is_string() || !p["elicitation_id"].is_string())
                    {
                        return Err(CodecError::Malformed);
                    }
                    family
                }
                "hook_callback" => {
                    return Ok(NativeMessage::Machine {
                        family: MachineFamily::Hook,
                        request: PrivateEnvelope(envelope.clone()),
                    })
                }
                _ => {
                    return Ok(NativeMessage::Unsupported(PrivateEnvelope(
                        envelope.clone(),
                    )))
                }
            };
            (id, method.to_owned(), p.clone(), family)
        }
    };
    if family == Family::Questions && protocol == Protocol::Codex0158 {
        validate_questions(&params["questions"], false)?;
    }
    Ok(NativeMessage::Owner(NativeRequest {
        protocol,
        id,
        method,
        family,
        context: Context::read(&params),
        envelope: envelope.clone(),
        default_to_no: params["default_to_no"] == true,
        suppress_always: params["suppress_always_allow_rule"] == true,
        params,
    }))
}

fn elicitation_family(mode: &str) -> Result<Family, CodecError> {
    match mode {
        "form" => Ok(Family::Form),
        "openai/form" | "openaiForm" => Ok(Family::ExternalForm),
        "url" => Ok(Family::Url),
        "openai/userVerification" => Ok(Family::Verification),
        _ => Err(CodecError::Unsupported),
    }
}

pub fn encode(
    request: &NativeRequest,
    context: &Context,
    answer: &Answer,
) -> Result<Value, CodecError> {
    if context != &request.context {
        return Err(CodecError::StaleContext);
    }
    let p = &request.params;
    let result = match (request.family, answer) {
        (
            Family::LegacyCommand | Family::LegacyFile | Family::Command | Family::File,
            Answer::Decision { decision },
        ) => {
            decision_allowed(request, decision)?;
            json!({"decision":decision})
        }
        (
            Family::Permissions,
            Answer::Permissions {
                permissions,
                scope,
                strict_auto_review,
            },
        ) => {
            if !profile_subset(permissions, &p["permissions"]) {
                return Err(CodecError::Enlarged);
            }
            let mut result = json!({"permissions":permissions,"scope":if *scope == Scope::Turn {"turn"} else {"session"}});
            if let Some(strict) = strict_auto_review {
                result["strictAutoReview"] = json!(strict);
            }
            result
        }
        (Family::Questions, Answer::Questions { answers }) => question_result(request, answers)?,
        (
            Family::Tool,
            Answer::Tool {
                allow,
                message,
                updated_permissions,
            },
        ) => {
            if !allow {
                if updated_permissions.is_some() {
                    return Err(CodecError::WrongAnswer);
                }
                json!({"behavior":"deny","message":message.as_deref().unwrap_or("Owner declined")})
            } else {
                let mut result = json!({"behavior":"allow","updatedInput":p["input"]});
                if let Some(grant) = updated_permissions {
                    if request.suppress_always {
                        return Err(CodecError::NativeVeto);
                    }
                    if !grant.is_array()
                        || grant.as_array().unwrap().is_empty()
                        || grant != &p["permission_suggestions"]
                    {
                        return Err(CodecError::Unoffered);
                    }
                    // Only the whole native offer, unchanged, can become a persistent update.
                    result["updatedPermissions"] = grant.clone();
                }
                result
            }
        }
        (
            Family::Form | Family::ExternalForm | Family::Url | Family::Verification,
            Answer::Elicitation { action, content },
        ) => {
            let action_name = match action {
                Action::Accept => "accept",
                Action::Decline => "decline",
                Action::Cancel => "cancel",
            };
            let mut result = json!({"action":action_name});
            if *action == Action::Accept {
                if request.family != Family::Form {
                    return Err(CodecError::Unqualified);
                }
                let schema = if request.protocol == Protocol::Codex0158 {
                    &p["requestedSchema"]
                } else {
                    &p["requested_schema"]
                };
                supported_form(schema)?;
                let content = content.as_ref().ok_or(CodecError::WrongAnswer)?;
                if !form_content(schema, content) {
                    return Err(CodecError::WrongAnswer);
                }
                result["content"] = content.clone();
            } else if content.is_some() {
                return Err(CodecError::WrongAnswer);
            }
            result
        }
        _ => return Err(CodecError::WrongAnswer),
    };
    match request.protocol {
        Protocol::Codex0158 => {
            let schema = match request.family {
                Family::LegacyCommand => "ExecCommandApprovalResponse",
                Family::LegacyFile => "ApplyPatchApprovalResponse",
                Family::Command => "CommandExecutionRequestApprovalResponse",
                Family::File => "FileChangeRequestApprovalResponse",
                Family::Permissions => "PermissionsRequestApprovalResponse",
                Family::Questions => "ToolRequestUserInputResponse",
                _ => "McpServerElicitationRequestResponse",
            };
            validate_native(schema, &result)?;
            Ok(json!({"id":request.id.value(),"result":result}))
        }
        Protocol::Claude21288 => Ok(
            json!({"type":"control_response","response":{"subtype":"success","request_id":request.id.value(),"response":result}}),
        ),
    }
}

fn decision_allowed(r: &NativeRequest, d: &Value) -> Result<(), CodecError> {
    let p = &r.params;
    if matches!(r.family, Family::LegacyCommand | Family::LegacyFile) {
        let valid = matches!(
            d.as_str(),
            Some("approved" | "approved_for_session" | "abort")
        ) || (d.as_object().is_some_and(|o| o.len() == 1)
            && d["denied"].as_object().is_some_and(|o| o.len() == 1)
            && d["denied"]["rejection"].is_string());
        return if valid {
            Ok(())
        } else {
            Err(CodecError::Unoffered)
        };
    }
    if r.family == Family::File {
        return if matches!(
            d.as_str(),
            Some("accept" | "acceptForSession" | "decline" | "cancel")
        ) {
            Ok(())
        } else {
            Err(CodecError::Unoffered)
        };
    }
    if let Some(choices) = p["availableDecisions"].as_array() {
        if !choices.contains(d) {
            return Err(CodecError::Unoffered);
        }
    } else if !matches!(
        d.as_str(),
        Some("accept" | "acceptForSession" | "decline" | "cancel")
    ) {
        return Err(CodecError::Unoffered);
    }
    if let Some(a) = d.get("acceptWithExecpolicyAmendment") {
        if a["execpolicy_amendment"] != p["proposedExecpolicyAmendment"]
            || !a["execpolicy_amendment"].is_array()
        {
            return Err(CodecError::Unoffered);
        }
    }
    if let Some(a) = d.get("applyNetworkPolicyAmendment") {
        if !p["proposedNetworkPolicyAmendments"]
            .as_array()
            .is_some_and(|offered| offered.contains(&a["network_policy_amendment"]))
        {
            return Err(CodecError::Unoffered);
        }
    }
    Ok(())
}

// Conservative equality for roots/globs/descriptors: no string-prefix authority.
// A subset may remove grants, never remove a deny while retaining any grants.
fn profile_subset(grant: &Value, offered: &Value) -> bool {
    let Some(g) = grant.as_object() else {
        return false;
    };
    if g.is_empty() {
        return true;
    }
    if g.keys()
        .any(|k| !["network", "fileSystem"].contains(&k.as_str()))
    {
        return false;
    }
    if let Some(n) = g.get("network").filter(|v| !v.is_null()) {
        let Some(n) = n.as_object() else {
            return false;
        };
        if n.keys().any(|k| k != "enabled") {
            return false;
        }
        if let Some(enabled) = n.get("enabled").filter(|v| !v.is_null()) {
            if !enabled.is_boolean() || (*enabled == true && offered["network"]["enabled"] != true)
            {
                return false;
            }
        }
    }
    let fs = grant.get("fileSystem").filter(|v| !v.is_null());
    if let Some(fs) = fs {
        let Some(obj) = fs.as_object() else {
            return false;
        };
        if obj
            .keys()
            .any(|k| !["entries", "read", "write", "globScanMaxDepth"].contains(&k.as_str()))
        {
            return false;
        }
        for field in ["entries", "read", "write"] {
            if let Some(a) = obj.get(field).filter(|v| !v.is_null()) {
                let Some(a) = a.as_array() else {
                    return false;
                };
                if !a.is_empty()
                    && !offered["fileSystem"][field]
                        .as_array()
                        .is_some_and(|b| a.iter().all(|item| b.contains(item)))
                {
                    return false;
                }
            }
        }
        let depth = fs
            .get("globScanMaxDepth")
            .filter(|v| !v.is_null())
            .and_then(Value::as_u64);
        let bound = offered["fileSystem"]["globScanMaxDepth"].as_u64();
        if fs
            .get("globScanMaxDepth")
            .is_some_and(|v| !v.is_null() && (depth.is_none() || depth == Some(0)))
        {
            return false;
        }
        if bound.is_some() && (depth.is_none() || depth > bound) {
            return false;
        }
    }
    if let Some(entries) = offered["fileSystem"]["entries"].as_array() {
        for deny in entries.iter().filter(|e| e["access"] == "deny") {
            if !grant["fileSystem"]["entries"]
                .as_array()
                .is_some_and(|a| a.contains(deny))
            {
                return false;
            }
        }
    }
    true
}

fn validate_questions(q: &Value, claude: bool) -> Result<(), CodecError> {
    let q = q.as_array().ok_or(CodecError::Malformed)?;
    if q.is_empty() {
        return Err(CodecError::Malformed);
    }
    let mut keys = std::collections::BTreeSet::new();
    for question in q {
        let key = question[if claude { "question" } else { "id" }]
            .as_str()
            .ok_or(CodecError::Malformed)?;
        if key.is_empty() || !keys.insert(key) {
            return Err(CodecError::Malformed);
        }
        if let Some(options) = question.get("options").filter(|v| !v.is_null()) {
            let options = options.as_array().ok_or(CodecError::Malformed)?;
            let mut labels = std::collections::BTreeSet::new();
            for o in options {
                let label = o["label"].as_str().ok_or(CodecError::Malformed)?;
                if !labels.insert(label) {
                    return Err(CodecError::Malformed);
                }
            }
        }
    }
    Ok(())
}
fn question_result(
    r: &NativeRequest,
    answers: &BTreeMap<String, Vec<String>>,
) -> Result<Value, CodecError> {
    let claude = r.protocol == Protocol::Claude21288;
    let q = if claude {
        &r.params["input"]["questions"]
    } else {
        &r.params["questions"]
    };
    let q = q.as_array().ok_or(CodecError::Malformed)?;
    if q.len() != answers.len() {
        return Err(CodecError::WrongAnswer);
    }
    let mut out = serde_json::Map::new();
    for question in q {
        let key = question[if claude { "question" } else { "id" }]
            .as_str()
            .ok_or(CodecError::Malformed)?;
        let a = answers.get(key).ok_or(CodecError::WrongAnswer)?;
        if a.is_empty()
            || a.iter().any(|s| s.is_empty())
            || (!claude && a.len() != 1)
            || (claude && question["multiSelect"] != true && a.len() != 1)
        {
            return Err(CodecError::WrongAnswer);
        }
        let unique: std::collections::BTreeSet<_> = a.iter().collect();
        if unique.len() != a.len() {
            return Err(CodecError::WrongAnswer);
        }
        if let Some(options) = question["options"].as_array() {
            if !claude
                && question["isOther"] != true
                && a.iter().any(|s| !options.iter().any(|o| o["label"] == *s))
            {
                return Err(CodecError::WrongAnswer);
            }
        }
        out.insert(
            key.to_owned(),
            if claude {
                json!(a.join(", "))
            } else {
                json!({"answers":a})
            },
        );
    }
    if claude {
        let mut input = r.params["input"].clone();
        input["answers"] = Value::Object(out);
        Ok(json!({"behavior":"allow","updatedInput":input}))
    } else {
        Ok(json!({"answers":out}))
    }
}

fn supported_form(s: &Value) -> Result<(), CodecError> {
    if s["type"] != "object" || !s["properties"].is_object() || !supported_schema(s, 0) {
        return Err(CodecError::Unqualified);
    }
    Ok(())
}
fn supported_schema(s: &Value, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let Some(o) = s.as_object() else {
        return s.is_boolean();
    };
    const KEYS: &[&str] = &[
        "type",
        "title",
        "description",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "enum",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "minItems",
        "maxItems",
    ];
    if o.keys().any(|k| !KEYS.contains(&k.as_str())) {
        return false;
    }
    if let Some(p) = o.get("properties") {
        let Some(p) = p.as_object() else {
            return false;
        };
        if p.values().any(|v| !supported_schema(v, depth + 1)) {
            return false;
        }
    }
    if let Some(i) = o.get("items") {
        if !supported_schema(i, depth + 1) {
            return false;
        }
    }
    if let Some(a) = o.get("additionalProperties") {
        if !supported_schema(a, depth + 1) {
            return false;
        }
    }
    if let Some(t) = o.get("type") {
        if !t.as_str().is_some_and(|t| {
            [
                "object", "array", "string", "boolean", "integer", "number", "null",
            ]
            .contains(&t)
        }) {
            return false;
        }
    }
    if let Some(r) = o.get("required") {
        if !r.as_array().is_some_and(|a| a.iter().all(Value::is_string)) {
            return false;
        }
    }
    if let Some(e) = o.get("enum") {
        if !e.is_array() {
            return false;
        }
    }
    for k in ["minimum", "maximum"] {
        if o.get(k).is_some_and(|v| !v.is_number()) {
            return false;
        }
    }
    // These counters must have the same bounded integer representation used
    // when enforcing them. A fraction, negative or overflow cannot be ignored.
    for k in ["minLength", "maxLength", "minItems", "maxItems"] {
        if o.get(k).is_some_and(|v| v.as_u64().is_none()) {
            return false;
        }
    }
    true
}
fn form_content(s: &Value, v: &Value) -> bool {
    if !schema_matches(s, v, s, 0) {
        return false;
    }
    // Owner forms never accept undeclared fields even when the native schema
    // omits additionalProperties, so secret/unoffered fields cannot be echoed.
    if let Some(obj) = v.as_object() {
        if obj.iter().any(|(k, item)| {
            s["properties"]
                .get(k)
                .is_none_or(|rule| !form_content(rule, item))
        }) {
            return false;
        }
    }
    if let Some(a) = v.as_array() {
        if let Some(items) = s.get("items") {
            if a.iter().any(|item| !form_content(items, item)) {
                return false;
            }
        }
    }
    true
}

// Frozen native schema validation is intentionally bounded. It rejects every
// unrecognized validation keyword; it is not a general JSON-Schema engine.
fn schema_matches(s: &Value, v: &Value, doc: &Value, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    if let Some(b) = s.as_bool() {
        return b;
    }
    let Some(o) = s.as_object() else {
        return false;
    };
    const KEYS: &[&str] = &[
        "$schema",
        "title",
        "description",
        "definitions",
        "default",
        "enumNames",
        "$ref",
        "type",
        "enum",
        "properties",
        "required",
        "additionalProperties",
        "oneOf",
        "anyOf",
        "allOf",
        "items",
        "minItems",
        "maxItems",
        "minLength",
        "maxLength",
        "pattern",
        "minimum",
        "maximum",
        "format",
    ];
    if o.keys().any(|k| !KEYS.contains(&k.as_str())) {
        return false;
    }
    if let Some(reference) = s["$ref"].as_str() {
        let Some(pointer) = reference.strip_prefix('#') else {
            return false;
        };
        if !doc
            .pointer(pointer)
            .is_some_and(|target| schema_matches(target, v, doc, depth + 1))
        {
            return false;
        }
    }
    for key in ["allOf", "anyOf", "oneOf"] {
        if let Some(a) = s.get(key) {
            let Some(a) = a.as_array() else {
                return false;
            };
            let count = a
                .iter()
                .filter(|p| schema_matches(p, v, doc, depth + 1))
                .count();
            if (key == "allOf" && count != a.len())
                || (key == "anyOf" && count == 0)
                || (key == "oneOf" && count != 1)
            {
                return false;
            }
        }
    }
    if let Some(t) = s.get("type") {
        let matches = |t: &Value| match t.as_str() {
            Some("null") => v.is_null(),
            Some("boolean") => v.is_boolean(),
            Some("string") => v.is_string(),
            Some("object") => v.is_object(),
            Some("array") => v.is_array(),
            Some("integer") => v.as_i64().is_some() || v.as_u64().is_some(),
            Some("number") => v.is_number(),
            _ => false,
        };
        if !t
            .as_array()
            .map(|a| a.iter().any(matches))
            .unwrap_or_else(|| matches(t))
        {
            return false;
        }
    }
    if let Some(e) = s.get("enum") {
        if !e.as_array().is_some_and(|a| a.contains(v)) {
            return false;
        }
    }
    if let Some(obj) = v.as_object() {
        if let Some(required) = s["required"].as_array() {
            if required
                .iter()
                .any(|k| !k.as_str().is_some_and(|k| obj.contains_key(k)))
            {
                return false;
            }
        }
        for (k, item) in obj {
            if let Some(rule) = s["properties"]
                .get(k)
                .or_else(|| s.get("additionalProperties"))
            {
                if !schema_matches(rule, item, doc, depth + 1) {
                    return false;
                }
            }
        }
    }
    if let Some(a) = v.as_array() {
        if s["minItems"]
            .as_u64()
            .is_some_and(|n| a.len() < (n as usize))
            || s["maxItems"]
                .as_u64()
                .is_some_and(|n| a.len() > (n as usize))
        {
            return false;
        }
        if let Some(items) = s.get("items") {
            if a.iter()
                .any(|item| !schema_matches(items, item, doc, depth + 1))
            {
                return false;
            }
        }
    }
    if let Some(text) = v.as_str() {
        let len = text.chars().count() as u64;
        if s["minLength"].as_u64().is_some_and(|n| len < n)
            || s["maxLength"].as_u64().is_some_and(|n| len > n)
        {
            return false;
        }
        if let Some(pattern) = s["pattern"].as_str() {
            if !regex::Regex::new(pattern).is_ok_and(|r| r.is_match(text)) {
                return false;
            }
        }
    }
    if let Some(n) = v.as_f64() {
        if s["minimum"].as_f64().is_some_and(|b| n < b)
            || s["maximum"].as_f64().is_some_and(|b| n > b)
        {
            return false;
        }
        match s["format"].as_str() {
            Some("int64") if v.as_i64().is_none() => return false,
            Some("uint" | "uint64") if v.as_u64().is_none() => return false,
            Some("uint32") if !v.as_u64().is_some_and(|n| n <= u32::MAX as u64) => return false,
            Some("int64" | "uint" | "uint64" | "uint32" | "double") | None => {}
            Some(_) => return false,
        }
    }
    true
}

fn validate_native(name: &str, value: &Value) -> Result<(), CodecError> {
    macro_rules! schema {
        ($name:literal) => {
            include_str!(concat!(
                "../../../fixtures/transcripts/ac274/schemas/codex-0.158.0/",
                $name,
                ".json"
            ))
        };
    }
    let text = match name {
        "ExecCommandApprovalParams" => schema!("ExecCommandApprovalParams"),
        "ExecCommandApprovalResponse" => schema!("ExecCommandApprovalResponse"),
        "ApplyPatchApprovalParams" => schema!("ApplyPatchApprovalParams"),
        "ApplyPatchApprovalResponse" => schema!("ApplyPatchApprovalResponse"),
        "CommandExecutionRequestApprovalParams" => schema!("CommandExecutionRequestApprovalParams"),
        "CommandExecutionRequestApprovalResponse" => {
            schema!("CommandExecutionRequestApprovalResponse")
        }
        "FileChangeRequestApprovalParams" => schema!("FileChangeRequestApprovalParams"),
        "FileChangeRequestApprovalResponse" => schema!("FileChangeRequestApprovalResponse"),
        "PermissionsRequestApprovalParams" => schema!("PermissionsRequestApprovalParams"),
        "PermissionsRequestApprovalResponse" => schema!("PermissionsRequestApprovalResponse"),
        "ToolRequestUserInputParams" => schema!("ToolRequestUserInputParams"),
        "ToolRequestUserInputResponse" => schema!("ToolRequestUserInputResponse"),
        "McpServerElicitationRequestParams" => schema!("McpServerElicitationRequestParams"),
        "McpServerElicitationRequestResponse" => schema!("McpServerElicitationRequestResponse"),
        "ServerRequestResolvedNotification" => schema!("ServerRequestResolvedNotification"),
        "DynamicToolCallParams" => schema!("DynamicToolCallParams"),
        "ChatgptAuthTokensRefreshParams" => schema!("ChatgptAuthTokensRefreshParams"),
        "AttestationGenerateParams" => schema!("AttestationGenerateParams"),
        "CurrentTimeReadParams" => schema!("CurrentTimeReadParams"),
        _ => return Err(CodecError::Unsupported),
    };
    let schema: Value = serde_json::from_str(text).map_err(|_| CodecError::Malformed)?;
    if schema_matches(&schema, value, &schema, 0) {
        Ok(())
    } else {
        Err(CodecError::Malformed)
    }
}
