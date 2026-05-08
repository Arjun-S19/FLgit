import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { FileChange } from "./domain";

export interface AppConfig {
  boundProjectPath?: string | null;
  repoRoot?: string | null;
  projectName?: string | null;
  lastSelectedBranch?: string | null;
  overlayPlacement: "left" | "right" | "floating";
  overlayExpanded: boolean;
  flpdiffPath?: string | null;
  gitPath?: string | null;
  gitLfsPath?: string | null;
}

export interface PrerequisiteStatus {
  gitAvailable: boolean;
  gitLfsAvailable: boolean;
  githubCliAvailable: boolean;
  flpdiffAvailable: boolean;
  gitVersion?: string | null;
  gitLfsVersion?: string | null;
  githubCliVersion?: string | null;
  flpdiffVersion?: string | null;
}

export interface BindProjectResult {
  boundProjectPath: string;
  repoRoot: string;
  isRepo: boolean;
  projectName?: string | null;
  flpFiles: string[];
}

export interface RepoStatus {
  repoRoot: string;
  branch?: string | null;
  upstream?: string | null;
  remoteVisibility?: string | null;
  ahead: number;
  behind: number;
  hasRemote: boolean;
  isRepo: boolean;
  mergeInProgress: boolean;
  rebaseInProgress: boolean;
  changes: FileChange[];
}

export interface GitOutput {
  ok: boolean;
  stdout: string;
  stderr: string;
}

export interface CommitSummary {
  sha: string;
  shortSha: string;
  subject: string;
  author: string;
  relativeTime: string;
}

export interface ProjectLock {
  active: boolean;
  owner?: string | null;
  createdAtUnix?: number | null;
}

export interface FlWindowInfo {
  found: boolean;
  title?: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
}

function isTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function chooseProjectPath(): Promise<string | null> {
  if (!isTauri()) return null;
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "FL Studio Project", extensions: ["flp"] }],
  });
  if (typeof selected === "string") return selected;

  const folder = await open({
    multiple: false,
    directory: true,
  });
  return typeof folder === "string" ? folder : null;
}

export async function chooseCloneDestination(): Promise<string | null> {
  if (!isTauri()) return null;
  const selected = await open({
    multiple: false,
    directory: true,
  });
  return typeof selected === "string" ? selected : null;
}

export async function getConfig(): Promise<AppConfig> {
  if (!isTauri()) {
    return { overlayPlacement: "right", overlayExpanded: true };
  }
  return invoke("get_config");
}

export async function checkPrerequisites(config?: AppConfig): Promise<PrerequisiteStatus> {
  if (!isTauri()) {
    return {
      gitAvailable: false,
      gitLfsAvailable: false,
      githubCliAvailable: false,
      flpdiffAvailable: false,
    };
  }
  return invoke("check_prerequisites", { config });
}

export async function bindProject(projectPath: string): Promise<BindProjectResult> {
  return invoke("bind_project", { projectPath });
}

export async function cloneProject(remoteUrl: string, destinationPath: string): Promise<BindProjectResult> {
  return invoke("clone_project", { remoteUrl, destinationPath });
}

export async function initializeProjectRepo(
  repoRoot: string,
  projectName: string,
  defaultBranch = "main",
): Promise<GitOutput> {
  return invoke("initialize_project_repo", { repoRoot, projectName, defaultBranch });
}

export async function repoStatus(repoRoot: string): Promise<RepoStatus> {
  return invoke("repo_status", { repoRoot });
}

export async function setupRepo(repoRoot: string): Promise<GitOutput> {
  return invoke("setup_git_lfs_flpdiff", { repoRoot });
}

export async function stagePaths(repoRoot: string, paths: string[]): Promise<GitOutput> {
  return invoke("stage_paths", { repoRoot, paths });
}

export async function unstagePaths(repoRoot: string, paths: string[]): Promise<GitOutput> {
  return invoke("unstage_paths", { repoRoot, paths });
}

