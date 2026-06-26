//! OfficeCLI integration — detect, install/uninstall the binary, and manage
//! OfficeCLI skills as external experts.
//!
//! Skills are loaded dynamically from the OfficeCLI binary (`officecli
//! load_skill <id>`) and placed in the same central store
//! (`~/.codeg/skills/<id>/`) used by built-in experts. Enabling a skill for
//! an agent reuses the expert system's symlink mechanism.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use tokio::sync::Mutex;

use crate::acp::types::AgentSkillScope;
use crate::commands::acp::{
    preferred_scope_skill_dir, remove_skill_entry, resolve_command_on_path, scoped_skill_dirs,
    skill_storage_spec, validate_skill_id,
};
use crate::commands::experts::{
    central_experts_dir, classify_link, create_link_raw, path_is_symlink, read_link_target,
    ExpertInstallStatus, ExpertLinkState, LinkOp, LinkOpResult,
};
use crate::app_error::AppCommandError;
use crate::commands::folders::resolve_tree_path;
use crate::models::agent::AgentType;
use crate::process::tokio_command;

// ─── Error type ─────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum OfficeToolsError {
    #[error("officecli is not installed")]
    NotInstalled,
    #[error("skill not found: {0}")]
    SkillNotFound(String),
    #[error("agent does not support skills: {0:?}")]
    UnsupportedAgent(AgentType),
    #[error("a real directory already exists at '{path}'")]
    NameCollision { path: String },
    #[error("a different link already exists at '{path}' (points to '{found}')")]
    ForeignLink { path: String, found: String },
    #[error("io error: {0}")]
    Io(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
}

impl Serialize for OfficeToolsError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<io::Error> for OfficeToolsError {
    fn from(err: io::Error) -> Self {
        OfficeToolsError::Io(err.to_string())
    }
}

// ─── Concurrency ───────────────────────────────────────────────────────

fn mutation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ─── Public types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficecliInfo {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficecliSkill {
    pub id: String,
    pub category: String,
    pub icon: String,
    pub sort_order: i32,
    pub display_name: BTreeMap<String, String>,
    pub description: BTreeMap<String, String>,
    pub installed_centrally: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSyncReport {
    pub synced: usize,
    pub errors: Vec<String>,
}

// ─── Skill metadata (hardcoded — OfficeCLI has no list command) ────────

struct SkillDef {
    /// Canonical skill identity used throughout codeg: the central-store
    /// directory name (`~/.codeg/skills/<id>/`) and the agent invocation
    /// name (`/<id>`). Matches the SKILL.md frontmatter `name:` so the
    /// directory and the skill's self-declared name agree.
    id: &'static str,
    /// Argument passed to `officecli load_skill <load_id>`. The CLI uses
    /// short ids (`pptx`, `word`, `excel`) that differ from the invocation
    /// name (`officecli-pptx`, `officecli-docx`, `officecli-xlsx`); for the
    /// morph skills the two happen to coincide.
    load_id: &'static str,
    category: &'static str,
    icon: &'static str,
    sort_order: i32,
    en_name: &'static str,
    en_desc: &'static str,
    zh_name: &'static str,
    zh_desc: &'static str,
}

const SKILLS: &[SkillDef] = &[
    SkillDef {
        id: "officecli-pptx",
        load_id: "pptx",
        category: "presentations",
        icon: "Presentation",
        sort_order: 10,
        en_name: "Presentation",
        en_desc: "Generic presentations — board reviews, sales decks, all-hands",
        zh_name: "演示文稿",
        zh_desc: "通用演示文稿——评审、销售汇报、全员大会",
    },
    SkillDef {
        id: "officecli-pitch-deck",
        load_id: "pitch-deck",
        category: "presentations",
        icon: "Rocket",
        sort_order: 20,
        en_name: "Pitch Deck",
        en_desc: "Fundraising pitch decks — Seed, Series A–C, SAFE, convertible",
        zh_name: "融资路演",
        zh_desc: "融资路演 PPT——种子轮、A–C 轮、SAFE、可转换票据",
    },
    SkillDef {
        id: "morph-ppt",
        load_id: "morph-ppt",
        category: "presentations",
        icon: "Clapperboard",
        sort_order: 30,
        en_name: "Morph Animation PPT",
        en_desc: "Cinematic Morph-animated presentations",
        zh_name: "Morph 动画 PPT",
        zh_desc: "电影级 Morph 过渡动画演示文稿",
    },
    SkillDef {
        id: "morph-ppt-3d",
        load_id: "morph-ppt-3d",
        category: "presentations",
        icon: "Box",
        sort_order: 40,
        en_name: "3D Morph PPT",
        en_desc: "3D Morph with GLB models and camera moves",
        zh_name: "3D Morph PPT",
        zh_desc: "3D Morph + GLB 模型 + 镜头运动",
    },
    SkillDef {
        id: "officecli-docx",
        load_id: "word",
        category: "documents",
        icon: "FileText",
        sort_order: 50,
        en_name: "Word Document",
        en_desc: "Reports, letters, memos, proposals",
        zh_name: "Word 文档",
        zh_desc: "报告、信件、备忘录、提案",
    },
    SkillDef {
        id: "officecli-academic-paper",
        load_id: "academic-paper",
        category: "documents",
        icon: "GraduationCap",
        sort_order: 60,
        en_name: "Academic Paper",
        en_desc: "Journal/thesis with citations, equations, cross-refs",
        zh_name: "学术论文",
        zh_desc: "期刊论文——引用、公式、交叉引用",
    },
    SkillDef {
        id: "officecli-xlsx",
        load_id: "excel",
        category: "spreadsheets",
        icon: "FileSpreadsheet",
        sort_order: 70,
        en_name: "Excel Workbook",
        en_desc: "Generic workbooks, formulas, pivots, trackers",
        zh_name: "Excel 工作簿",
        zh_desc: "通用工作簿——公式、数据透视表、追踪表",
    },
    SkillDef {
        id: "officecli-financial-model",
        load_id: "financial-model",
        category: "spreadsheets",
        icon: "TrendingUp",
        sort_order: 80,
        en_name: "Financial Model",
        en_desc: "3-statement, DCF, LBO, scenarios, projections",
        zh_name: "财务模型",
        zh_desc: "三大报表、DCF、LBO、情景分析、预测",
    },
    SkillDef {
        id: "officecli-data-dashboard",
        load_id: "data-dashboard",
        category: "spreadsheets",
        icon: "BarChart3",
        sort_order: 90,
        en_name: "Data Dashboard",
        en_desc: "CSV/tabular data → KPI/analytics Excel dashboards",
        zh_name: "数据仪表盘",
        zh_desc: "CSV/表格数据 → KPI/分析 Excel 仪表盘",
    },
];

fn skill_defs() -> &'static [SkillDef] {
    SKILLS
}

fn find_skill_def(id: &str) -> Option<&'static SkillDef> {
    skill_defs().iter().find(|s| s.id == id)
}

