use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use codeg_lib::acp::antigravity_bridge::{
    fallback_antigravity_models, parse_antigravity_models, query_antigravity_models,
    run_antigravity_print, AntigravityPrintOptions,
};

fn fake_agy_script(dir: &Path) -> (PathBuf, PathBuf) {
    let log_path = dir.join("agy-args.log");

    #[cfg(windows)]
    {
        let script_path = dir.join("agy.cmd");
        let log_arg = log_path.to_string_lossy().replace('%', "%%");
        let script = format!(
            r#"@echo off
echo %* > "{log_arg}"
if "%AGY_TEST_MODE%"=="fail" (
  echo bridge failed 1>&2
  exit /b 7
)
if "%AGY_TEST_MODE%"=="empty" (
  exit /b 0
)
if "%AGY_TEST_MODE%"=="sleep" (
  powershell -NoProfile -Command "Start-Sleep -Milliseconds 500"
  echo too late
  exit /b 0
)
if "%AGY_TEST_MODE%"=="models" (
  echo Gemini 3.5 Flash ^(High^)
  echo Gemini 3.1 Pro ^(High^)
  echo Claude Sonnet 4.6 ^(Thinking^)
  exit /b 0
)
echo bridge ok
"#
        );
        fs::write(&script_path, script).expect("write fake agy");
        (script_path, log_path)
    }

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;

        let script_path = dir.join("agy");
        let log_arg = log_path.to_string_lossy();
        let script = format!(
            r#"#!/bin/sh
printf '%s\n' "$*" > "{log_arg}"
case "$AGY_TEST_MODE" in
  fail)
    echo "bridge failed" >&2
    exit 7
    ;;
  empty)
    exit 0
    ;;
  sleep)
    sleep 1
    echo "too late"
    exit 0
    ;;
  models)
    echo "Gemini 3.5 Flash (High)"
    echo "Gemini 3.1 Pro (High)"
    echo "Claude Sonnet 4.6 (Thinking)"
    exit 0
    ;;
esac
echo "bridge ok"
"#
        );
        fs::write(&script_path, script).expect("write fake agy");
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();
        (script_path, log_path)
    }
}

fn options(command: PathBuf, workspace: PathBuf) -> AntigravityPrintOptions {
    AntigravityPrintOptions {
        command,
        workspace,
        model: Some("Gemini 3.1 Pro (High)".to_string()),
        project: Some("proj-1".to_string()),
        conversation: Some("conv-1".to_string()),
        print_timeout: "45s".to_string(),
        child_timeout: Duration::from_secs(2),
        env: BTreeMap::new(),
    }
}

#[test]
fn antigravity_model_parser_trims_blanks_and_deduplicates() {
    let models = parse_antigravity_models(
        "\n Gemini 3.5 Flash (High) \nGemini 3.1 Pro (High)\n\
         Gemini 3.5 Flash (High)\n",
    );
    assert_eq!(
        models,
        vec!["Gemini 3.5 Flash (High)", "Gemini 3.1 Pro (High)"]
    );
}

#[test]
fn antigravity_fallback_models_cover_current_agy_catalog() {
    let models = fallback_antigravity_models();
    assert!(models.contains(&"Gemini 3.5 Flash (High)".to_string()));
    assert!(models.contains(&"Gemini 3.1 Pro (High)".to_string()));
    assert!(models.contains(&"Claude Sonnet 4.6 (Thinking)".to_string()));
    assert!(models.contains(&"GPT-OSS 120B (Medium)".to_string()));
}

#[tokio::test]
async fn antigravity_models_queries_agy_catalog() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (command, log_path) = fake_agy_script(temp.path());
    let mut opts = options(command, temp.path().to_path_buf());
    opts.env.insert("AGY_TEST_MODE".into(), "models".into());

    let models = query_antigravity_models(&opts)
        .await
        .expect("model catalog");

    assert_eq!(
        models,
        vec![
            "Gemini 3.5 Flash (High)",
            "Gemini 3.1 Pro (High)",
            "Claude Sonnet 4.6 (Thinking)",
        ]
    );
    assert_eq!(fs::read_to_string(log_path).unwrap().trim(), "models");
}

#[tokio::test]
async fn antigravity_print_passes_expected_args_and_returns_stdout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (command, log_path) = fake_agy_script(temp.path());
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let output = run_antigravity_print(&options(command, workspace.clone()), "hello")
        .await
        .expect("bridge output");

    assert_eq!(output, "bridge ok");
    let args = fs::read_to_string(log_path).expect("args log");
    assert!(args.contains("--print"));
    assert!(args.contains("--print-timeout"));
    assert!(args.contains("45s"));
    assert!(args.contains("--add-dir"));
    assert!(args.contains(&workspace.to_string_lossy().to_string()));
    assert!(args.contains("--model"));
    assert!(args.contains("Gemini 3.1 Pro (High)"));
    assert!(args.contains("--project"));
    assert!(args.contains("proj-1"));
    assert!(args.contains("--conversation"));
    assert!(args.contains("conv-1"));
    assert!(args.contains("hello"));
    let timeout_idx = args
        .find("--print-timeout")
        .expect("print timeout option is present");
    let print_idx = args.rfind("--print").expect("print flag is present");
    assert!(
        timeout_idx < print_idx,
        "Antigravity parses args after --print as prompt text: {args}"
    );
}

#[tokio::test]
async fn antigravity_print_reports_nonzero_exit_with_stderr() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (command, _) = fake_agy_script(temp.path());
    let mut opts = options(command, temp.path().to_path_buf());
    opts.env.insert("AGY_TEST_MODE".into(), "fail".into());

    let err = run_antigravity_print(&opts, "hello")
        .await
        .expect_err("nonzero exit should fail")
        .to_string();

    assert!(err.contains("bridge failed"), "{err}");
    assert!(err.contains("exit"), "{err}");
}

#[tokio::test(start_paused = true)]
async fn antigravity_print_times_out() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (command, _) = fake_agy_script(temp.path());
    let mut opts = options(command, temp.path().to_path_buf());
    opts.child_timeout = Duration::from_millis(10);
    opts.env.insert("AGY_TEST_MODE".into(), "sleep".into());

    let err = run_antigravity_print(&opts, "hello")
        .await
        .expect_err("timeout should fail")
        .to_string();

    assert!(err.to_ascii_lowercase().contains("timeout"), "{err}");
}

#[tokio::test]
async fn antigravity_print_rejects_empty_stdout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (command, _) = fake_agy_script(temp.path());
    let mut opts = options(command, temp.path().to_path_buf());
    opts.env.insert("AGY_TEST_MODE".into(), "empty".into());

    let err = run_antigravity_print(&opts, "hello")
        .await
        .expect_err("empty stdout should fail")
        .to_string();

    assert!(err.to_ascii_lowercase().contains("empty"), "{err}");
}
