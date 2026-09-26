//! Local-only OpenCode provider discovery. This module never contacts a model.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LocalModel {
    pub provider_id: String,
    pub model: String,
    pub endpoint: String,
    pub context_limit: Option<u64>,
    pub toolcall: bool,
    pub reasoning: bool,
    pub variants: Vec<String>,
    pub is_default: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LocalCatalog {
    pub observed_ms: i64,
    pub expires_ms: i64,
    pub models: Vec<LocalModel>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointProbe {
    Reachable,
    Unavailable,
    Invalid,
}

fn identifier(value: &str, nested: bool) -> bool {
    !value.is_empty() && value.len() <= 120 && !value.contains("..")
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.') || nested && byte == b'/')
}

/// Only numeric IPv4 loopback endpoints qualify. A host name, user-info,
/// remote URL, zero port, or URL metacharacter cannot turn into a local route.
fn loopback_port(url: &str) -> Option<u16> {
    let suffix = url.strip_prefix("http://127.0.0.1:")?;
    let (port, path) = suffix.split_once('/')?;
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit())
        || path.is_empty() || path.len() > 80
        || !path.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.')) {
        return None;
    }
    port.parse::<u16>().ok().filter(|value| *value > 0)
}

/// Only a validated local endpoint escapes a host-offline health signal.
pub fn is_verified_loopback_endpoint(url: &str) -> bool {
    loopback_port(url).is_some()
}

/// The numeric socket identity shared by endpoint URL aliases. Different
/// paths on one local listener conservatively share one unknown-draw pool.
pub(crate) fn verified_loopback_port(url: &str) -> Option<u16> {
    loopback_port(url)
}

/// A loopback TCP check is metadata-only; reachability is not a model-turn
/// success or evidence about the provider's quality, permissions, or quota.
pub fn probe_local_endpoint(url: &str) -> EndpointProbe {
    let Some(port) = loopback_port(url) else { return EndpointProbe::Invalid };
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    match TcpStream::connect_timeout(&address.into(), Duration::from_millis(250)) {
        Ok(stream) => { drop(stream); EndpointProbe::Reachable }
        Err(_) => EndpointProbe::Unavailable,
    }
}