fn skill_def_to_metadata(def: &SkillDef) -> OfficecliSkill {
    let mut display_name = BTreeMap::new();
    display_name.insert("en".to_string(), def.en_name.to_string());
    display_name.insert("zh-CN".to_string(), def.zh_name.to_string());

    let mut description = BTreeMap::new();
    description.insert("en".to_string(), def.en_desc.to_string());
    description.insert("zh-CN".to_string(), def.zh_desc.to_string());

    let central_path = skill_central_path(def.id);
    OfficecliSkill {
        id: def.id.to_string(),
        category: def.category.to_string(),
        icon: def.icon.to_string(),
        sort_order: def.sort_order,
        display_name,
        description,
        installed_centrally: central_path.exists(),
    }
}

// ─── Path helpers ──────────────────────────────────────────────────────

fn skill_central_path(skill_id: &str) -> PathBuf {
    central_experts_dir().join(skill_id)
}

fn agent_link_path(agent: AgentType, skill_id: &str) -> Result<PathBuf, OfficeToolsError> {
    let dir = preferred_scope_skill_dir(agent, AgentSkillScope::Global, None)
        .map_err(|_| OfficeToolsError::UnsupportedAgent(agent))?;
    Ok(dir.join(skill_id))
}

// ─── Binary detection ──────────────────────────────────────────────────

/// Locations the official OfficeCLI installers drop the binary, in priority
/// order. `install.sh` uses `~/.local/bin/officecli` on Unix; `install.ps1`
/// uses `%LOCALAPPDATA%\OfficeCLI\officecli.exe` on Windows. Used as a fallback
/// when `officecli` isn't (yet) on `PATH`.
fn officecli_known_install_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            paths.push(
                PathBuf::from(local_app_data)
                    .join("OfficeCLI")
                    .join("officecli.exe"),
            );
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".local").join("bin").join("officecli"));
        }
    }
    paths
}

/// The path `officecli_uninstall` removes — the official installer's primary
/// install location for this platform.
fn officecli_primary_install_path() -> Option<PathBuf> {
    officecli_known_install_paths().into_iter().next()
}

