use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State};

const CONFIG_DIR: &str = "FLgit";
const COLLAPSED_SIZE: f64 = 64.0;
const EXPANDED_WIDTH: f64 = 860.0;
const EXPANDED_HEIGHT: f64 = 680.0;
const COLLAPSE_BUTTON_TOP_INSET: i32 = 16;
const COLLAPSE_BUTTON_RIGHT_INSET: i32 = 86;
const FLPDIFF_EXE_NAME: &str = "flpdiff-windows-x64.exe";
const FLPDIFF_BYTES: &[u8] = include_bytes!("../bin/flpdiff-windows-x64.exe");
static GIT_LOG_SESSION: OnceLock<String> = OnceLock::new();

#[derive(Default)]
struct WatchState {
    watcher: Mutex<Option<RecommendedWatcher>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    bound_project_path: Option<PathBuf>,
    repo_root: Option<PathBuf>,
    project_name: Option<String>,
    last_selected_branch: Option<String>,
    overlay_placement: OverlayPlacement,
    overlay_expanded: bool,
    expanded_position: Option<SavedWindowGeometry>,
    flpdiff_path: Option<PathBuf>,
    git_path: Option<PathBuf>,
    git_lfs_path: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            bound_project_path: None,
            repo_root: None,
            project_name: None,
            last_selected_branch: None,
            overlay_placement: OverlayPlacement::default(),
            overlay_expanded: true,
            expanded_position: None,
            flpdiff_path: None,
            git_path: None,
            git_lfs_path: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SavedWindowGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
enum OverlayPlacement {
    Left,
    #[default]
    Right,
    Floating,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PrerequisiteStatus {
    git_available: bool,
    git_lfs_available: bool,
    github_cli_available: bool,
    flpdiff_available: bool,
    git_version: Option<String>,
    git_lfs_version: Option<String>,
    github_cli_version: Option<String>,
    flpdiff_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct BindProjectResult {
    bound_project_path: PathBuf,
    repo_root: PathBuf,
    is_repo: bool,
    project_name: Option<String>,
    flp_files: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct RepoStatus {
    repo_root: PathBuf,
    branch: Option<String>,
    upstream: Option<String>,
    remote_visibility: Option<String>,
    ahead: u32,
    behind: u32,
    has_remote: bool,
    is_repo: bool,
    merge_in_progress: bool,
    rebase_in_progress: bool,
    changes: Vec<FileChange>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FileChange {
    path: String,
    original_path: Option<String>,
    status: String,
    staged: bool,
    unstaged: bool,
    conflicted: bool,
    category: ChangeCategory,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum ChangeCategory {
    Flp,
    Samples,
    Exports,
    Metadata,
    Other,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GitOutput {
    ok: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CommitSummary {
    sha: String,
    short_sha: String,
    subject: String,
    author: String,
    relative_time: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ProjectLock {
    active: bool,
    owner: Option<String>,
    created_at_unix: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FlWindowInfo {
    found: bool,
    title: Option<String>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[tauri::command]
fn get_config() -> Result<AppConfig, String> {
    read_config()
}

#[tauri::command]
fn check_prerequisites(config: Option<AppConfig>) -> PrerequisiteStatus {
    let config = config.unwrap_or_default();
    let git = command_version(config.git_path.as_ref(), "git", &["--version"]);
    let git_lfs = command_version(config.git_lfs_path.as_ref(), "git", &["lfs", "version"]);
    let github_cli = command_version(None, "gh", &["--version"]);
    let flpdiff_path = config.flpdiff_path.clone().or_else(resolve_flpdiff_path);
    let flpdiff = command_version(flpdiff_path.as_ref(), "flpdiff", &["--version"]);

    PrerequisiteStatus {
        git_available: git.is_some(),
        git_lfs_available: git_lfs.is_some(),
        github_cli_available: github_cli.is_some(),
        flpdiff_available: flpdiff.is_some(),
        git_version: git,
        git_lfs_version: git_lfs,
        github_cli_version: github_cli,
        flpdiff_version: flpdiff,
    }
}

#[tauri::command]
fn bind_project(project_path: String) -> Result<BindProjectResult, String> {
    bind_project_path(PathBuf::from(project_path))
}

#[tauri::command]
fn clone_project(
    remote_url: String,
    destination_path: String,
) -> Result<BindProjectResult, String> {
    let remote_url = validate_clone_remote_url(&remote_url)?;
    let destination = normalize_path(PathBuf::from(destination_path))?;
    clone_remote_into(&remote_url, &destination)?;
    bind_project_path(destination)
}

fn clone_remote_into(remote_url: &str, destination: &Path) -> Result<(), String> {
    validate_clone_destination(&destination)?;

    let parent = destination
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if !parent.exists() {
        fs::create_dir_all(&parent).map_err(|err| err.to_string())?;
    }

    let destination_arg = destination.to_string_lossy().to_string();
    let clone_args = ["clone", remote_url, destination_arg.as_str()];
    let clone = run_command("git", Some(&parent), &clone_args);
    log_clone_git_action(destination, &parent, &clone_args, &clone);
    let clone = clone?;
    if !clone.ok {
        return Err(trim_command_error(&clone, "Git clone failed"));
    }

    let lfs_install = run_git(&destination, &["lfs", "install", "--local"])?;
    if !lfs_install.ok {
        return Err(trim_command_error(
            &lfs_install,
            "Git LFS local setup failed",
        ));
    }
    let lfs_pull = run_git(&destination, &["lfs", "pull"])?;
    if !lfs_pull.ok {
        return Err(trim_command_error(&lfs_pull, "Git LFS pull failed"));
    }

    configure_local_flpdiff_driver(&destination)?;
    Ok(())
}

#[tauri::command]
fn repo_status(repo_root: String) -> Result<RepoStatus, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    if !repo_root.join(".git").exists() {
        return Ok(RepoStatus {
            repo_root,
            is_repo: false,
            ..RepoStatus::default()
        });
    }

    let output = run_git(
        &repo_root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "-b",
            "--untracked-files=all",
        ],
    )?;
    let mut status = parse_status(&repo_root, &output.stdout);
    status.remote_visibility = detect_remote_visibility(&repo_root);
    status.merge_in_progress = repo_root.join(".git").join("MERGE_HEAD").exists();
    status.rebase_in_progress = repo_root.join(".git").join("rebase-merge").exists()
        || repo_root.join(".git").join("rebase-apply").exists();
    Ok(status)
}

#[tauri::command]
fn initialize_project_repo(
    repo_root: String,
    project_name: String,
    default_branch: Option<String>,
) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let project_name = sanitize_project_name(&project_name)?;
    let default_branch = default_branch
        .filter(|branch| !branch.trim().is_empty())
        .unwrap_or_else(|| "main".to_string());

    if !repo_root.exists() {
        fs::create_dir_all(&repo_root).map_err(|err| err.to_string())?;
    }

    let mut combined = String::new();
    if !repo_root.join(".git").exists() {
        let init = run_git(&repo_root, &["init", "--initial-branch", &default_branch])?;
        combined.push_str(&init.stdout);
        combined.push_str(&init.stderr);
        if !init.ok {
            return Ok(init);
        }
    } else {
        let branch = run_git(&repo_root, &["branch", "-M", &default_branch])?;
        combined.push_str(&branch.stdout);
        combined.push_str(&branch.stderr);
        if !branch.ok {
            return Ok(branch);
        }
    }

    update_gitignore(&repo_root)?;
    update_gitattributes(&repo_root)?;
    write_project_metadata(&repo_root, &project_name)?;

    let lfs = run_git(&repo_root, &["lfs", "install", "--local"])?;
    combined.push_str(&lfs.stdout);
    combined.push_str(&lfs.stderr);

    let exe = resolve_flpdiff_path()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "flpdiff".to_string());
    let output = hidden_command(&exe)
        .current_dir(&repo_root)
        .args(["git-setup", "--lfs"])
        .output()
        .map_err(|err| format!("Failed to run flpdiff setup using '{}': {}", exe, err))?;
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    combined.push_str(&String::from_utf8_lossy(&output.stderr));

    let mut config = read_config().unwrap_or_default();
    config.repo_root = Some(repo_root);
    config.project_name = Some(project_name);
    write_config(&config)?;

    Ok(GitOutput {
        ok: output.status.success() && lfs.ok,
        stdout: combined,
        stderr: String::new(),
    })
}

#[tauri::command]
fn setup_git_lfs_flpdiff(
    repo_root: String,
    flpdiff_path: Option<String>,
) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    if !repo_root.join(".git").exists() {
        run_git(&repo_root, &["init"])?;
    }

    let mut combined = String::new();
    combined.push_str(&run_git(&repo_root, &["lfs", "install", "--local"])?.stdout);
    update_gitignore(&repo_root)?;
    update_gitattributes(&repo_root)?;

    let exe = flpdiff_path
        .map(PathBuf::from)
        .or_else(resolve_flpdiff_path)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "flpdiff".to_string());
    match hidden_command(&exe)
        .current_dir(&repo_root)
        .args(["git-setup", "--lfs"])
        .output()
    {
        Ok(output) => Ok(GitOutput {
            ok: output.status.success(),
            stdout: format!("{}{}", combined, String::from_utf8_lossy(&output.stdout)),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }),
        Err(err) => Err(format!(
            "Failed to run flpdiff setup using '{}': {}",
            exe, err
        )),
    }
}

#[tauri::command]
fn stage_paths(repo_root: String, paths: Vec<String>) -> Result<GitOutput, String> {
    run_git_dynamic(&repo_root, "add", paths, true)
}

#[tauri::command]
fn unstage_paths(repo_root: String, paths: Vec<String>) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let args = unstage_args(repo_has_commits(&repo_root)?, paths);
    run_git_owned(&repo_root, &args)
}

#[tauri::command]
fn commit(repo_root: String, message: String) -> Result<GitOutput, String> {
    if message.trim().is_empty() {
        return Err("Commit message is required".to_string());
    }
    run_git(
        &normalize_path(PathBuf::from(repo_root))?,
        &["commit", "-m", message.trim()],
    )
}

#[tauri::command]
fn pull(repo_root: String) -> Result<GitOutput, String> {
    run_git(
        &normalize_path(PathBuf::from(repo_root))?,
        &["pull", "--ff-only"],
    )
}

#[tauri::command]
fn push(repo_root: String) -> Result<GitOutput, String> {
    run_git(&normalize_path(PathBuf::from(repo_root))?, &["push"])
}

#[tauri::command]
fn set_remote(repo_root: String, remote_url: String) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let remote_url = remote_url.trim();
    if remote_url.is_empty() {
        return Err("Remote URL is required".to_string());
    }

    let existing = run_git(&repo_root, &["remote", "get-url", "origin"])?;
    if existing.ok {
        run_git(&repo_root, &["remote", "set-url", "origin", remote_url])
    } else {
        run_git(&repo_root, &["remote", "add", "origin", remote_url])
    }
}

#[tauri::command]
fn diff_path(repo_root: String, path: String, staged: bool) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    if staged {
        run_git(&repo_root, &["diff", "--cached", "--", &path])
    } else {
        run_git(&repo_root, &["diff", "--", &path])
    }
}

#[tauri::command]
fn show_commit(repo_root: String, revision: String) -> Result<GitOutput, String> {
    run_git(
        &normalize_path(PathBuf::from(repo_root))?,
        &["show", "--stat", "--patch", &revision],
    )
}

#[tauri::command]
fn list_commits(repo_root: String, limit: Option<u32>) -> Result<Vec<CommitSummary>, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let limit_arg = format!("-n{}", limit.unwrap_or(40).clamp(1, 200));
    let output = run_git(
        &repo_root,
        &[
            "log",
            &limit_arg,
            "--date=relative",
            "--pretty=format:%H%x1f%h%x1f%s%x1f%an%x1f%cr",
        ],
    )?;
    if !output.ok {
        return Err(output.stderr);
    }
    Ok(parse_commit_log(&output.stdout))
}

#[tauri::command]
fn diff_revision_path(
    repo_root: String,
    revision: String,
    path: String,
) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let parent = format!("{}^", revision);
    let parent_check = run_git(&repo_root, &["rev-parse", "--verify", &parent])?;
    if !parent_check.ok {
        return Ok(GitOutput {
            ok: true,
            stdout: "This is the first commit".to_string(),
            stderr: String::new(),
        });
    }
    run_git(
        &repo_root,
        &["diff", &format!("{}~1", revision), &revision, "--", &path],
    )
}