export async function commitChanges(repoRoot: string, message: string): Promise<GitOutput> {
  return invoke("commit", { repoRoot, message });
}

export async function pullRepo(repoRoot: string): Promise<GitOutput> {
  return invoke("pull", { repoRoot });
}

export async function pushRepo(repoRoot: string): Promise<GitOutput> {
  return invoke("push", { repoRoot });
}

export async function setRemote(repoRoot: string, remoteUrl: string): Promise<GitOutput> {
  return invoke("set_remote", { repoRoot, remoteUrl });
}

export async function diffPath(repoRoot: string, path: string, staged: boolean): Promise<GitOutput> {
  return invoke("diff_path", { repoRoot, path, staged });
}

export async function showCommit(repoRoot: string, revision: string): Promise<GitOutput> {
  return invoke("show_commit", { repoRoot, revision });
}

export async function listCommits(repoRoot: string, limit = 40): Promise<CommitSummary[]> {
  return invoke("list_commits", { repoRoot, limit });
}

export async function diffRevisionPath(repoRoot: string, revision: string, path: string): Promise<GitOutput> {
  return invoke("diff_revision_path", { repoRoot, revision, path });
}

export async function diffCommits(
  repoRoot: string,
  baseRevision: string,
  headRevision: string,
): Promise<GitOutput> {
  return invoke("diff_commits", { repoRoot, baseRevision, headRevision });
}

export async function diffWorkingTreeAgainstRevision(
  repoRoot: string,
  revision: string,
  path: string,
): Promise<GitOutput> {
  return invoke("diff_working_tree_against_revision", { repoRoot, revision, path });
}

export async function resetToCommit(repoRoot: string, revision: string): Promise<GitOutput> {
  return invoke("reset_to_commit", { repoRoot, revision });
}

export async function githubPublishRepo(
  repoRoot: string,
  repoName: string,
  visibility: "private" | "public" | "internal" = "private",
): Promise<GitOutput> {
  return invoke("github_publish_repo", { repoRoot, repoName, visibility });
}

export async function startProjectWatch(repoRoot: string): Promise<void> {
  if (!isTauri()) return;
  return invoke("start_project_watch", { repoRoot });
}

export async function stopProjectWatch(): Promise<void> {
  if (!isTauri()) return;
  return invoke("stop_project_watch");
}

export async function resolveConflictPath(
  repoRoot: string,
  path: string,
  choice: "local" | "remote",
): Promise<GitOutput> {
  return invoke("resolve_conflict_path", { repoRoot, path, choice });
}

export async function abortMergeOrRebase(repoRoot: string): Promise<GitOutput> {
  return invoke("abort_merge_or_rebase", { repoRoot });
}

export async function readProjectLock(repoRoot: string): Promise<ProjectLock> {
  return invoke("read_project_lock", { repoRoot });
}

export async function acquireProjectLock(repoRoot: string): Promise<GitOutput> {
  return invoke("acquire_project_lock", { repoRoot });
}

export async function releaseProjectLock(repoRoot: string): Promise<GitOutput> {
  return invoke("release_project_lock", { repoRoot });
}

export async function detectFlStudioWindow(): Promise<FlWindowInfo> {
  if (!isTauri()) return { found: false, x: 0, y: 0, width: 0, height: 0 };
  return invoke("detect_fl_studio_window");
}

export async function anchorToFlStudio(placement: AppConfig["overlayPlacement"]): Promise<FlWindowInfo> {
  return invoke("anchor_to_fl_studio", { placement });
}

export async function setOverlayExpanded(expanded: boolean): Promise<void> {
  if (!isTauri()) return;
  return invoke("set_overlay_expanded", { expanded });
}

export async function startOverlayDrag(): Promise<void> {
  if (!isTauri()) return;
  return invoke("start_overlay_drag");
}

export async function undockOverlay(): Promise<void> {
  if (!isTauri()) return;
  return invoke("undock_overlay");
}

export async function closeWindow(): Promise<void> {
  if (!isTauri()) return;
  return invoke("close_window");
}