pub(crate) fn resolve_officecli() -> Option<PathBuf> {
    if let Some(p) = resolve_command_on_path("officecli") {
        return Some(p);
    }
    // Fall back to the official installers' known locations — covers the window
    // on Windows where `install.ps1`'s persistent User-PATH change hasn't yet
    // reached this already-running process.
    officecli_known_install_paths()
        .into_iter()
        .find(|p| p.is_file())
}

/// Directory to prepend to a spawned agent's `PATH` so agent-invoked
/// `officecli …` (from an enabled office skill) resolves immediately after a
/// fresh install — before `install.ps1`'s persistent User-PATH change reaches
/// already-running processes (codeg and the agents it spawns). Returns `None`
/// once `officecli` is on `PATH` (the injection then self-deactivates) or when
/// it isn't installed. Also closes the latent gap where a GUI-launched codeg on
/// Unix doesn't inherit `~/.local/bin` on `PATH`.
pub(crate) fn officecli_agent_path_dir() -> Option<PathBuf> {
    if resolve_command_on_path("officecli").is_some() {
        return None;
    }
    officecli_known_install_paths()
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

async fn detect_version(binary: &Path) -> Option<String> {
    let output = tokio_command(binary).arg("--version").output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.trim().to_string();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

// ─── Commands: detect ──────────────────────────────────────────────────

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_detect() -> OfficecliInfo {
    match resolve_officecli() {
        Some(path) => {
            let version = detect_version(&path).await;
            OfficecliInfo {
                installed: true,
                version,
                path: Some(path.to_string_lossy().to_string()),
            }
        }
        None => OfficecliInfo {
            installed: false,
            version: None,
            path: None,
        },
    }
}

// ─── Commands: install / uninstall ─────────────────────────────────────

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_install() -> Result<OfficecliInfo, OfficeToolsError> {
    let _guard = mutation_lock().lock().await;

    // Run the vendor's official installer script (mirror-first, GitHub
    // fallback). It owns the download, checksum, install location, and — on
    // Windows — the persistent User-PATH registration. See `officecli_install_command`.
    let cmd = officecli_install_command(current_install_os());
    let output = tokio_command(&cmd.program)
        .args(&cmd.args)
        .output()
        .await
        .map_err(|e| {
            OfficeToolsError::CommandFailed(format!(
                "failed to run the OfficeCLI installer: {e} — install manually from {OFFICECLI_MANUAL_URL}"
            ))
        })?;

    if !output.status.success() {
        // The official scripts report failures on stdout (PowerShell `Write-Host`,
        // bash `echo`) as much as stderr, so prefer stderr but fall back to stdout
        // — otherwise the toast can read just "OfficeCLI install failed:". Bound
        // the tail so a chatty script can't flood the UI.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(OfficeToolsError::CommandFailed(format!(
            "OfficeCLI install failed: {} — install manually from {OFFICECLI_MANUAL_URL}",
            bounded_tail(detail, 800)
        )));
    }

    let info = officecli_detect().await;
    if info.installed {
        Ok(info)
    } else {
        Err(OfficeToolsError::CommandFailed(format!(
            "installation completed but the officecli binary was not found — install manually from {OFFICECLI_MANUAL_URL}"
        )))
    }
}

// ─── Official installer (shell out, mirror-first) ──────────────────────
//
// codeg installs OfficeCLI by running the vendor's official installer script —
// `install.sh` on Unix, `install.ps1` on Windows — mirror-first (the
// CN-reachable `d.officecli.ai`) with a GitHub-raw fallback. This mirrors how
// iOfficeAI's own AionUi backend installs OfficeCLI, keeps both platforms
// symmetric, and never reimplements the download/checksum/PATH logic those
// scripts already own (on Windows `install.ps1` also persists the install dir
// onto the User PATH).

/// Last `max` chars of `s` (char-boundary safe), prefixed with `…` when
/// truncated. Bounds installer diagnostics surfaced to the UI toast.
fn bounded_tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

/// Where users can install OfficeCLI by hand when the network path fails.
const OFFICECLI_MANUAL_URL: &str = "https://github.com/iOfficeAI/OfficeCLI";
const OFFICECLI_INSTALL_SH_MIRROR_URL: &str = "https://d.officecli.ai/install.sh";
const OFFICECLI_INSTALL_SH_GITHUB_URL: &str =
    "https://raw.githubusercontent.com/iOfficeAI/OfficeCLI/main/install.sh";
const OFFICECLI_INSTALL_PS1_MIRROR_URL: &str = "https://d.officecli.ai/install.ps1";
const OFFICECLI_INSTALL_PS1_GITHUB_URL: &str =
    "https://raw.githubusercontent.com/iOfficeAI/OfficeCLI/main/install.ps1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallOs {
    Unix,
    Windows,
}