#[tauri::command]
fn diff_commits(
    repo_root: String,
    base_revision: String,
    head_revision: String,
) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let mut summary_args = vec![
        "diff".to_string(),
        "--stat".to_string(),
        "--summary".to_string(),
        base_revision.clone(),
        head_revision.clone(),
    ];
    summary_args.extend(compare_surface_pathspecs());
    let summary = run_git_owned(&repo_root, &summary_args)?;

    let mut patch_args = vec![
        "diff".to_string(),
        base_revision.clone(),
        head_revision.clone(),
    ];
    patch_args.extend(compare_surface_pathspecs());
    let patch = run_git_owned(&repo_root, &patch_args)?;

    let flp_paths = run_git_owned(
        &repo_root,
        &[
            "diff".to_string(),
            "--name-only".to_string(),
            base_revision.clone(),
            head_revision.clone(),
            "--".to_string(),
            "*.flp".to_string(),
            "*.FLP".to_string(),
        ],
    )?;

    let mut stdout = String::new();
    stdout.push_str(&format!(
        "Comparing {}..{}\n\n",
        base_revision.chars().take(12).collect::<String>(),
        head_revision.chars().take(12).collect::<String>()
    ));

    let surface = [summary.stdout.trim(), patch.stdout.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if surface.is_empty() {
        stdout.push_str("No surface file changes were returned by Git.\n");
    } else {
        stdout.push_str("Git file diff:\n");
        stdout.push_str(&surface);
        stdout.push_str("\n");
    }

    let semantic = semantic_flp_diff_for_revisions(
        &repo_root,
        &base_revision,
        &head_revision,
        &flp_paths.stdout,
    )?;
    if !semantic.trim().is_empty() {
        stdout.push_str("\n");
        stdout.push_str(&semantic);
    }

    Ok(GitOutput {
        ok: summary.ok && patch.ok && flp_paths.ok,
        stdout,
        stderr: [summary.stderr, patch.stderr, flp_paths.stderr]
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    })
}

#[tauri::command]
fn diff_working_tree_against_revision(
    repo_root: String,
    revision: String,
    path: String,
) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    run_git(&repo_root, &["diff", &revision, "--", &path])
}

#[tauri::command]
fn reset_to_commit(repo_root: String, revision: String) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    run_git(&repo_root, &["reset", "--hard", &revision])
}

#[tauri::command]
fn github_publish_repo(
    repo_root: String,
    repo_name: String,
    visibility: Option<String>,
) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let repo_name = sanitize_project_name(&repo_name)?;
    update_gitignore(&repo_root)?;
    update_gitattributes(&repo_root)?;
    let snapshot = commit_project_snapshot_if_needed(&repo_root)?;
    if !snapshot.ok {
        return Ok(snapshot);
    }
    let visibility = visibility.unwrap_or_else(|| "private".to_string());
    let visibility_arg = match visibility.as_str() {
        "public" => "--public",
        "internal" => "--internal",
        _ => "--private",
    };

    let status = hidden_command("gh")
        .current_dir(&repo_root)
        .args([
            "repo",
            "create",
            &repo_name,
            "--source",
            ".",
            "--remote",
            "origin",
            "--push",
            visibility_arg,
        ])
        .output()
        .map_err(|err| {
            format!(
                "Failed to run GitHub CLI. Install gh and authenticate with `gh auth login`: {}",
                err
            )
        })?;

    Ok(GitOutput {
        ok: status.status.success(),
        stdout: format!(
            "{}{}",
            snapshot.stdout,
            String::from_utf8_lossy(&status.stdout)
        ),
        stderr: String::from_utf8_lossy(&status.stderr).to_string(),
    })
}

#[tauri::command]
fn start_project_watch(
    app: AppHandle,
    state: State<'_, WatchState>,
    repo_root: String,
) -> Result<(), String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    if !repo_root.exists() {
        return Err("Project path does not exist".to_string());
    }

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = tx.send(event);
        },
        Config::default(),
    )
    .map_err(|err| err.to_string())?;
    watcher
        .watch(&repo_root, RecursiveMode::Recursive)
        .map_err(|err| err.to_string())?;

    let app_for_thread = app.clone();
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if let Ok(event) = event {
                if event
                    .paths
                    .iter()
                    .any(|path| should_emit_project_change(path))
                {
                    let _ = app_for_thread.emit("project-changed", ());
                }
            }
        }
    });

    let mut guard = state
        .watcher
        .lock()
        .map_err(|_| "Watcher state lock poisoned".to_string())?;
    *guard = Some(watcher);
    Ok(())
}