/// Cross-check OpenCode's structured `/config/providers` response against its
/// explicit project configuration. `/provider` is also accepted for fixtures;
/// its `connected` list can include cloud providers even with isolated XDG
/// directories, so that list alone is never authorization.
/// Raw provider objects, credentials, API prices, and arbitrary model text are
/// never retained. Unknown or credentialed providers are left for manual use.
pub fn parse_local_catalog(config: &Value, response: &Value, observed_ms: i64) -> Result<LocalCatalog> {
    let configured = config.get("provider").and_then(Value::as_object)
        .ok_or_else(|| anyhow!("OpenCode project has no explicit provider map"))?;
    let providers = response.get("providers").or_else(|| response.get("all")).and_then(Value::as_array)
        .ok_or_else(|| anyhow!("OpenCode provider metadata is unavailable"))?;
    let connected = response.get("connected").and_then(Value::as_array);
    if configured.len() > 128 || providers.len() > 128 || connected.is_some_and(|list| list.len() > 128) {
        return Err(anyhow!("OpenCode provider metadata exceeded its bound"));
    }
    let default_model = config.get("model").and_then(Value::as_str);
    let mut models = Vec::new();
    let mut seen_providers = BTreeSet::new();
    for provider in providers {
        let Some(id) = provider.get("id").and_then(Value::as_str).filter(|id| identifier(id, false)) else { continue };
        let Some(configuration) = configured.get(id) else { continue };
        if !seen_providers.insert(id) { return Err(anyhow!("duplicate OpenCode local provider metadata")); }
        if provider.get("source").and_then(Value::as_str) != Some("config")
            || connected.is_some_and(|list| !list.iter().any(|value| value.as_str() == Some(id)))
            || configuration.get("npm").and_then(Value::as_str) != Some("@ai-sdk/openai-compatible")
            || provider.get("env").and_then(Value::as_array).is_some_and(|env| !env.is_empty()) {
            continue;
        }
        let Some(config_options) = configuration.get("options").and_then(Value::as_object) else { continue };
        let Some(reported_options) = provider.get("options").and_then(Value::as_object) else { continue };
        if config_options.keys().any(|key| key != "baseURL")
            || reported_options.keys().any(|key| key != "baseURL") { continue; }
        let Some(endpoint) = config_options.get("baseURL").and_then(Value::as_str)
            .filter(|url| loopback_port(url).is_some()) else { continue };
        if reported_options.get("baseURL").and_then(Value::as_str) != Some(endpoint) { continue; }
        let Some(config_models) = configuration.get("models").and_then(Value::as_object) else { continue };
        let Some(reported_models) = provider.get("models").and_then(Value::as_object) else { continue };
        if config_models.len() > 128 || reported_models.len() > 128 { continue; }
        for (model_id, model) in reported_models {
            if models.len() >= 128 { return Err(anyhow!("OpenCode local model catalog exceeded its bound")); }
            if !identifier(model_id, true) || !config_models.contains_key(model_id)
                || model.get("id").and_then(Value::as_str) != Some(model_id)
                || model.get("providerID").and_then(Value::as_str) != Some(id)
                || model.get("status").and_then(Value::as_str) != Some("active") { continue; }
            let context_limit = model.pointer("/limit/context").and_then(Value::as_u64)
                .filter(|limit| (1..=10_000_000).contains(limit));
            let capabilities = &model["capabilities"];
            let variants = model.get("variants").and_then(Value::as_object)
                .map(|entries| entries.keys().filter(|variant| identifier(variant, false) && variant.len() <= 32)
                    .cloned().collect::<Vec<_>>()).unwrap_or_default();
            let full_model = format!("{id}/{model_id}");
            models.push(LocalModel { provider_id:id.into(), model:full_model.clone(),
                endpoint:endpoint.into(), context_limit,
                toolcall:capabilities["toolcall"] == true,
                reasoning:capabilities["reasoning"] == true,
                variants, is_default:default_model == Some(full_model.as_str()) });
        }
    }
    models.sort_by(|a,b| a.model.cmp(&b.model));
    Ok(LocalCatalog { observed_ms, expires_ms:observed_ms.saturating_add(60_000), models })
}

/// Narrow execution boundary for an explicit local OpenCode profile. The
/// metadata catalog alone cannot prove which secondary model, plugin, or
/// credential an actual `opencode run` will use. Recheck the child workspace
/// immediately before launch; this does not establish that a loopback proxy
/// itself is free or trustworthy.
pub fn auto_local_execution_guard(profile: &crate::store::Profile, project: &Path,
    model: &str, endpoint: &str) -> Result<()> {
    validate_auto_local_config(profile, project, model, endpoint, false)
}

/// A process-scoped override for one selected local provider. OpenCode's
/// `OPENCODE_CONFIG_CONTENT` is loaded after the project config, so this avoids
/// editing task files when Auto chooses a different provider. Only the model
/// and its small-model fallback change. The child launch boundary must call
/// this again against its own workspace before a model turn.
pub fn auto_local_inline_config(profile: &crate::store::Profile, project: &Path,
    model: &str, endpoint: &str) -> Result<String> {
    validate_auto_local_config(profile, project, model, endpoint, true)?;
    Ok(serde_json::json!({"model":model,"small_model":model,
        "autoupdate":false,"share":"disabled","plugin":[],
        "permission":{"*":"deny","read":"allow","glob":"allow",
            "grep":"allow","list":"allow"}}).to_string())
}