/// `cfg!(windows)` (not `#[cfg]`) so both variants stay referenced in source —
/// otherwise the unused variant trips dead-code on the platform that omits it.
fn current_install_os() -> InstallOs {
    if cfg!(windows) {
        InstallOs::Windows
    } else {
        InstallOs::Unix
    }
}

struct OfficecliInstallCommand {
    program: String,
    args: Vec<String>,
}

/// Build the installer invocation for `os`. Both branches try the mirror first,
/// then fall back to GitHub raw. Kept platform-parameterized (not `cfg`-gated)
/// so unit tests verify both shapes on any host.
fn officecli_install_command(os: InstallOs) -> OfficecliInstallCommand {
    match os {
        InstallOs::Windows => OfficecliInstallCommand {
            program: "powershell.exe".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-Command".to_string(),
                format!(
                    "$ErrorActionPreference='Stop'; try {{ $s = irm {OFFICECLI_INSTALL_PS1_MIRROR_URL} }} catch {{ $s = irm {OFFICECLI_INSTALL_PS1_GITHUB_URL} }}; iex $s"
                ),
            ],
        },
        InstallOs::Unix => OfficecliInstallCommand {
            program: "bash".to_string(),
            args: vec![
                "-lc".to_string(),
                // Download to a temp file rather than `curl | bash`: a
                // connection dropped mid-stream would otherwise concatenate the
                // fallback output after a partial script.
                format!(
                    "f=$(mktemp) || exit 1; (curl -fsSL {OFFICECLI_INSTALL_SH_MIRROR_URL} -o \"$f\" || curl -fsSL {OFFICECLI_INSTALL_SH_GITHUB_URL} -o \"$f\") && bash \"$f\"; s=$?; rm -f \"$f\"; exit $s"
                ),
            ],
        },
    }
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_uninstall() -> Result<OfficecliInfo, OfficeToolsError> {
    let _guard = mutation_lock().lock().await;

    // Operate on the official installer's primary install location directly,
    // not the PATH-resolved binary. This avoids removing a Homebrew/system
    // binary that happens to shadow it, and ensures we delete the right file.
    let managed_path = officecli_primary_install_path()
        .ok_or_else(|| OfficeToolsError::Io("could not determine install directory".to_string()))?;

    // Remove the binary if it exists.  If it's already gone (e.g. retrying
    // after a partial failure), skip straight to cleanup.
    if managed_path.exists() {
        fs::remove_file(&managed_path).map_err(|e| {
            OfficeToolsError::Io(format!("failed to remove {}: {e}", managed_path.display()))
        })?;
    }

    let mut cleanup_errors: Vec<String> = Vec::new();

    // Remove per-agent symlinks across all scoped skill dirs (not just the
    // preferred dir) so secondary dirs like ~/.agents/skills are also cleaned.
    for agent in supported_agents() {
        let dirs = match scoped_skill_dirs(agent, AgentSkillScope::Global, None) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for def in skill_defs() {
            let central = skill_central_path(def.id);
            for dir in &dirs {
                let candidate = dir.join(def.id);
                if !candidate.exists() && !path_is_symlink(&candidate) {
                    continue;
                }
                let state = classify_link(&candidate, &central);
                let should_remove = match state {
                    ExpertLinkState::LinkedToCodeg => true,
                    ExpertLinkState::Broken => {
                        // Only remove broken links whose target was our
                        // central skill dir (not user-owned danglers).
                        read_link_target(&candidate)
                            .map(|t| t.starts_with(&central))
                            .unwrap_or(false)
                    }
                    _ => false,
                };
                if should_remove {
                    if let Err(e) = remove_skill_entry(&candidate) {
                        cleanup_errors.push(format!(
                            "failed to remove link {}: {e}",
                            candidate.display()
                        ));
                    }
                }
            }
        }
    }

    // Clean up OfficeCLI skills from central store
    for def in skill_defs() {
        let central = skill_central_path(def.id);
        if central.exists() {
            if let Err(e) = fs::remove_dir_all(&central) {
                cleanup_errors.push(format!("failed to remove {}: {e}", central.display()));
            }
        }
    }

    if !cleanup_errors.is_empty() {
        return Err(OfficeToolsError::Io(format!(
            "binary removed but cleanup had errors: {}",
            cleanup_errors.join("; ")
        )));
    }

    // Re-detect so the caller sees the real post-uninstall state (e.g. a
    // system/Homebrew binary may still be on PATH).
    Ok(officecli_detect().await)
}

// ─── Commands: skill listing ───────────────────────────────────────────

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_list_skills() -> Vec<OfficecliSkill> {
    skill_defs().iter().map(skill_def_to_metadata).collect()
}