#[tauri::command]
fn stop_project_watch(state: State<'_, WatchState>) -> Result<(), String> {
    let mut guard = state
        .watcher
        .lock()
        .map_err(|_| "Watcher state lock poisoned".to_string())?;
    *guard = None;
    Ok(())
}

#[tauri::command]
fn resolve_conflict_path(
    repo_root: String,
    path: String,
    choice: String,
) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let checkout_arg = match choice.as_str() {
        "local" => "--ours",
        "remote" => "--theirs",
        _ => return Err("Conflict choice must be local or remote".to_string()),
    };
    let checkout = run_git(&repo_root, &["checkout", checkout_arg, "--", &path])?;
    if !checkout.ok {
        return Ok(checkout);
    }
    let add = run_git(&repo_root, &["add", "--", &path])?;
    Ok(GitOutput {
        ok: add.ok,
        stdout: format!("{}{}", checkout.stdout, add.stdout),
        stderr: format!("{}{}", checkout.stderr, add.stderr),
    })
}

#[tauri::command]
fn abort_merge_or_rebase(repo_root: String) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    if repo_root.join(".git").join("rebase-merge").exists()
        || repo_root.join(".git").join("rebase-apply").exists()
    {
        return run_git(&repo_root, &["rebase", "--abort"]);
    }
    run_git(&repo_root, &["merge", "--abort"])
}

#[tauri::command]
fn read_project_lock(repo_root: String) -> Result<ProjectLock, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    Ok(read_lock_file(&repo_root).unwrap_or_default())
}

#[tauri::command]
fn acquire_project_lock(repo_root: String) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let owner = git_user_name(&repo_root).unwrap_or_else(|| "Unknown collaborator".to_string());
    let created_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_secs();
    write_lock_file(
        &repo_root,
        &ProjectLock {
            active: true,
            owner: Some(owner),
            created_at_unix: Some(created_at_unix),
        },
    )?;
    commit_lock_change(&repo_root, "Lock FL Studio project for editing")
}

#[tauri::command]
fn release_project_lock(repo_root: String) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let path = repo_root.join(".flgit").join("lock.json");
    if path.exists() {
        fs::remove_file(path).map_err(|err| err.to_string())?;
    }
    commit_lock_change(&repo_root, "Release FL Studio project lock")
}

#[tauri::command]
fn flpdiff_info(file_path: String, flpdiff_path: Option<String>) -> Result<GitOutput, String> {
    let file_path = normalize_path(PathBuf::from(file_path))?;
    let exe = flpdiff_path
        .map(PathBuf::from)
        .or_else(resolve_flpdiff_path)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "flpdiff".to_string());
    run_command(
        &exe,
        None,
        &[
            "info",
            file_path.to_string_lossy().as_ref(),
            "--format",
            "json",
        ],
    )
}

#[tauri::command]
fn detect_fl_studio_window() -> FlWindowInfo {
    detect_fl_window()
}

#[tauri::command]
fn anchor_to_fl_studio(
    app: AppHandle,
    placement: OverlayPlacement,
) -> Result<FlWindowInfo, String> {
    let fl = detect_fl_window();
    if !fl.found {
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_always_on_top(false)
                .map_err(|err| err.to_string())?;
            window.show().map_err(|err| err.to_string())?;
            window.set_focus().map_err(|err| err.to_string())?;
        }
        return Ok(fl);
    }

    if let Some(window) = app.get_webview_window("main") {
        window
            .set_always_on_top(true)
            .map_err(|err| err.to_string())?;
        window.show().map_err(|err| err.to_string())?;
        let width = 380.0;
        let height = f64::from(fl.height.max(520));
        let x = match placement {
            OverlayPlacement::Left => f64::from(fl.x),
            OverlayPlacement::Right => f64::from(fl.x + fl.width - width as i32),
            OverlayPlacement::Floating => f64::from(fl.x + fl.width - width as i32 - 24),
        };
        let y = f64::from(fl.y);
        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }))
            .map_err(|err| err.to_string())?;
        window
            .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .map_err(|err| err.to_string())?;
    }

    let mut config = read_config().unwrap_or_default();
    config.overlay_placement = placement;
    write_config(&config)?;
    Ok(fl)
}

#[tauri::command]
fn set_overlay_expanded(app: AppHandle, expanded: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    let fl = detect_fl_window();

    if expanded {
        let config = read_config().unwrap_or_default();
        if let Some(saved) = config.expanded_position {
            let current_position = window.outer_position().ok();
            let x = current_position
                .map(|position| position.x - saved.width as i32 + COLLAPSE_BUTTON_RIGHT_INSET)
                .unwrap_or(saved.x);
            let y = current_position
                .map(|position| position.y - COLLAPSE_BUTTON_TOP_INSET)
                .unwrap_or(saved.y);
            window.set_resizable(true).map_err(|err| err.to_string())?;
            window
                .set_size(tauri::Size::Physical(PhysicalSize {
                    width: saved.width,
                    height: saved.height,
                }))
                .map_err(|err| err.to_string())?;
            window
                .set_position(tauri::Position::Physical(PhysicalPosition { x, y }))
                .map_err(|err| err.to_string())?;
        } else {
            let width = if fl.found {
                EXPANDED_WIDTH.min(f64::from(fl.width) * 0.78)
            } else {
                EXPANDED_WIDTH
            };
            let height = if fl.found {
                EXPANDED_HEIGHT.min(f64::from(fl.height) * 0.82)
            } else {
                EXPANDED_HEIGHT
            };
            let x = if fl.found {
                f64::from(fl.x) + (f64::from(fl.width) - width) / 2.0
            } else {
                220.0
            };
            let y = if fl.found {
                f64::from(fl.y) + (f64::from(fl.height) - height) / 2.0
            } else {
                120.0
            };
            window
                .set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }))
                .map_err(|err| err.to_string())?;
            if fl.found {
                window
                    .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
                    .map_err(|err| err.to_string())?;
            } else {
                window.center().map_err(|err| err.to_string())?;
            }
        }
        window.set_resizable(true).map_err(|err| err.to_string())?;
    } else {
        let mut config = read_config().unwrap_or_default();
        let mut collapsed_position: Option<PhysicalPosition<i32>> = None;
        if let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) {
            if size.width > COLLAPSED_SIZE as u32 + 10 && size.height > COLLAPSED_SIZE as u32 + 10 {
                config.expanded_position = Some(SavedWindowGeometry {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                });
                collapsed_position = Some(PhysicalPosition {
                    x: position.x + size.width as i32 - COLLAPSE_BUTTON_RIGHT_INSET,
                    y: position.y + COLLAPSE_BUTTON_TOP_INSET,
                });
            }
        }
        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: COLLAPSED_SIZE,
                height: COLLAPSED_SIZE,
            }))
            .map_err(|err| err.to_string())?;
        if let Some(position) = collapsed_position {
            window
                .set_position(tauri::Position::Physical(position))
                .map_err(|err| err.to_string())?;
        } else if fl.found {
            window
                .set_position(tauri::Position::Logical(tauri::LogicalPosition {
                    x: f64::from(fl.x + fl.width - 92),
                    y: f64::from(fl.y + 48),
                }))
                .map_err(|err| err.to_string())?;
        } else {
            window
                .set_position(tauri::Position::Logical(tauri::LogicalPosition {
                    x: 1220.0,
                    y: 72.0,
                }))
                .map_err(|err| err.to_string())?;
        }
        window.set_resizable(false).map_err(|err| err.to_string())?;
        write_config(&config)?;
    }

    window
        .set_always_on_top(fl.found)
        .map_err(|err| err.to_string())?;
    let mut config = read_config().unwrap_or_default();
    config.overlay_expanded = expanded;
    write_config(&config)?;
    Ok(())
}

#[tauri::command]
fn start_overlay_drag(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    window.start_dragging().map_err(|err| err.to_string())
}

#[tauri::command]
fn undock_overlay(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    window
        .set_always_on_top(false)
        .map_err(|err| err.to_string())?;
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: EXPANDED_WIDTH,
            height: EXPANDED_HEIGHT,
        }))
        .map_err(|err| err.to_string())?;
    window
        .set_position(tauri::Position::Logical(tauri::LogicalPosition {
            x: 220.0,
            y: 120.0,
        }))
        .map_err(|err| err.to_string())?;
    window.show().map_err(|err| err.to_string())?;
    window.set_focus().map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