fn validate_auto_local_config(profile: &crate::store::Profile, project: &Path,
    model: &str, endpoint: &str, allow_inline_model_override: bool) -> Result<()> {
    if profile.harness != "opencode" || profile.is_system {
        return Err(anyhow!("automatic local OpenCode requires an isolated profile"));
    }
    let home = profile.home.as_deref().map(Path::new)
        .ok_or_else(|| anyhow!("isolated OpenCode profile home is unavailable"))?;
    for dir in [home.to_path_buf(), home.join("data"), home.join("config")] {
        let metadata = std::fs::symlink_metadata(&dir)?;
        if !metadata.file_type().is_dir() { return Err(anyhow!("OpenCode profile path is not an owned directory")); }
    }
    match std::fs::symlink_metadata(home.join("data/opencode/auth.json")) {
        Ok(_) => return Err(anyhow!("automatic local OpenCode profile contains credentials")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let config_home = home.join("config/opencode");
    match std::fs::symlink_metadata(&config_home) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(anyhow!("automatic local OpenCode profile config is not a directory"));
        }
        Ok(_) => {
            for entry in std::fs::read_dir(&config_home)? {
                let entry = entry?;
                let metadata = std::fs::symlink_metadata(entry.path())?;
                match entry.file_name().to_str() {
                    Some(".gitignore") if metadata.file_type().is_file() && metadata.len() <= 4096 => {}
                    Some("opencode.jsonc") if metadata.file_type().is_file() && metadata.len() <= 4096 => {
                        let generated: Value = serde_json::from_slice(&std::fs::read(entry.path())?)?;
                        if generated.as_object().is_none_or(|map| map.len() != 1
                            || map.get("$schema").and_then(Value::as_str) != Some("https://opencode.ai/config.json")) {
                            return Err(anyhow!("automatic local OpenCode profile config was modified"));
                        }
                    }
                    _ => return Err(anyhow!("automatic local OpenCode profile has unverified configuration")),
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // OpenCode may load either alternate project file and project-specific
    // agents/plugins. A narrow local route cannot validate their effects.
    for name in ["opencode.jsonc", ".opencode"] {
        match std::fs::symlink_metadata(project.join(name)) {
            Ok(_) => return Err(anyhow!("automatic local OpenCode project has alternate configuration")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let config_path = project.join("opencode.json");
    let metadata = std::fs::symlink_metadata(&config_path)?;
    if !metadata.file_type().is_file() || metadata.len() > 128 * 1024 {
        return Err(anyhow!("automatic local OpenCode project config is unavailable"));
    }
    let config: Value = serde_json::from_slice(&std::fs::read(config_path)?)?;
    let root = config.as_object().ok_or_else(|| anyhow!("invalid OpenCode project config"))?;
    if root.keys().any(|key| !matches!(key.as_str(), "$schema" | "provider" | "model" | "small_model" | "autoupdate" | "share"))
        || root.get("$schema").is_some_and(|value| value != "https://opencode.ai/config.json") {
        return Err(anyhow!("automatic local OpenCode config has unverified fields"));
    }
    if allow_inline_model_override && config["$schema"] != "https://opencode.ai/config.json" {
        return Err(anyhow!("automatic local OpenCode inline route requires a stable project config"));
    }
    if config["autoupdate"] != false || config["share"] != "disabled" {
        return Err(anyhow!("automatic local OpenCode updates or sharing are not disabled"));
    }
    let default_model = config["model"].as_str().filter(|default| *default == config["small_model"])
        .ok_or_else(|| anyhow!("automatic local OpenCode default and small models disagree"))?;
    if !allow_inline_model_override && default_model != model {
        return Err(anyhow!("automatic local OpenCode selected and small models disagree"));
    }
    let (selected_provider, selected_model) = model.split_once('/')
        .ok_or_else(|| anyhow!("invalid OpenCode model identity"))?;
    if !identifier(selected_provider, false) || !identifier(selected_model, true)
        || loopback_port(endpoint).is_none() {
        return Err(anyhow!("invalid local OpenCode route"));
    }
    let providers = config["provider"].as_object()
        .filter(|map| !map.is_empty() && map.len() <= 128)
        .ok_or_else(|| anyhow!("automatic local OpenCode provider map is unavailable"))?;
    let mut selected_found = false;
    let mut default_found = false;
    for (id, provider) in providers {
        if !identifier(id, false) { return Err(anyhow!("invalid local OpenCode provider")); }
        let object = provider.as_object().ok_or_else(|| anyhow!("invalid local OpenCode provider"))?;
        if object.keys().any(|key| !matches!(key.as_str(), "npm" | "options" | "models"))
            || provider["npm"] != "@ai-sdk/openai-compatible" {
            return Err(anyhow!("automatic OpenCode provider is not a credential-free local adapter"));
        }
        let options = provider["options"].as_object().filter(|map| map.len() == 1 && map.contains_key("baseURL"))
            .ok_or_else(|| anyhow!("automatic OpenCode provider has unverified options"))?;
        let url = options["baseURL"].as_str().filter(|url| loopback_port(url).is_some())
            .ok_or_else(|| anyhow!("automatic OpenCode provider has a nonlocal endpoint"))?;
        let models = provider["models"].as_object().filter(|map| !map.is_empty() && map.len() <= 128)
            .ok_or_else(|| anyhow!("automatic OpenCode provider has no bounded models"))?;
        for (model_id, model_config) in models {
            if !identifier(model_id, true) || !model_config.as_object().is_some_and(|item|
                item.keys().all(|key| matches!(key.as_str(), "name" | "tool_call" | "reasoning" | "limit" | "variants"))) {
                return Err(anyhow!("automatic OpenCode model config has unverified fields"));
            }
        }
        if id == selected_provider {
            if url != endpoint || !models.contains_key(selected_model) {
                return Err(anyhow!("selected OpenCode route changed before launch"));
            }
            selected_found = true;
        }
        if default_model.strip_prefix(id).is_some_and(|suffix| suffix.starts_with('/')
            && models.contains_key(&suffix[1..])) {
            default_found = true;
        }
    }
    if !selected_found { return Err(anyhow!("selected OpenCode provider is unavailable")); }
    if !default_found { return Err(anyhow!("configured OpenCode default is not a local provider model")); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::TcpListener;

    #[test]
    fn structured_discovery_keeps_two_local_providers_separate_and_drops_cloud_and_credentials() {
        let config = json!({"provider":{
            "local_a":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:47811/v1"},"models":{"fixture-a":{}}},
            "local_b":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:47812/v1"},"models":{"fixture-b":{}}},
            "remote":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://api.example.test/v1","apiKey":"secret-sentinel"},"models":{"cloud":{}}}
        }});
        let provider = |id: &str, endpoint: &str, model: &str| json!({
            "id":id,"source":"config","options":{"baseURL":endpoint},"models":{
                (model):{"id":model,"providerID":id,"status":"active","limit":{"context":32000,"output":4096},
                    "capabilities":{"toolcall":true,"reasoning":false},"variants":{},"cost":{"input":9999}}
            }});
        let payload = json!({"connected":["local_a","local_b","remote","openai"],"all":[
            provider("local_a","http://127.0.0.1:47811/v1","fixture-a"),
            provider("local_b","http://127.0.0.1:47812/v1","fixture-b"),
            provider("remote","https://api.example.test/v1","cloud"),
            {"id":"openai","source":"models.dev","models":{"gpt":{"id":"gpt"}}}
        ]});
        let catalog = parse_local_catalog(&config, &payload, 1000).unwrap();
        assert_eq!(catalog.models.len(), 2);
        assert_eq!(catalog.models[0].model, "local_a/fixture-a");
        assert_eq!(catalog.models[1].model, "local_b/fixture-b");
        assert_ne!(catalog.models[0].endpoint, catalog.models[1].endpoint);
        assert_eq!(catalog.models[0].context_limit, Some(32000));
        let stored = serde_json::to_string(&catalog).unwrap();
        assert!(!stored.contains("secret-sentinel") && !stored.contains("9999"));
        let smaller_response = json!({"providers":payload["all"],"default":{"local_a":"fixture-a"}});
        assert_eq!(parse_local_catalog(&config, &smaller_response, 1000).unwrap().models.len(), 2,
            "the installed /config/providers interface omits the misleading connected list");
    }

    #[test]
    fn failed_local_endpoint_does_not_erase_an_independent_available_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let good = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
        let failed = "http://127.0.0.1:1/v1";
        assert_eq!(probe_local_endpoint(&good), EndpointProbe::Reachable);
        assert_eq!(probe_local_endpoint(failed), EndpointProbe::Unavailable);
    }

    #[test]
    fn malformed_or_credentialed_local_provider_never_becomes_a_candidate() {
        let config = json!({"provider":{"local":{"npm":"@ai-sdk/openai-compatible",
            "options":{"baseURL":"http://127.0.0.1:47811/v1","headers":{"Authorization":"secret-sentinel"}},
            "models":{"fixture":{}}}}});
        let payload = json!({"connected":["local"],"all":[{"id":"local","source":"config",
            "options":{"baseURL":"http://127.0.0.1:47811/v1"},"models":{"fixture":{
                "id":"fixture","providerID":"local","status":"active","limit":{"context":32000},
                "capabilities":{"toolcall":true},"variants":{}}}}]});
        assert!(parse_local_catalog(&config, &payload, 1000).unwrap().models.is_empty());
        for url in ["http://localhost:47811/v1", "http://127.0.0.1:47811@evil.test/v1",
            "https://127.0.0.1:47811/v1", "http://127.0.0.1:0/v1"] {
            assert_eq!(probe_local_endpoint(url), EndpointProbe::Invalid, "{url}");
        }
    }

    #[test]
    fn installed_opencode_reports_two_explicit_local_providers_without_a_model_turn() {
        let Some(program) = crate::adapters::resolve_program("opencode") else { return };
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let config = json!({"provider":{
            "local_a":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:47811/v1"},"models":{"fixture-a":{"name":"Fixture A","tool_call":true}}},
            "local_b":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:47812/v1"},"models":{"fixture-b":{"name":"Fixture B","tool_call":true}}}
        },"model":"local_a/fixture-a","small_model":"local_a/fixture-a","autoupdate":false,"share":"disabled"});
        std::fs::write(project.join("opencode.json"), serde_json::to_vec(&config).unwrap()).unwrap();
        let mut env = crate::adapters::base_env(&program.display().to_string());
        for (key, dir_name) in [("XDG_CONFIG_HOME","config"),("XDG_DATA_HOME","data"),
            ("XDG_CACHE_HOME","cache"),("XDG_STATE_HOME","state")] {
            let path = dir.path().join(dir_name);
            std::fs::create_dir(&path).unwrap();
            env.insert(key.into(), path.display().to_string());
        }
        // The installed CLI can miss an eight-second cold-start deadline under
        // full-suite load. Only that bounded timeout is retried; other errors
        // still fail immediately so parser or auth regressions remain visible.
        let mut catalog = None;
        for _ in 0..3 {
            match crate::auto_collect::opencode_local_catalog(&program, &env, &project,
                Duration::from_secs(8), 1000) {
                Ok(found) => { catalog = Some(found); break; }
                Err(error) if error.to_string().contains("metadata read timed out") => continue,
                Err(error) => panic!("OpenCode metadata discovery failed: {error}"),
            }
        }
        let catalog = catalog.expect("installed OpenCode metadata never became available");
        assert_eq!(catalog.models.iter().map(|m| m.model.as_str()).collect::<Vec<_>>(),
            ["local_a/fixture-a", "local_b/fixture-b"]);
    }

    #[test]
    fn installed_opencode_uses_process_scoped_alternate_local_model() {
        let Some(program) = crate::adapters::resolve_program("opencode") else { return };
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let home = dir.path().join("profile");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(home.join("data/opencode")).unwrap();
        std::fs::create_dir_all(home.join("config")).unwrap();
        let profile = crate::store::Profile { id:"p-local".into(), name:"Local".into(),
            harness:"opencode".into(), home:Some(home.display().to_string()),
            is_system:false, created_ms:0 };
        let port_file = dir.path().join("mock-port");
        let mock_log = dir.path().join("mock-log");
        let server_file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/mock-openai/server.js");
        let mut server = std::process::Command::new("node").arg(server_file)
            .env("MOCK_PORT_FILE", &port_file).env("MOCK_LOG", &mock_log)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .spawn().unwrap();
        struct StopServer<'a>(&'a mut std::process::Child);
        impl Drop for StopServer<'_> {
            fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
        }
        let _server_guard = StopServer(&mut server);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !port_file.exists() {
            assert!(std::time::Instant::now() < deadline, "local mock did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let port = std::fs::read_to_string(&port_file).unwrap();
        let alternate = format!("http://127.0.0.1:{port}/v1");
        let config = serde_json::json!({"$schema":"https://opencode.ai/config.json","provider":{
            "local_a":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:1/v1"},
                "models":{"fixture-a":{"name":"Fixture A","tool_call":true}}},
            "local_b":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":alternate},
                "models":{"fixture-b":{"name":"Fixture B","tool_call":true}}}
        },"model":"local_a/fixture-a","small_model":"local_a/fixture-a",
            "autoupdate":false,"share":"disabled"});
        std::fs::write(project.join("opencode.json"), config.to_string()).unwrap();
        let inline = auto_local_inline_config(&profile, &project, "local_b/fixture-b", &alternate).unwrap();
        let mut env = crate::adapters::base_env(&program.display().to_string());
        env.extend(crate::daemon::Daemon::profile_env(&profile));
        env.insert("OPENCODE_CONFIG_CONTENT".into(), inline);
        env.insert("OPENCODE_DISABLE_MODELS_FETCH".into(), "true".into());
        env.insert("OPENCODE_DISABLE_DEFAULT_PLUGINS".into(), "true".into());
        let mut child = std::process::Command::new(program)
            .args(["run", "--format", "json", "-m", "local_b/fixture-b", "--", "write forbidden.txt"])
            .current_dir(&project).env_clear().envs(env)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .spawn().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "OpenCode alternate local route failed");
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill(); let _ = child.wait();
                panic!("OpenCode alternate local route timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let requests = std::fs::read_to_string(mock_log).unwrap();
        assert!(requests.contains("/v1/chat/completions"), "alternate provider saw no model turn");
        for request in requests.lines() {
            let request: Value = serde_json::from_str(request).unwrap();
            let tools = request["tools"].as_array().unwrap();
            for forbidden in ["bash", "edit", "write", "apply_patch", "task", "webfetch", "websearch"] {
                assert!(!tools.iter().any(|tool| tool == forbidden),
                    "read-only route exposed {forbidden} to the model");
            }
        }
        assert!(!project.join("forbidden.txt").exists());
        let project_config: Value = serde_json::from_slice(&std::fs::read(project.join("opencode.json")).unwrap()).unwrap();
        assert_eq!(project_config, config, "process-scoped override must not edit the project");
    }

    #[test]
    fn duplicate_provider_metadata_cannot_multiply_one_local_route() {
        let config = json!({"provider":{"local_a":{"npm":"@ai-sdk/openai-compatible",
            "options":{"baseURL":"http://127.0.0.1:47811/v1"},"models":{"fixture-a":{}}}}});
        let provider = json!({"id":"local_a","source":"config","options":{"baseURL":"http://127.0.0.1:47811/v1"},
            "models":{"fixture-a":{"id":"fixture-a","providerID":"local_a","status":"active","limit":{"context":32000},
                "capabilities":{"toolcall":true},"variants":{}}}});
        let response = json!({"providers":[provider.clone(),provider]});
        assert!(parse_local_catalog(&config, &response, 1000).is_err());
    }

    #[test]
    fn automatic_local_execution_requires_isolated_credentials_and_only_local_config() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("profile");
        let project = root.path().join("project");
        std::fs::create_dir_all(home.join("data/opencode")).unwrap();
        std::fs::create_dir_all(home.join("config")).unwrap();
        std::fs::create_dir(&project).unwrap();
        let profile = crate::store::Profile { id:"p-local".into(), name:"Local".into(),
            harness:"opencode".into(), home:Some(home.display().to_string()),
            is_system:false, created_ms:0 };
        let endpoint = "http://127.0.0.1:47811/v1";
        let mut config = serde_json::json!({"provider":{
            "local_a":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":endpoint},
                "models":{"fixture-a":{"name":"Fixture A","tool_call":true}}},
            "local_b":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:47812/v1"},
                "models":{"fixture-b":{"name":"Fixture B","tool_call":true}}}
        },"model":"local_a/fixture-a","small_model":"local_a/fixture-a",
            "autoupdate":false,"share":"disabled"});
        let save = |value: &Value| std::fs::write(project.join("opencode.json"), value.to_string()).unwrap();
        save(&config);
        assert!(auto_local_execution_guard(&profile, &project, "local_a/fixture-a", endpoint).is_ok());
        assert!(auto_local_inline_config(&profile, &project,
            "local_b/fixture-b", "http://127.0.0.1:47812/v1").is_err(),
            "OpenCode mutates project config without its schema marker");
        config["$schema"] = serde_json::json!("https://opencode.ai/config.json");
        save(&config);
        assert!(auto_local_execution_guard(&profile, &project,
            "local_b/fixture-b", "http://127.0.0.1:47812/v1").is_err());
        let inline = auto_local_inline_config(&profile, &project,
            "local_b/fixture-b", "http://127.0.0.1:47812/v1").unwrap();
        let inline: Value = serde_json::from_str(&inline).unwrap();
        assert_eq!(inline["model"], "local_b/fixture-b");
        assert_eq!(inline["small_model"], "local_b/fixture-b");
        assert_eq!(inline["autoupdate"], false);
        assert_eq!(inline["share"], "disabled");
        assert_eq!(inline["plugin"], serde_json::json!([]));
        assert_eq!(inline["permission"]["*"], "deny");
        assert_eq!(inline["permission"]["read"], "allow");
        assert_eq!(inline["permission"]["edit"], Value::Null);
        let generated = home.join("config/opencode");
        std::fs::create_dir_all(&generated).unwrap();
        std::fs::write(generated.join(".gitignore"), "node_modules\n").unwrap();
        std::fs::write(generated.join("opencode.jsonc"),
            r#"{"$schema":"https://opencode.ai/config.json"}"#).unwrap();
        assert!(auto_local_execution_guard(&profile, &project, "local_a/fixture-a", endpoint).is_ok());
        std::fs::write(generated.join("opencode.jsonc"),
            r#"{"$schema":"https://opencode.ai/config.json","plugin":["unsafe"]}"#).unwrap();
        assert!(auto_local_execution_guard(&profile, &project, "local_a/fixture-a", endpoint).is_err());
        std::fs::write(generated.join("opencode.jsonc"),
            r#"{"$schema":"https://opencode.ai/config.json"}"#).unwrap();
        let mut remote = config.clone();
        remote["provider"]["cloud"] = serde_json::json!({"npm":"@ai-sdk/openai-compatible",
            "options":{"baseURL":"https://api.example.test/v1"},"models":{"paid":{}}});
        save(&remote);
        assert!(auto_local_execution_guard(&profile, &project, "local_a/fixture-a", endpoint).is_err());
        assert!(auto_local_inline_config(&profile, &project,
            "local_b/fixture-b", "http://127.0.0.1:47812/v1").is_err());
        let mut plugin = config.clone();
        plugin["plugin"] = serde_json::json!(["some-plugin"]);
        save(&plugin);
        assert!(auto_local_execution_guard(&profile, &project, "local_a/fixture-a", endpoint).is_err());
        assert!(auto_local_inline_config(&profile, &project,
            "local_b/fixture-b", "http://127.0.0.1:47812/v1").is_err());
        save(&config);
        std::fs::write(project.join("opencode.jsonc"), "{}").unwrap();
        assert!(auto_local_execution_guard(&profile, &project, "local_a/fixture-a", endpoint).is_err());
        std::fs::remove_file(project.join("opencode.jsonc")).unwrap();
        std::fs::create_dir(project.join(".opencode")).unwrap();
        assert!(auto_local_execution_guard(&profile, &project, "local_a/fixture-a", endpoint).is_err());
        std::fs::remove_dir(project.join(".opencode")).unwrap();
        std::fs::write(home.join("data/opencode/auth.json"), "secret-auth-sentinel").unwrap();
        assert!(auto_local_execution_guard(&profile, &project, "local_a/fixture-a", endpoint).is_err());
        assert!(auto_local_inline_config(&profile, &project,
            "local_b/fixture-b", "http://127.0.0.1:47812/v1").is_err());
        std::fs::remove_file(home.join("data/opencode/auth.json")).unwrap();
        let mut system = profile.clone();
        system.is_system = true;
        assert!(auto_local_execution_guard(&system, &project, "local_a/fixture-a", endpoint).is_err());
    }
}