// ─── Commands: skill sync ──────────────────────────────────────────────

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_sync_skills() -> Result<SkillSyncReport, OfficeToolsError> {
    let _guard = mutation_lock().lock().await;
    let binary = resolve_officecli().ok_or(OfficeToolsError::NotInstalled)?;

    let central_dir = central_experts_dir();
    fs::create_dir_all(&central_dir)?;

    let mut report = SkillSyncReport {
        synced: 0,
        errors: vec![],
    };

    for def in skill_defs() {
        let target_dir = skill_central_path(def.id);
        let skill_md = target_dir.join("SKILL.md");

        // Load skill content from OfficeCLI binary. The CLI keys skills by
        // its own short id (`load_id`), which differs from our invocation
        // name (`def.id`) for the officecli-* skills.
        let output = tokio_command(&binary)
            .arg("load_skill")
            .arg(def.load_id)
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let content = String::from_utf8_lossy(&out.stdout);
                if content.trim().is_empty() {
                    report
                        .errors
                        .push(format!("{}: empty skill content", def.id));
                    continue;
                }
                fs::create_dir_all(&target_dir)?;
                fs::write(&skill_md, content.as_ref())?;
                report.synced += 1;
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                report
                    .errors
                    .push(format!("{}: load_skill failed: {}", def.id, stderr.trim()));
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("{}: command error: {e}", def.id));
            }
        }
    }

    Ok(report)
}

// ─── Commands: skill link / unlink ─────────────────────────────────────

fn supported_agents() -> Vec<AgentType> {
    const ALL: &[AgentType] = &[
        AgentType::ClaudeCode,
        AgentType::Codex,
        AgentType::OpenCode,
        AgentType::Gemini,
        AgentType::OpenClaw,
        AgentType::Cline,
        AgentType::Hermes,
        AgentType::CodeBuddy,
        AgentType::KimiCode,
    ];
    ALL.iter()
        .filter(|a| skill_storage_spec(**a).is_some())
        .copied()
        .collect()
}