fn close_window(app: AppHandle) -> Result<(), String> {
    let mut config = read_config().unwrap_or_default();
    reset_project_state_for_close(&mut config);
    write_config(&config)?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    window.close().map_err(|err| err.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .manage(WatchState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_config,
            check_prerequisites,
            bind_project,
            clone_project,
            initialize_project_repo,
            repo_status,
            setup_git_lfs_flpdiff,
            stage_paths,
            unstage_paths,
            commit,
            pull,
            push,
            set_remote,
            diff_path,
            show_commit,
            list_commits,
            diff_revision_path,
            diff_commits,
            diff_working_tree_against_revision,
            reset_to_commit,
            github_publish_repo,
            start_project_watch,
            stop_project_watch,
            resolve_conflict_path,
            abort_merge_or_rebase,
            read_project_lock,
            acquire_project_lock,
            release_project_lock,
            flpdiff_info,
            detect_fl_studio_window,
            anchor_to_fl_studio,
            set_overlay_expanded,
            start_overlay_drag,
            undock_overlay,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn config_path() -> Result<PathBuf, String> {
    let base =
        dirs::config_dir().ok_or_else(|| "Could not find user config directory".to_string())?;
    Ok(base.join(CONFIG_DIR).join("config.json"))
}

fn read_config() -> Result<AppConfig, String> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let bytes = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&bytes).map_err(|err| err.to_string())
}

fn write_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = serde_json::to_string_pretty(config).map_err(|err| err.to_string())?;
    fs::write(path, bytes).map_err(|err| err.to_string())
}

fn sanitize_project_name(project_name: &str) -> Result<String, String> {
    let trimmed = project_name.trim();
    if trimmed.is_empty() {
        return Err("Project name is required".to_string());
    }
    if trimmed
        .chars()
        .any(|ch| matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err("Project name cannot contain Windows path separator characters".to_string());
    }
    Ok(trimmed.to_string())
}

fn reset_project_state_for_close(config: &mut AppConfig) {
    config.bound_project_path = None;
    config.repo_root = None;
    config.project_name = None;
    config.last_selected_branch = None;
    config.expanded_position = None;
    config.overlay_expanded = true;
}

fn read_project_name(repo_root: &Path) -> Option<String> {
    let path = repo_root.join(".flgit").join("project.json");
    let bytes = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&bytes).ok()?;
    value
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn bind_project_path(project_path: PathBuf) -> Result<BindProjectResult, String> {
    let bound_path = normalize_path(project_path)?;
    let repo_candidate = if bound_path.is_file() {
        bound_path
            .parent()
            .ok_or_else(|| "Selected file has no parent directory".to_string())?
            .to_path_buf()
    } else {
        bound_path.clone()
    };
    let repo_root = existing_repo_root(&repo_candidate).unwrap_or_else(|| repo_candidate.clone());
    let is_repo = repo_root.join(".git").exists();
    let flp_files = list_flp_files(&repo_candidate)?;
    let project_name = read_project_name(&repo_root).or_else(|| {
        repo_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
    });

    let mut config = read_config().unwrap_or_default();
    config.bound_project_path = Some(bound_path.clone());
    config.repo_root = Some(repo_root.clone());
    config.project_name = project_name.clone();
    write_config(&config)?;

    Ok(BindProjectResult {
        bound_project_path: bound_path,
        repo_root,
        is_repo,
        project_name,
        flp_files,
    })
}

fn validate_clone_remote_url(remote_url: &str) -> Result<String, String> {
    let trimmed = remote_url.trim();
    if trimmed.is_empty() {
        return Err("Remote URL is required".to_string());
    }
    if trimmed.contains('\0') {
        return Err("Remote URL contains an invalid character".to_string());
    }

    let lower = trimmed.to_ascii_lowercase();
    let is_local_path = Path::new(trimmed).exists();
    let valid = lower.starts_with("https://github.com/")
        || lower.starts_with("http://github.com/")
        || lower.starts_with("ssh://")
        || lower.starts_with("git@github.com:")
        || lower.starts_with("file://")
        || is_local_path;
    if !valid {
        return Err("Enter a GitHub HTTPS or SSH remote URL".to_string());
    }
    if !is_local_path && trimmed.contains(char::is_whitespace) {
        return Err("Remote URL cannot contain spaces".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_clone_destination(destination: &Path) -> Result<(), String> {
    if destination.join(".git").exists() {
        return Err("Destination already contains a Git repo".to_string());
    }
    if destination.exists() {
        if !destination.is_dir() {
            return Err("Clone destination must be a folder".to_string());
        }
        let mut entries = fs::read_dir(destination).map_err(|err| err.to_string())?;
        if entries
            .next()
            .transpose()
            .map_err(|err| err.to_string())?
            .is_some()
        {
            return Err("Clone destination must be empty".to_string());
        }
    }
    Ok(())
}

fn configure_local_flpdiff_driver(repo_root: &Path) -> Result<(), String> {
    let exe = resolve_flpdiff_path()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "flpdiff".to_string());
    let config = run_git(repo_root, &["config", "--local", "diff.flp.command", &exe])?;
    if !config.ok {
        return Err(trim_command_error(
            &config,
            "flpdiff local Git config failed",
        ));
    }

    let info_dir = repo_root.join(".git").join("info");
    fs::create_dir_all(&info_dir).map_err(|err| err.to_string())?;
    let attributes_path = info_dir.join("attributes");
    let existing = if attributes_path.exists() {
        fs::read_to_string(&attributes_path).map_err(|err| err.to_string())?
    } else {
        String::new()
    };
    let mut lines = existing
        .lines()
        .map(str::to_string)
        .collect::<BTreeSet<String>>();
    lines.insert("*.flp diff=flp".to_string());
    lines.insert("*.FLP diff=flp".to_string());
    let mut output = lines.into_iter().collect::<Vec<_>>().join("\n");
    output.push('\n');
    fs::write(attributes_path, output).map_err(|err| err.to_string())
}

fn trim_command_error(output: &GitOutput, fallback: &str) -> String {
    let message = if output.stderr.trim().is_empty() {
        output.stdout.trim()
    } else {
        output.stderr.trim()
    };
    if message.is_empty() {
        fallback.to_string()
    } else {
        message.to_string()
    }
}

fn write_project_metadata(repo_root: &Path, project_name: &str) -> Result<(), String> {
    let dir = repo_root.join(".flgit");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let metadata = serde_json::json!({
        "name": project_name,
        "formatVersion": 1,
        "ignoredLocalFolders": ["Backup"],
        "trackedAssetFolders": ["Audio"]
    });
    fs::write(
        dir.join("project.json"),
        serde_json::to_string_pretty(&metadata).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn read_lock_file(repo_root: &Path) -> Option<ProjectLock> {
    let path = repo_root.join(".flgit").join("lock.json");
    let bytes = fs::read_to_string(path).ok()?;
    serde_json::from_str(&bytes).ok()
}

fn write_lock_file(repo_root: &Path, lock: &ProjectLock) -> Result<(), String> {
    let dir = repo_root.join(".flgit");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    fs::write(
        dir.join("lock.json"),
        serde_json::to_string_pretty(lock).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn commit_lock_change(repo_root: &Path, message: &str) -> Result<GitOutput, String> {
    let add = run_git(repo_root, &["add", "--", ".flgit/lock.json"])?;
    let commit = run_git(repo_root, &["commit", "-m", message])?;
    let push = if commit.ok {
        run_git(repo_root, &["push"]).unwrap_or(GitOutput {
            ok: false,
            stdout: String::new(),
            stderr: "Push failed or no upstream is configured".to_string(),
        })
    } else {
        GitOutput {
            ok: true,
            stdout: String::new(),
            stderr: String::new(),
        }
    };
    Ok(GitOutput {
        ok: add.ok && commit.ok && push.ok,
        stdout: format!("{}{}{}", add.stdout, commit.stdout, push.stdout),
        stderr: format!("{}{}{}", add.stderr, commit.stderr, push.stderr),
    })
}

fn commit_project_snapshot_if_needed(repo_root: &Path) -> Result<GitOutput, String> {
    let add = run_git(repo_root, &["add", "--", "."])?;
    if !add.ok {
        return Ok(add);
    }
    let diff = run_git(repo_root, &["diff", "--cached", "--quiet"])?;
    if diff.ok {
        return Ok(GitOutput {
            ok: true,
            stdout: add.stdout,
            stderr: add.stderr,
        });
    }
    let commit = run_git(
        repo_root,
        &["commit", "-m", "Initial FLgit project snapshot"],
    )?;
    Ok(GitOutput {
        ok: commit.ok,
        stdout: format!("{}{}", add.stdout, commit.stdout),
        stderr: format!("{}{}", add.stderr, commit.stderr),
    })
}

fn git_user_name(repo_root: &Path) -> Option<String> {
    let output = run_git(repo_root, &["config", "user.name"]).ok()?;
    if !output.ok {
        return None;
    }
    let name = output.stdout.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn repo_has_commits(repo_root: &Path) -> Result<bool, String> {
    let output = run_git(repo_root, &["rev-parse", "--verify", "HEAD"])?;
    Ok(output.ok)
}

fn unstage_args(has_commits: bool, paths: Vec<String>) -> Vec<String> {
    let mut args = if has_commits {
        vec![
            "restore".to_string(),
            "--staged".to_string(),
            "--".to_string(),
        ]
    } else {
        vec![
            "rm".to_string(),
            "-r".to_string(),
            "--cached".to_string(),
            "--".to_string(),
        ]
    };
    args.extend(paths);
    args
}

fn detect_remote_visibility(repo_root: &Path) -> Option<String> {
    let remotes = run_git(repo_root, &["remote"]).ok()?;
    if !remotes.ok
        || !remotes
            .stdout
            .lines()
            .any(|remote| remote.trim() == "origin")
    {
        return None;
    }

    let visibility = hidden_command("gh")
        .current_dir(repo_root)
        .args(["repo", "view", "--json", "visibility", "-q", ".visibility"])
        .output()
        .ok()?;
    if !visibility.status.success() {
        return Some("remote".to_string());
    }
    let value = String::from_utf8_lossy(&visibility.stdout)
        .trim()
        .to_ascii_lowercase();
    if value.is_empty() {
        Some("remote".to_string())
    } else {
        Some(value)
    }
}

fn should_emit_project_change(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.contains("/Backup/") || normalized.ends_with("/Backup") {
        return false;
    }
    if normalized.contains("/.git/") {
        return normalized.ends_with("/index")
            || normalized.ends_with("/HEAD")
            || normalized.contains("/refs/")
            || normalized.contains("/MERGE_")
            || normalized.contains("/rebase-");
    }
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return normalized.ends_with(".gitignore") || normalized.ends_with(".gitattributes");
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "flp" | "wav" | "mp3" | "ogg" | "aiff" | "aif" | "flac" | "fst" | "json" | "md"
    )
}

fn is_backup_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized == "backup" || normalized.starts_with("backup/") || normalized.contains("/backup/")
}

fn normalize_path(path: PathBuf) -> Result<PathBuf, String> {
    if path.exists() {
        fs::canonicalize(path)
            .map(normalize_windows_verbatim_path)
            .map_err(|err| err.to_string())
    } else {
        Ok(normalize_windows_verbatim_path(path))
    }
}

fn normalize_windows_verbatim_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let raw = path.to_string_lossy();
        if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{}", rest));
        }
        if let Some(rest) = raw.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

fn existing_repo_root(path: &Path) -> Option<PathBuf> {
    let output = hidden_command("git")
        .current_dir(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let root = stdout.trim();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

fn list_flp_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(path
            .extension()
            .and_then(|ext| ext.to_str())
            .filter(|ext| ext.eq_ignore_ascii_case("flp"))
            .map(|_| vec![path.to_path_buf()])
            .unwrap_or_default());
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(path).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let entry_path = entry.path();
        if entry_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("Backup"))
            .unwrap_or(false)
        {
            continue;
        }
        if entry_path.is_file()
            && entry_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("flp"))
                .unwrap_or(false)
        {
            files.push(entry_path);
        }
    }
    files.sort();
    Ok(files)
}

fn command_version(path: Option<&PathBuf>, fallback: &str, args: &[&str]) -> Option<String> {
    let exe = path
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| fallback.to_string());
    let output = hidden_command(&exe).args(args).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn resolve_flpdiff_path() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("bin").join(FLPDIFF_EXE_NAME),
        PathBuf::from("src-tauri")
            .join("bin")
            .join(FLPDIFF_EXE_NAME),
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join(FLPDIFF_EXE_NAME)))
            .unwrap_or_default(),
        materialize_embedded_flpdiff().unwrap_or_default(),
    ];
    candidates.into_iter().find(|path| path.exists())
}

