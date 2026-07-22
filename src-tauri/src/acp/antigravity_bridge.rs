use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use crate::acp::error::AcpError;
use crate::acp::types::PromptInputBlock;

const DEFAULT_PRINT_TIMEOUT: &str = "5m0s";
const DEFAULT_CHILD_TIMEOUT_SECS: u64 = 330;
const MODEL_DISCOVERY_TIMEOUT_SECS: u64 = 15;
const FALLBACK_MODELS: &[&str] = &[
    "Gemini 3.5 Flash (Medium)",
    "Gemini 3.5 Flash (High)",
    "Gemini 3.5 Flash (Low)",
    "Gemini 3.1 Pro (Low)",
    "Gemini 3.1 Pro (High)",
    "Claude Sonnet 4.6 (Thinking)",
    "Claude Opus 4.6 (Thinking)",
    "GPT-OSS 120B (Medium)",
];

static AVAILABLE_MODELS: tokio::sync::OnceCell<Vec<String>> = tokio::sync::OnceCell::const_new();

#[derive(Debug, Clone)]
pub struct AntigravityPrintOptions {
    pub command: PathBuf,
    pub workspace: PathBuf,
    pub model: Option<String>,
    pub project: Option<String>,
    pub conversation: Option<String>,
    pub print_timeout: String,
    pub child_timeout: Duration,
    pub env: BTreeMap<String, String>,
}

impl AntigravityPrintOptions {
    pub fn from_runtime_env(
        command: PathBuf,
        workspace: PathBuf,
        runtime_env: &BTreeMap<String, String>,
    ) -> Self {
        let print_timeout = runtime_env
            .get("ANTIGRAVITY_PRINT_TIMEOUT")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .unwrap_or(DEFAULT_PRINT_TIMEOUT)
            .to_string();
        let child_timeout_secs = runtime_env
            .get("ANTIGRAVITY_CHILD_TIMEOUT_SECS")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_CHILD_TIMEOUT_SECS);

        Self {
            command,
            workspace,
            model: runtime_env
                .get("ANTIGRAVITY_MODEL")
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(ToString::to_string),
            project: runtime_env
                .get("ANTIGRAVITY_PROJECT")
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(ToString::to_string),
            conversation: runtime_env
                .get("ANTIGRAVITY_CONVERSATION")
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(ToString::to_string),
            print_timeout,
            child_timeout: Duration::from_secs(child_timeout_secs),
            env: runtime_env.clone(),
        }
    }
}

pub fn prompt_text_from_blocks(blocks: &[PromptInputBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            PromptInputBlock::Text { text } => parts.push(text.clone()),
            PromptInputBlock::Image { uri, .. } => {
                let label = uri.as_deref().unwrap_or("inline image");
                parts.push(format!("[image attachment omitted: {label}]"));
            }
            PromptInputBlock::Resource { uri, text, .. } => {
                if let Some(text) = text.as_ref().filter(|t| !t.trim().is_empty()) {
                    parts.push(format!("{uri}\n{text}"));
                } else {
                    parts.push(format!("[resource]({uri})"));
                }
            }
            PromptInputBlock::ResourceLink {
                uri,
                name,
                description,
                ..
            } => {
                if let Some(description) = description.as_ref().filter(|t| !t.trim().is_empty()) {
                    parts.push(format!("[{name}]({uri})\n{description}"));
                } else {
                    parts.push(format!("[{name}]({uri})"));
                }
            }
        }
    }
    parts.join("\n\n")
}

pub fn parse_antigravity_models(stdout: &str) -> Vec<String> {
    let mut models = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if !models.iter().any(|model| model == line) {
            models.push(line.to_string());
        }
    }
    models
}

pub fn fallback_antigravity_models() -> Vec<String> {
    FALLBACK_MODELS
        .iter()
        .map(|model| (*model).to_string())
        .collect()
}

pub async fn query_antigravity_models(
    options: &AntigravityPrintOptions,
) -> Result<Vec<String>, AcpError> {
    let mut cmd = crate::process::tokio_command(&options.command);
    cmd.arg("models")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if options.workspace.is_dir() {
        cmd.current_dir(&options.workspace);
    }
    for (key, value) in &options.env {
        cmd.env(key, value);
    }

    let output = match tokio::time::timeout(
        Duration::from_secs(MODEL_DISCOVERY_TIMEOUT_SECS),
        cmd.output(),
    )
    .await
    {
        Ok(result) => result
            .map_err(|err| AcpError::SpawnFailed(format!("failed to list agy models: {err}")))?,
        Err(_) => {
            return Err(AcpError::protocol(format!(
                "Antigravity CLI model discovery timeout after {MODEL_DISCOVERY_TIMEOUT_SECS} seconds"
            )));
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let detail = if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            format!("exit status {}: {stderr}", output.status)
        };
        return Err(AcpError::protocol(format!(
            "Antigravity CLI model discovery failed with {detail}"
        )));
    }

    let models = parse_antigravity_models(&String::from_utf8_lossy(&output.stdout));
    if models.is_empty() {
        return Err(AcpError::protocol(
            "Antigravity CLI returned an empty model list",
        ));
    }
    Ok(models)
}

pub async fn available_antigravity_models(options: &AntigravityPrintOptions) -> Vec<String> {
    AVAILABLE_MODELS
        .get_or_init(|| async {
            match query_antigravity_models(options).await {
                Ok(models) => models,
                Err(err) => {
                    tracing::warn!(
                        "[ACP][Antigravity] failed to discover models, using fallback list: {err}"
                    );
                    fallback_antigravity_models()
                }
            }
        })
        .await
        .clone()
}

pub async fn run_antigravity_print(
    options: &AntigravityPrintOptions,
    prompt: &str,
) -> Result<String, AcpError> {
    let mut cmd = crate::process::tokio_command(&options.command);
    cmd.arg(format!("--print-timeout={}", options.print_timeout))
        .arg(format!("--add-dir={}", options.workspace.display()));

    if let Some(model) = &options.model {
        cmd.arg(format!("--model={model}"));
    }
    if let Some(project) = &options.project {
        cmd.arg(format!("--project={project}"));
    }
    if let Some(conversation) = &options.conversation {
        cmd.arg(format!("--conversation={conversation}"));
    }
    if options
        .env
        .get("ANTIGRAVITY_DANGEROUSLY_SKIP_PERMISSIONS")
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        cmd.arg("--dangerously-skip-permissions");
    }

    cmd.arg("--print")
        .arg(prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if options.workspace.is_dir() {
        cmd.current_dir(&options.workspace);
    }

    for (key, value) in &options.env {
        cmd.env(key, value);
    }

    let output = match tokio::time::timeout(options.child_timeout, cmd.output()).await {
        Ok(result) => {
            result.map_err(|err| AcpError::SpawnFailed(format!("failed to run agy: {err}")))?
        }
        Err(_) => {
            return Err(AcpError::protocol(format!(
                "Antigravity CLI print timeout after {} seconds",
                options.child_timeout.as_secs()
            )));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let detail = if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            format!("exit status {}: {stderr}", output.status)
        };
        return Err(AcpError::protocol(format!(
            "Antigravity CLI print failed with {detail}"
        )));
    }
    if stdout.is_empty() {
        return Err(AcpError::protocol("Antigravity CLI returned empty output"));
    }
    Ok(stdout)
}