/// Link one office skill into one agent's skill dir. **Assumes the mutation
/// lock is already held** — `tokio::sync::Mutex` is not reentrant, so
/// `officecli_skill_apply_links` locks once and calls this directly rather than
/// the public command. The "not synced" guard stays here so a batch enable of
/// an un-synced skill fails only that op.
fn link_one_locked(
    skill_id: &str,
    agent_type: AgentType,
) -> Result<ExpertInstallStatus, OfficeToolsError> {
    let skill_id = validate_skill_id(skill_id).map_err(|e| OfficeToolsError::Io(e.to_string()))?;
    let _ = find_skill_def(&skill_id)
        .ok_or_else(|| OfficeToolsError::SkillNotFound(skill_id.clone()))?;

    let central = skill_central_path(&skill_id);
    if !central.exists() {
        return Err(OfficeToolsError::Io(format!(
            "skill '{skill_id}' is not synced — run sync first"
        )));
    }

    let link_path = agent_link_path(agent_type, &skill_id)?;
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut copy_mode = false;
    match create_link_raw(&central, &link_path) {
        Ok(is_copy) => {
            copy_mode = is_copy;
        }
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            match classify_link(&link_path, &central) {
                ExpertLinkState::LinkedToCodeg => {}
                ExpertLinkState::BlockedByRealDirectory => {
                    return Err(OfficeToolsError::NameCollision {
                        path: link_path.to_string_lossy().to_string(),
                    });
                }
                ExpertLinkState::LinkedElsewhere | ExpertLinkState::Broken => {
                    let found = read_link_target(&link_path)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "<unknown>".into());
                    return Err(OfficeToolsError::ForeignLink {
                        path: link_path.to_string_lossy().to_string(),
                        found,
                    });
                }
                ExpertLinkState::NotLinked => {
                    create_link_raw(&central, &link_path)
                        .map_err(|e| OfficeToolsError::Io(format!("retry link failed: {e}")))?;
                }
            }
        }
        Err(err) => return Err(OfficeToolsError::Io(err.to_string())),
    }

    let state = classify_link(&link_path, &central);
    let target_path = read_link_target(&link_path).map(|p| p.to_string_lossy().to_string());
    Ok(ExpertInstallStatus {
        expert_id: skill_id.clone(),
        agent_type,
        state,
        link_path: link_path.to_string_lossy().to_string(),
        target_path,
        expected_target_path: central.to_string_lossy().to_string(),
        copy_mode,
    })
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_skill_link_to_agent(
    skill_id: String,
    agent_type: AgentType,
) -> Result<ExpertInstallStatus, OfficeToolsError> {
    let _guard = mutation_lock().lock().await;
    link_one_locked(&skill_id, agent_type)
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_skill_unlink_from_agent(
    skill_id: String,
    agent_type: AgentType,
) -> Result<(), OfficeToolsError> {
    let _guard = mutation_lock().lock().await;
    unlink_one_locked(&skill_id, agent_type)
}

/// Remove one office skill's link from one agent's skill dirs. **Assumes the
/// mutation lock is already held** (see `link_one_locked`).
fn unlink_one_locked(skill_id: &str, agent_type: AgentType) -> Result<(), OfficeToolsError> {
    let skill_id = validate_skill_id(skill_id).map_err(|e| OfficeToolsError::Io(e.to_string()))?;
    let _ = find_skill_def(&skill_id)
        .ok_or_else(|| OfficeToolsError::SkillNotFound(skill_id.clone()))?;

    let dirs = scoped_skill_dirs(agent_type, AgentSkillScope::Global, None)
        .map_err(|_| OfficeToolsError::UnsupportedAgent(agent_type))?;

    let central = skill_central_path(&skill_id);
    for dir in dirs {
        let candidate = dir.join(&skill_id);
        if !candidate.exists() && !path_is_symlink(&candidate) {
            continue;
        }
        let state = classify_link(&candidate, &central);
        let should_remove = match state {
            ExpertLinkState::LinkedToCodeg => true,
            ExpertLinkState::Broken => read_link_target(&candidate)
                .map(|t| t.starts_with(&central))
                .unwrap_or(false),
            _ => false,
        };
        if should_remove {
            remove_skill_entry(&candidate).map_err(|e| {
                OfficeToolsError::Io(format!("remove link {}: {e}", candidate.display()))
            })?;
        } else if state == ExpertLinkState::LinkedElsewhere {
            return Err(OfficeToolsError::ForeignLink {
                path: candidate.to_string_lossy().to_string(),
                found: read_link_target(&candidate)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "<unknown>".into()),
            });
        }
    }
    Ok(())
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_skill_get_install_status(
    skill_id: String,
) -> Result<Vec<ExpertInstallStatus>, OfficeToolsError> {
    let skill_id = validate_skill_id(&skill_id).map_err(|e| OfficeToolsError::Io(e.to_string()))?;
    let _ = find_skill_def(&skill_id)
        .ok_or_else(|| OfficeToolsError::SkillNotFound(skill_id.clone()))?;

    let expected = skill_central_path(&skill_id);
    let agents = supported_agents();

    let mut out = Vec::with_capacity(agents.len());
    for agent in agents {
        let link_path = match agent_link_path(agent, &skill_id) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let state = classify_link(&link_path, &expected);
        let target_path = read_link_target(&link_path).map(|p| p.to_string_lossy().to_string());
        out.push(ExpertInstallStatus {
            expert_id: skill_id.clone(),
            agent_type: agent,
            state,
            link_path: link_path.to_string_lossy().to_string(),
            target_path,
            expected_target_path: expected.to_string_lossy().to_string(),
            copy_mode: false,
        });
    }
    Ok(out)
}

/// Apply a batch of enable/disable operations under a single lock acquisition.
/// Mirrors `experts_apply_links`; an un-synced skill simply fails its own op.
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_skill_apply_links(
    ops: Vec<LinkOp>,
) -> Result<Vec<LinkOpResult>, OfficeToolsError> {
    let _guard = mutation_lock().lock().await;
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        // `LinkOp.expert_id` carries the office skill id here — the field name is
        // shared with the experts batch type, and office's ExpertInstallStatus
        // already overloads `expert_id` as the skill id.
        let LinkOp {
            expert_id,
            agent_type,
            enable,
        } = op;
        let res = if enable {
            link_one_locked(&expert_id, agent_type).map(Some)
        } else {
            unlink_one_locked(&expert_id, agent_type).map(|()| None)
        };
        out.push(match res {
            Ok(status) => LinkOpResult {
                expert_id,
                agent_type,
                ok: true,
                status,
                error: None,
            },
            Err(err) => LinkOpResult {
                expert_id,
                agent_type,
                ok: false,
                status: None,
                error: Some(err.to_string()),
            },
        });
    }
    Ok(out)
}