fn materialize_embedded_flpdiff() -> Result<PathBuf, String> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| "Could not find local app data directory".to_string())?
        .join(CONFIG_DIR)
        .join("bin");
    fs::create_dir_all(&base).map_err(|err| err.to_string())?;
    let path = base.join(FLPDIFF_EXE_NAME);
    let needs_write = fs::metadata(&path)
        .map(|metadata| metadata.len() != FLPDIFF_BYTES.len() as u64)
        .unwrap_or(true);
    if needs_write {
        fs::write(&path, FLPDIFF_BYTES).map_err(|err| err.to_string())?;
    }
    Ok(path)
}

fn run_git(repo_root: &Path, args: &[&str]) -> Result<GitOutput, String> {
    let output = run_command("git", Some(repo_root), args);
    log_git_action(repo_root, args, &output);
    output
}

fn hidden_command(exe: &str) -> Command {
    let mut command = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn run_git_dynamic(
    repo_root: &str,
    subcommand: &str,
    paths: Vec<String>,
    force: bool,
) -> Result<GitOutput, String> {
    let repo_root = normalize_path(PathBuf::from(repo_root))?;
    let filtered_paths = paths
        .into_iter()
        .filter(|path| !is_backup_path(path))
        .collect::<Vec<_>>();
    if filtered_paths.is_empty() {
        return Err("No stageable paths selected".to_string());
    }
    let mut args = vec![subcommand.to_string()];
    if force {
        args.push("-f".to_string());
    }
    args.push("--".to_string());
    args.extend(filtered_paths);
    run_git_owned(&repo_root, &args)
}

fn run_git_owned(repo_root: &Path, args: &[String]) -> Result<GitOutput, String> {
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_git(repo_root, &refs)
}

fn compare_surface_pathspecs() -> Vec<String> {
    [
        "--",
        ".",
        ":(exclude)*.flp",
        ":(exclude)*.FLP",
        ":(exclude).gitignore",
        ":(exclude).gitattributes",
        ":(exclude).gitmodules",
        ":(exclude).lfsconfig",
        ":(exclude).flgit/**",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn run_git_bytes(repo_root: &Path, args: &[String]) -> Result<Vec<u8>, String> {
    let output = hidden_command("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .map_err(|err| format!("Failed to run git {}: {}", args.join(" "), err))?;
    let git_output = GitOutput {
        ok: output.status.success(),
        stdout: format!("{} bytes", output.stdout.len()),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    };
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    log_git_action(repo_root, &refs, &Ok(git_output.clone()));
    if git_output.ok {
        Ok(output.stdout)
    } else {
        Err(git_output.stderr)
    }
}

fn run_git_bytes_with_stdin(
    repo_root: &Path,
    args: &[String],
    stdin: &[u8],
) -> Result<Vec<u8>, String> {
    let mut child = hidden_command("git")
        .current_dir(repo_root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to run git {}: {}", args.join(" "), err))?;

    if let Some(mut child_stdin) = child.stdin.take() {
        child_stdin
            .write_all(stdin)
            .map_err(|err| format!("Failed to write input to git {}: {}", args.join(" "), err))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|err| format!("Failed to read output from git {}: {}", args.join(" "), err))?;
    let git_output = GitOutput {
        ok: output.status.success(),
        stdout: format!("{} bytes", output.stdout.len()),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    };
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    log_git_action(repo_root, &refs, &Ok(git_output.clone()));
    if git_output.ok {
        Ok(output.stdout)
    } else {
        Err(git_output.stderr)
    }
}

fn semantic_flp_diff_for_revisions(
    repo_root: &Path,
    base_revision: &str,
    head_revision: &str,
    paths_text: &str,
) -> Result<String, String> {
    let paths = paths_text
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(String::new());
    }

    let exe = resolve_flpdiff_path()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "flpdiff".to_string());
    let temp_root = env::temp_dir().join(format!(
        "flgit-compare-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    ));
    fs::create_dir_all(&temp_root).map_err(|err| err.to_string())?;

    let mut output = String::from("Semantic FLP diff:\n");
    for path in paths {
        let normalized_path = path.replace('\\', "/");
        let safe_name = sanitize_temp_file_name(&normalized_path);
        let base_file = temp_root.join(format!("base-{}", safe_name));
        let head_file = temp_root.join(format!("head-{}", safe_name));
        let base_blob = git_blob_at_revision(repo_root, base_revision, &normalized_path);
        let head_blob = git_blob_at_revision(repo_root, head_revision, &normalized_path);

        match (base_blob, head_blob) {
            (Ok(base), Ok(head)) => {
                fs::write(&base_file, base).map_err(|err| err.to_string())?;
                fs::write(&head_file, head).map_err(|err| err.to_string())?;
                let diff = run_command(
                    &exe,
                    Some(repo_root),
                    &[
                        base_file.to_string_lossy().as_ref(),
                        head_file.to_string_lossy().as_ref(),
                    ],
                )?;
                output.push_str(&format!("\n--- {}\n", normalized_path));
                if diff.stdout.trim().is_empty() && diff.stderr.trim().is_empty() {
                    output.push_str("No semantic FLP changes reported.\n");
                } else {
                    output.push_str(diff.stdout.trim());
                    if !diff.stderr.trim().is_empty() {
                        output.push('\n');
                        output.push_str(diff.stderr.trim());
                    }
                    output.push('\n');
                }
            }
            (Err(_), Ok(_)) => {
                output.push_str(&format!(
                    "\n--- {}\nFLP added in selected range.\n",
                    normalized_path
                ));
            }
            (Ok(_), Err(_)) => {
                output.push_str(&format!(
                    "\n--- {}\nFLP removed in selected range.\n",
                    normalized_path
                ));
            }
            (Err(base_err), Err(head_err)) => {
                output.push_str(&format!(
                    "\n--- {}\nCould not extract FLP from either revision.\nbase: {}\nhead: {}\n",
                    normalized_path, base_err, head_err
                ));
            }
        }
    }

    let _ = fs::remove_dir_all(&temp_root);
    Ok(output)
}

fn git_blob_at_revision(repo_root: &Path, revision: &str, path: &str) -> Result<Vec<u8>, String> {
    let spec = format!("{}:{}", revision, path);
    let blob = run_git_bytes(repo_root, &["show".to_string(), spec])?;
    if is_lfs_pointer(&blob) {
        run_git_bytes_with_stdin(
            repo_root,
            &[
                "lfs".to_string(),
                "smudge".to_string(),
                "--".to_string(),
                path.to_string(),
            ],
            &blob,
        )
    } else {
        Ok(blob)
    }
}

fn is_lfs_pointer(blob: &[u8]) -> bool {
    blob.starts_with(b"version https://git-lfs.github.com/spec/v1\n")
}

fn sanitize_temp_file_name(path: &str) -> String {
    path.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn run_command(exe: &str, cwd: Option<&Path>, args: &[&str]) -> Result<GitOutput, String> {
    let mut command = hidden_command(exe);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .args(args)
        .output()
        .map_err(|err| format!("Failed to run {} {}: {}", exe, args.join(" "), err))?;
    Ok(GitOutput {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn log_clone_git_action(
    destination: &Path,
    cwd: &Path,
    args: &[&str],
    output: &Result<GitOutput, String>,
) {
    let log_root = if destination.exists() {
        destination.join(".flgit").join("logs")
    } else {
        dirs::data_local_dir()
            .unwrap_or_else(env::temp_dir)
            .join(CONFIG_DIR)
            .join("logs")
    };
    log_git_action_to_dir(&log_root, cwd, args, output);
}

fn log_git_action(repo_root: &Path, args: &[&str], output: &Result<GitOutput, String>) {
    let log_dir = repo_root.join(".flgit").join("logs");
    log_git_action_to_dir(&log_dir, repo_root, args, output);
}

fn log_git_action_to_dir(
    log_dir: &Path,
    cwd: &Path,
    args: &[&str],
    output: &Result<GitOutput, String>,
) {
    if fs::create_dir_all(log_dir).is_err() {
        return;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let session = GIT_LOG_SESSION.get_or_init(|| timestamp.to_string());
    let path = log_dir.join(format!("git-{}.log", session));
    let mut file = match fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(_) => return,
    };
    let _ = writeln!(
        file,
        "time={} cwd={} cmd=git {}",
        timestamp,
        cwd.display(),
        args.join(" ")
    );
    match output {
        Ok(output) => {
            let _ = writeln!(file, "ok={}", output.ok);
            if !output.stdout.trim().is_empty() {
                let _ = writeln!(file, "stdout:\n{}", output.stdout.trim_end());
            }
            if !output.stderr.trim().is_empty() {
                let _ = writeln!(file, "stderr:\n{}", output.stderr.trim_end());
            }
        }
        Err(err) => {
            let _ = writeln!(file, "error={}", err);
        }
    }
    let _ = writeln!(file, "---");
}

fn parse_status(repo_root: &Path, status_text: &str) -> RepoStatus {
    let mut result = RepoStatus {
        repo_root: repo_root.to_path_buf(),
        is_repo: true,
        ..RepoStatus::default()
    };

    if status_text.contains('\0') {
        parse_status_z(status_text, &mut result);
        return result;
    }

    for line in status_text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            parse_branch_line(rest, &mut result);
            continue;
        }
        if line.len() < 4 {
            continue;
        }

        let bytes = line.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let raw_path = &line[3..];
        let (path, original_path) = raw_path
            .split_once(" -> ")
            .map(|(old, new)| (decode_git_path(new), Some(decode_git_path(old))))
            .unwrap_or_else(|| (decode_git_path(raw_path), None));
        let status = format!("{}{}", x, y);
        let conflicted = matches!(
            status.as_str(),
            "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"
        );
        let staged = x != ' ' && x != '?';
        let unstaged = y != ' ' || x == '?';
        result.changes.push(FileChange {
            category: categorize_path(&path),
            path,
            original_path,
            status,
            staged,
            unstaged,
            conflicted,
        });
    }

    result
}

fn parse_status_z(status_text: &str, result: &mut RepoStatus) {
    let mut entries = status_text.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        if let Some(rest) = entry.strip_prefix("## ") {
            parse_branch_line(rest, result);
            continue;
        }
        if entry.len() < 4 {
            continue;
        }

        let bytes = entry.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let status = format!("{}{}", x, y);
        let mut path = decode_git_path(&entry[3..]);
        let mut original_path = None;
        if x == 'R' || x == 'C' {
            if let Some(next) = entries.next() {
                original_path = Some(path);
                path = decode_git_path(next);
            }
        }
        let conflicted = matches!(
            status.as_str(),
            "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"
        );
        let staged = x != ' ' && x != '?';
        let unstaged = y != ' ' || x == '?';
        result.changes.push(FileChange {
            category: categorize_path(&path),
            path,
            original_path,
            status,
            staged,
            unstaged,
            conflicted,
        });
    }
}

fn decode_git_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut output = String::new();
        let mut chars = inner.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                output.push(ch);
                continue;
            }
            match chars.next() {
                Some('"') => output.push('"'),
                Some('\\') => output.push('\\'),
                Some('n') => output.push('\n'),
                Some('t') => output.push('\t'),
                Some(other) => output.push(other),
                None => output.push('\\'),
            }
        }
        output
    } else {
        trimmed.to_string()
    }
}

fn parse_branch_line(line: &str, result: &mut RepoStatus) {
    let (branch, meta) = line
        .split_once("...")
        .map(|(branch, meta)| (branch, Some(meta)))
        .unwrap_or((line, None));
    result.branch = Some(branch.to_string());

    if let Some(meta) = meta {
        let upstream = meta.split(' ').next().unwrap_or_default();
        result.upstream = Some(upstream.to_string());
        result.has_remote = !upstream.is_empty();

        if let (Some(start), Some(end)) = (meta.find('['), meta.find(']')) {
            for part in meta[start + 1..end].split(',') {
                let part = part.trim();
                if let Some(value) = part.strip_prefix("ahead ") {
                    result.ahead = value.parse().unwrap_or(0);
                }
                if let Some(value) = part.strip_prefix("behind ") {
                    result.behind = value.parse().unwrap_or(0);
                }
            }
        }
    }
}

fn parse_commit_log(log: &str) -> Vec<CommitSummary> {
    log.lines()
        .filter_map(|line| {
            let mut parts = line.split('\x1f');
            let sha = parts.next()?;
            let short_sha = parts.next()?;
            let subject = parts.next()?;
            let author = parts.next()?;
            let relative_time = parts.next()?;
            Some(CommitSummary {
                sha: sha.to_string(),
                short_sha: short_sha.to_string(),
                subject: subject.to_string(),
                author: author.to_string(),
                relative_time: relative_time.to_string(),
            })
        })
        .collect()
}

fn categorize_path(path: &str) -> ChangeCategory {
    let decoded = decode_git_path(path);
    let ext = Path::new(&decoded)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "flp" => ChangeCategory::Flp,
        "wav" | "mp3" | "ogg" | "aiff" | "aif" | "flac" | "mid" | "midi" => ChangeCategory::Samples,
        "zip" | "rar" | "7z" | "mp4" | "mov" => ChangeCategory::Exports,
        "json" | "md" | "txt" | "gitattributes" | "gitignore" => ChangeCategory::Metadata,
        _ => ChangeCategory::Other,
    }
}

fn update_gitattributes(repo_root: &Path) -> Result<(), String> {
    let path = repo_root.join(".gitattributes");
    let existing = if path.exists() {
        fs::read_to_string(&path).map_err(|err| err.to_string())?
    } else {
        String::new()
    };

    let required = [
        "*.flp filter=lfs diff=flp merge=lfs -text",
        "*.FLP filter=lfs diff=flp merge=lfs -text",
        "*.zip filter=lfs diff=lfs merge=lfs -text",
        "*.ZIP filter=lfs diff=lfs merge=lfs -text",
        "*.wav filter=lfs diff=lfs merge=lfs -text",
        "*.WAV filter=lfs diff=lfs merge=lfs -text",
        "*.mp3 filter=lfs diff=lfs merge=lfs -text",
        "*.MP3 filter=lfs diff=lfs merge=lfs -text",
        "*.ogg filter=lfs diff=lfs merge=lfs -text",
        "*.OGG filter=lfs diff=lfs merge=lfs -text",
        "*.aiff filter=lfs diff=lfs merge=lfs -text",
        "*.AIFF filter=lfs diff=lfs merge=lfs -text",
        "*.aif filter=lfs diff=lfs merge=lfs -text",
        "*.AIF filter=lfs diff=lfs merge=lfs -text",
        "*.flac filter=lfs diff=lfs merge=lfs -text",
        "*.FLAC filter=lfs diff=lfs merge=lfs -text",
        "*.fst filter=lfs diff=lfs merge=lfs -text",
        "*.FST filter=lfs diff=lfs merge=lfs -text",
    ];

    let mut lines = existing
        .lines()
        .map(str::to_string)
        .collect::<BTreeSet<String>>();
    for line in required {
        lines.insert(line.to_string());
    }

    let mut output = lines.into_iter().collect::<Vec<_>>().join("\n");
    output.push('\n');
    fs::write(path, output).map_err(|err| err.to_string())
}

fn update_gitignore(repo_root: &Path) -> Result<(), String> {
    let path = repo_root.join(".gitignore");
    let existing = if path.exists() {
        fs::read_to_string(&path).map_err(|err| err.to_string())?
    } else {
        String::new()
    };

    let required = ["Backup/", "Backup/**", "*.tmp", "*.bak", ".flgit/logs/"];
    let mut lines = existing
        .lines()
        .map(str::to_string)
        .collect::<BTreeSet<String>>();
    for line in required {
        lines.insert(line.to_string());
    }

    let mut output = lines.into_iter().collect::<Vec<_>>().join("\n");
    output.push('\n');
    fs::write(path, output).map_err(|err| err.to_string())
}

#[cfg(windows)]
fn detect_fl_window() -> FlWindowInfo {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    struct SearchState {
        found: Option<FlWindowInfo>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return BOOL(1);
        }
        let mut buffer = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        if copied <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buffer[..copied as usize]);
        if !title.to_ascii_lowercase().contains("fl studio") {
            return BOOL(1);
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return BOOL(1);
        }
        let state = &mut *(lparam.0 as *mut SearchState);
        state.found = Some(FlWindowInfo {
            found: true,
            title: Some(title),
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        });
        BOOL(0)
    }

    let mut state = SearchState { found: None };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut state as *mut _ as isize));
    }
    state.found.unwrap_or(FlWindowInfo {
        found: false,
        title: None,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    })
}