/// One-shot snapshot of every (skill, agent) link state for the matrix UI.
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_skill_list_all_install_statuses(
) -> Result<Vec<ExpertInstallStatus>, OfficeToolsError> {
    let agents = supported_agents();
    let mut out = Vec::with_capacity(skill_defs().len() * agents.len());
    for def in skill_defs() {
        let expected = skill_central_path(def.id);
        for &agent in &agents {
            let link_path = match agent_link_path(agent, def.id) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let state = classify_link(&link_path, &expected);
            let target_path =
                read_link_target(&link_path).map(|p| p.to_string_lossy().to_string());
            out.push(ExpertInstallStatus {
                expert_id: def.id.to_string(),
                agent_type: agent,
                state,
                link_path: link_path.to_string_lossy().to_string(),
                target_path,
                expected_target_path: expected.to_string_lossy().to_string(),
                copy_mode: false,
            });
        }
    }
    Ok(out)
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_skill_read_content(skill_id: String) -> Result<String, OfficeToolsError> {
    let skill_id = validate_skill_id(&skill_id).map_err(|e| OfficeToolsError::Io(e.to_string()))?;
    let _ = find_skill_def(&skill_id)
        .ok_or_else(|| OfficeToolsError::SkillNotFound(skill_id.clone()))?;

    let path = skill_central_path(&skill_id).join("SKILL.md");
    if !path.exists() {
        return Err(OfficeToolsError::Io(format!(
            "skill '{skill_id}' has no SKILL.md — run sync first"
        )));
    }
    let content = fs::read_to_string(&path)?;
    Ok(content)
}

// ─── Commands: office file preview ─────────────────────────────────────

pub(crate) fn is_office_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("docx") | Some("xlsx") | Some("pptx")
    )
}

/// Render an office file (.docx/.xlsx/.pptx) to self-contained HTML via
/// `officecli view <file> html`, for the in-app preview. Runs officecli in
/// codeg's own process (not the agent's command sandbox), so it is unaffected
/// by the sandbox restrictions that can break officecli inside an agent turn.
///
/// `path` is relative to `root_path`; the resolved target is canonicalized and
/// confined to the workspace root, mirroring `read_workspace_file_base64`.
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn officecli_render_html(
    root_path: String,
    path: String,
) -> Result<String, OfficeToolsError> {
    let root = PathBuf::from(&root_path);
    if !root.is_dir() {
        return Err(OfficeToolsError::Io(
            "workspace root does not exist".to_string(),
        ));
    }

    let target =
        resolve_tree_path(&root, &path).map_err(|e| OfficeToolsError::Io(e.to_string()))?;

    // Canonicalize + confine within the workspace root (defense in depth: the
    // path comes from an open file tab, but never render outside the root).
    let canonical_root = fs::canonicalize(&root)?;
    let canonical_target = fs::canonicalize(&target)?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(OfficeToolsError::Io(
            "path is outside workspace root".to_string(),
        ));
    }
    if !canonical_target.is_file() {
        return Err(OfficeToolsError::Io("path is not a file".to_string()));
    }
    if !is_office_path(&canonical_target) {
        return Err(OfficeToolsError::Io(
            "not a supported office file (.docx/.xlsx/.pptx)".to_string(),
        ));
    }

    let binary = resolve_officecli().ok_or(OfficeToolsError::NotInstalled)?;
    let output = tokio_command(&binary)
        .arg("view")
        .arg(&canonical_target)
        .arg("html")
        .output()
        .await
        .map_err(|e| OfficeToolsError::CommandFailed(format!("officecli view failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(OfficeToolsError::CommandFailed(format!(
            "officecli view failed: {}",
            stderr.trim()
        )));
    }

    let html = String::from_utf8_lossy(&output.stdout).to_string();
    if html.trim().is_empty() {
        return Err(OfficeToolsError::CommandFailed(
            "officecli produced empty output".to_string(),
        ));
    }
    Ok(html)
}

// ─── Commands: office live preview (watch) ─────────────────────────────

/// Start (or share, by ref-count) a long-lived `officecli watch` HTTP preview
/// server for an office file and return its loopback port. The live preview is
/// driven by officecli's own SSE refresh, so it no longer races the agent's
/// edits for the file on disk (the bug the one-shot `view html` path caused on
/// Windows). See `crate::office_watch`.
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn start_office_watch(
    root_path: String,
    path: String,
) -> Result<crate::office_watch::OfficeWatchStarted, AppCommandError> {
    crate::office_watch::start_office_watch_core(root_path, path)
        .await
        .map_err(Into::into)
}