#[cfg(not(windows))]
fn detect_fl_window() -> FlWindowInfo {
    FlWindowInfo {
        found: false,
        title: None,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_clean_branch_tracking() {
        let status = parse_status(
            Path::new("C:/tmp/project"),
            "## main...origin/main [ahead 2, behind 1]\n",
        );
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(status.ahead, 2);
        assert_eq!(status.behind, 1);
        assert!(status.has_remote);
        assert!(status.changes.is_empty());
    }

    #[test]
    fn parses_changes_and_categories() {
        let status = parse_status(
            Path::new("C:/tmp/project"),
            "## main\n M song.flp\nA  Samples/kick.wav\n?? notes.md\nUU collab.flp\nR  old.flp -> new.flp\n",
        );
        assert_eq!(status.changes.len(), 5);
        assert_eq!(status.changes[0].category, ChangeCategory::Flp);
        assert!(status.changes[0].unstaged);
        assert!(status.changes[1].staged);
        assert_eq!(status.changes[1].category, ChangeCategory::Samples);
        assert_eq!(status.changes[2].category, ChangeCategory::Metadata);
        assert!(status.changes[3].conflicted);
        assert_eq!(status.changes[4].original_path.as_deref(), Some("old.flp"));
    }

    #[test]
    fn parses_quoted_paths_as_samples_and_flps() {
        let status = parse_status(
            Path::new("C:/tmp/project"),
            "## main\0?? \"!classic rim.wav\"\0?? \"808 30nickk (C) @wintfye.wav\"\0?? \"memories 4.flp\"\0?? \"take the world arsunol 2.mp3\"\0",
        );
        assert_eq!(status.changes.len(), 4);
        assert_eq!(status.changes[0].path, "!classic rim.wav");
        assert_eq!(status.changes[0].category, ChangeCategory::Samples);
        assert_eq!(status.changes[1].category, ChangeCategory::Samples);
        assert_eq!(status.changes[2].category, ChangeCategory::Flp);
        assert_eq!(status.changes[3].category, ChangeCategory::Samples);
    }

    #[test]
    fn first_commit_revision_diff_message_shape() {
        let output = GitOutput {
            ok: true,
            stdout: "This is the first commit".to_string(),
            stderr: String::new(),
        };
        assert_eq!(output.stdout, "This is the first commit");
    }

    #[test]
    fn detects_lfs_pointer_blobs() {
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 123\n";
        assert!(is_lfs_pointer(pointer));
        assert!(!is_lfs_pointer(b"FLhd real flp bytes"));
    }

    #[test]
    fn compare_surface_pathspecs_exclude_flps_and_internal_git_files() {
        let pathspecs = compare_surface_pathspecs();
        assert!(pathspecs.contains(&":(exclude)*.flp".to_string()));
        assert!(pathspecs.contains(&":(exclude).gitattributes".to_string()));
        assert!(pathspecs.contains(&":(exclude).gitignore".to_string()));
        assert!(pathspecs.contains(&":(exclude).flgit/**".to_string()));
    }

    #[test]
    fn unstage_uses_cached_rm_before_first_commit() {
        let args = unstage_args(false, vec!["kick.wav".to_string()]);
        assert_eq!(args, vec!["rm", "-r", "--cached", "--", "kick.wav"]);
    }

    #[test]
    fn unstage_uses_restore_when_repo_has_commits() {
        let args = unstage_args(true, vec!["song.flp".to_string()]);
        assert_eq!(args, vec!["restore", "--staged", "--", "song.flp"]);
    }

    #[test]
    fn updates_gitattributes_without_dropping_existing_rules() {
        let dir = tempdir().unwrap();
        let attrs = dir.path().join(".gitattributes");
        fs::write(&attrs, "*.png filter=lfs diff=lfs merge=lfs -text\n").unwrap();
        update_gitattributes(dir.path()).unwrap();
        let updated = fs::read_to_string(attrs).unwrap();
        assert!(updated.contains("*.png filter=lfs diff=lfs merge=lfs -text"));
        assert!(updated.contains("*.flp filter=lfs diff=flp merge=lfs -text"));
        assert!(updated.contains("*.wav filter=lfs diff=lfs merge=lfs -text"));
    }

    #[test]
    fn updates_gitignore_for_fl_backup_folder() {
        let dir = tempdir().unwrap();
        let ignore = dir.path().join(".gitignore");
        fs::write(&ignore, ".env\n").unwrap();
        update_gitignore(dir.path()).unwrap();
        let updated = fs::read_to_string(ignore).unwrap();
        assert!(updated.contains(".env"));
        assert!(updated.contains("Backup/"));
        assert!(updated.contains("Backup/**"));
    }

    #[test]
    fn writes_project_metadata() {
        let dir = tempdir().unwrap();
        write_project_metadata(dir.path(), "Track One").unwrap();
        let metadata = fs::read_to_string(dir.path().join(".flgit").join("project.json")).unwrap();
        assert!(metadata.contains("\"name\": \"Track One\""));
        assert!(metadata.contains("\"Backup\""));
        assert!(metadata.contains("\"Audio\""));
    }

    #[test]
    fn close_reset_clears_project_state_and_preserves_global_preferences() {
        let mut config = AppConfig {
            bound_project_path: Some(PathBuf::from("C:/Songs/Track/track.flp")),
            repo_root: Some(PathBuf::from("C:/Songs/Track")),
            project_name: Some("Track".to_string()),
            last_selected_branch: Some("main".to_string()),
            overlay_placement: OverlayPlacement::Floating,
            overlay_expanded: false,
            expanded_position: Some(SavedWindowGeometry {
                x: 10,
                y: 20,
                width: 700,
                height: 500,
            }),
            flpdiff_path: Some(PathBuf::from("C:/Tools/flpdiff.exe")),
            git_path: Some(PathBuf::from("C:/Tools/git.exe")),
            git_lfs_path: Some(PathBuf::from("C:/Tools/git-lfs.exe")),
        };

        reset_project_state_for_close(&mut config);

        assert!(config.bound_project_path.is_none());
        assert!(config.repo_root.is_none());
        assert!(config.project_name.is_none());
        assert!(config.last_selected_branch.is_none());
        assert!(config.expanded_position.is_none());
        assert!(config.overlay_expanded);
        assert!(matches!(
            config.overlay_placement,
            OverlayPlacement::Floating
        ));
        assert_eq!(
            config.flpdiff_path.as_deref(),
            Some(Path::new("C:/Tools/flpdiff.exe"))
        );
        assert_eq!(
            config.git_path.as_deref(),
            Some(Path::new("C:/Tools/git.exe"))
        );
        assert_eq!(
            config.git_lfs_path.as_deref(),
            Some(Path::new("C:/Tools/git-lfs.exe"))
        );
    }

    #[test]
    fn default_config_opens_expanded_without_project_binding() {
        let config = AppConfig::default();
        assert!(config.overlay_expanded);
        assert!(config.bound_project_path.is_none());
        assert!(config.repo_root.is_none());
        assert!(config.expanded_position.is_none());
    }

    #[test]
    fn parses_commit_log() {
        let commits = parse_commit_log(
            "0123456789abcdef\x1f0123456\x1fInitial commit\x1fAdmin\x1f2 minutes ago\nabcdef\x1fabcdef0\x1fUpdate mix\x1fProducer\x1fyesterday",
        );
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].short_sha, "0123456");
        assert_eq!(commits[0].subject, "Initial commit");
        assert_eq!(commits[1].author, "Producer");
    }

    #[test]
    fn project_change_filter_ignores_backups() {
        assert!(!should_emit_project_change(Path::new(
            "C:/Song/Backup/song_1.flp"
        )));
        assert!(should_emit_project_change(Path::new("C:/Song/song.flp")));
        assert!(should_emit_project_change(Path::new(
            "C:/Song/Audio/kick.wav"
        )));
        assert!(should_emit_project_change(Path::new("C:/Song/.git/index")));
    }

    #[test]
    fn writes_and_reads_lock_file() {
        let dir = tempdir().unwrap();
        let lock = ProjectLock {
            active: true,
            owner: Some("Admin".to_string()),
            created_at_unix: Some(123),
        };
        write_lock_file(dir.path(), &lock).unwrap();
        let read = read_lock_file(dir.path()).unwrap();
        assert!(read.active);
        assert_eq!(read.owner.as_deref(), Some("Admin"));
        assert_eq!(read.created_at_unix, Some(123));
    }

    #[test]
    fn detects_backup_paths() {
        assert!(is_backup_path("Backup/song.flp"));
        assert!(is_backup_path("Project/Backup/song.flp"));
        assert!(!is_backup_path("Audio/kick.wav"));
    }

    #[test]
    fn validates_clone_remote_urls() {
        assert!(validate_clone_remote_url("https://github.com/user/repo.git").is_ok());
        assert!(validate_clone_remote_url("git@github.com:user/repo.git").is_ok());
        assert!(validate_clone_remote_url("ssh://git@github.com/user/repo.git").is_ok());
        assert!(validate_clone_remote_url("").is_err());
        assert!(validate_clone_remote_url("https://example.com/user/repo.git").is_err());
        assert!(validate_clone_remote_url("https://github.com/user repo.git").is_err());
    }

    #[test]
    fn validates_clone_destination_shape() {
        let dir = tempdir().unwrap();
        let empty = dir.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(validate_clone_destination(&empty).is_ok());

        let missing = dir.path().join("missing");
        assert!(validate_clone_destination(&missing).is_ok());

        let non_empty = dir.path().join("non-empty");
        fs::create_dir(&non_empty).unwrap();
        fs::write(non_empty.join("song.flp"), "fake").unwrap();
        assert!(validate_clone_destination(&non_empty).is_err());

        let git_repo = dir.path().join("repo");
        fs::create_dir(&git_repo).unwrap();
        fs::create_dir(git_repo.join(".git")).unwrap();
        assert!(validate_clone_destination(&git_repo).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_windows_verbatim_paths_for_git_cli() {
        assert_eq!(
            normalize_windows_verbatim_path(PathBuf::from(r"\\?\C:\Users\Admin\Downloads\remv2")),
            PathBuf::from(r"C:\Users\Admin\Downloads\remv2")
        );
        assert_eq!(
            normalize_windows_verbatim_path(PathBuf::from(r"\\?\UNC\server\share\repo")),
            PathBuf::from(r"\\server\share\repo")
        );
    }

    #[test]
    fn local_flpdiff_setup_does_not_create_tracked_attributes() {
        if command_version(None, "git", &["--version"]).is_none() {
            return;
        }
        let dir = tempdir().unwrap();
        run_git(dir.path(), &["init"]).unwrap();
        configure_local_flpdiff_driver(dir.path()).unwrap();

        assert!(!dir.path().join(".gitattributes").exists());
        assert!(!dir.path().join(".gitignore").exists());
        assert!(dir
            .path()
            .join(".git")
            .join("info")
            .join("attributes")
            .exists());
        let status = run_git(
            dir.path(),
            &[
                "status",
                "--porcelain",
                "--",
                ".gitattributes",
                ".gitignore",
            ],
        )
        .unwrap();
        assert!(status.stdout.trim().is_empty());
    }

    #[test]
    fn clone_project_from_local_bare_repo_binds_destination() {
        if command_version(None, "git", &["--version"]).is_none()
            || command_version(None, "git", &["lfs", "version"]).is_none()
        {
            return;
        }

        let dir = tempdir().unwrap();
        let source = dir.path().join("source");
        let bare = dir.path().join("remote.git");
        let destination = dir.path().join("clone");
        fs::create_dir(&source).unwrap();
        run_git(&source, &["init"]).unwrap();
        run_git(&source, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(&source, &["config", "user.name", "Test User"]).unwrap();
        fs::write(source.join("song.flp"), "fake flp").unwrap();
        run_git(&source, &["add", "--", "song.flp"]).unwrap();
        run_git(&source, &["commit", "-m", "Initial"]).unwrap();
        run_git(
            dir.path(),
            &[
                "clone",
                "--bare",
                &source.to_string_lossy(),
                &bare.to_string_lossy(),
            ],
        )
        .unwrap();

        clone_remote_into(&bare.to_string_lossy(), &destination).unwrap();

        assert!(destination.join(".git").exists());
        assert_eq!(list_flp_files(&destination).unwrap().len(), 1);
        assert!(destination
            .join(".git")
            .join("info")
            .join("attributes")
            .exists());
    }
}