/// Release one reference to the watch preview for an office file; kills the
/// server when the last viewer goes away.
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn stop_office_watch(root_path: String, path: String) -> Result<(), AppCommandError> {
    crate::office_watch::stop_office_watch_core(root_path, path)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    // Tests use unknown skill ids so they never touch the developer's real skill
    // directories: office link/unlink both fail at `find_skill_def` for an
    // unknown id, before any filesystem access.

    #[tokio::test]
    async fn apply_links_does_not_deadlock() {
        // Regression guard: `officecli_skill_apply_links` must hold the
        // (non-reentrant) lock once and call the lock-free inner helpers, not the
        // public single commands — otherwise the second op would hang.
        let ops = vec![
            LinkOp {
                expert_id: "zzz-unknown-office-skill-a".into(),
                agent_type: AgentType::ClaudeCode,
                enable: false,
            },
            LinkOp {
                expert_id: "zzz-unknown-office-skill-b".into(),
                agent_type: AgentType::Codex,
                enable: false,
            },
        ];
        let results = timeout(Duration::from_secs(5), officecli_skill_apply_links(ops))
            .await
            .expect("officecli_skill_apply_links must not deadlock")
            .expect("batch returns Ok");
        assert_eq!(results.len(), 2);
        // Unknown skills fail their own op without aborting the batch.
        assert!(results.iter().all(|r| !r.ok && r.error.is_some()), "{results:?}");
    }

    #[tokio::test]
    async fn apply_links_collects_per_op_results_without_aborting() {
        let ops = vec![
            LinkOp {
                expert_id: "zzz-unknown-office-skill".into(),
                agent_type: AgentType::ClaudeCode,
                enable: true,
            },
            LinkOp {
                expert_id: "zzz-unknown-office-skill".into(),
                agent_type: AgentType::Codex,
                enable: false,
            },
        ];
        let results = officecli_skill_apply_links(ops)
            .await
            .expect("batch returns Ok");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| !r.ok));
        assert!(results.iter().all(|r| r.error.is_some()));
        assert!(results.iter().all(|r| r.status.is_none()));
    }

    #[tokio::test]
    async fn list_all_install_statuses_covers_every_skill_agent_pair() {
        let rows = officecli_skill_list_all_install_statuses()
            .await
            .expect("snapshot returns Ok");
        let expected = skill_defs().len() * supported_agents().len();
        assert_eq!(rows.len(), expected);
    }

    #[test]
    fn primary_install_path_is_platform_specific() {
        let p = officecli_primary_install_path().expect("install path resolvable");
        let name = p
            .file_name()
            .expect("has file name")
            .to_string_lossy()
            .to_string();
        #[cfg(windows)]
        {
            assert_eq!(name, "officecli.exe");
            assert!(
                p.components().any(|c| c.as_os_str() == "OfficeCLI"),
                "{p:?}"
            );
        }
        #[cfg(not(windows))]
        {
            assert_eq!(name, "officecli");
            assert!(p.ends_with(".local/bin/officecli"), "{p:?}");
        }
    }

    #[test]
    fn install_command_uses_official_scripts() {
        let unix = officecli_install_command(InstallOs::Unix);
        assert_eq!(unix.program, "bash");
        let unix_script = unix.args.join(" ");
        assert!(unix_script.contains("install.sh"), "{unix_script}");

        let windows = officecli_install_command(InstallOs::Windows);
        assert_eq!(windows.program, "powershell.exe");
        let win_script = windows.args.join(" ");
        assert!(win_script.contains("install.ps1"), "{win_script}");
        // PowerShell fetch-and-run idiom: Invoke-RestMethod | Invoke-Expression.
        assert!(
            win_script.contains("irm") && win_script.contains("iex"),
            "{win_script}"
        );
    }

    #[test]
    fn install_command_tries_mirror_before_github() {
        // The CN-reachable mirror must be attempted before raw.githubusercontent
        // (often unreachable from mainland-China deployments) on both platforms.
        for os in [InstallOs::Unix, InstallOs::Windows] {
            let script = officecli_install_command(os).args.join(" ");
            let mirror = script.find("d.officecli.ai").expect("mirror URL present");
            let github = script
                .find("raw.githubusercontent.com")
                .expect("github URL present");
            assert!(mirror < github, "mirror must precede github for {os:?}: {script}");
        }
    }

    #[test]
    fn bounded_tail_is_char_safe_and_bounded() {
        assert_eq!(bounded_tail("short", 800), "short");

        let long = "x".repeat(1000);
        let tail = bounded_tail(&long, 800);
        assert!(tail.starts_with('…'));
        assert_eq!(tail.chars().filter(|c| *c == 'x').count(), 800);

        // Multibyte input must not panic and must stay on a char boundary.
        let multibyte = "あ".repeat(500); // 1500 bytes
        let tail = bounded_tail(&multibyte, 800);
        assert!(tail.starts_with('…'));
        assert!(tail.chars().skip(1).all(|c| c == 'あ'));
    }
}
