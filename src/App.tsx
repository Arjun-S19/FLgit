import {
  AlertTriangle,
  ArrowDownUp,
  Check,
  ChevronRight,
  Cloud,
  X,
  Download,
  ExternalLink,
  FileCode2,
  FolderOpen,
  GitBranch,
  GitCommitHorizontal,
  GitCompare,
  Lock,
  Loader2,
  Minimize2,
  PanelRight,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  Unlock,
  Upload,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  anchorToFlStudio,
  abortMergeOrRebase,
  acquireProjectLock,
  bindProject,
  checkPrerequisites,
  chooseCloneDestination,
  chooseProjectPath,
  cloneProject,
  closeWindow,
  commitChanges,
  detectFlStudioWindow,
  diffPath,
  diffCommits,
  diffRevisionPath,
  getConfig,
  githubPublishRepo,
  initializeProjectRepo,
  listCommits,
  pullRepo,
  pushRepo,
  readProjectLock,
  releaseProjectLock,
  resetToCommit,
  repoStatus,
  resolveConflictPath,
  setRemote,
  setOverlayExpanded,
  setupRepo,
  showCommit,
  startOverlayDrag,
  startProjectWatch,
  stopProjectWatch,
  stagePaths,
  unstagePaths,
  type AppConfig,
  type BindProjectResult,
  type CommitSummary,
  type FlWindowInfo,
  type GitOutput,
  type ProjectLock,
  type PrerequisiteStatus,
  type RepoStatus,
  undockOverlay,
} from "./api";
import { groupChanges, shortPath, statusLabel, type FileChange } from "./domain";
import {
  canResetSelectedCommit,
  handleHistoryDoubleClick,
  handleHistorySingleClick,
  initialHistoryState,
  makeDiffTitle,
  type HistoryState,
} from "./viewModel";

type BusyAction =
  | "boot"
  | "bind"
  | "clone"
  | "refresh"
  | "setup"
  | "init"
  | "stage"
  | "commit"
  | "pull"
  | "push"
  | "diff"
  | "anchor"
  | "lock"
  | "resolve"
  | "reset"
  | null;

export function App() {
  const [config, setConfig] = useState<AppConfig>({ overlayPlacement: "right", overlayExpanded: true });
  const [prereqs, setPrereqs] = useState<PrerequisiteStatus | null>(null);
  const [binding, setBinding] = useState<BindProjectResult | null>(null);
  const [status, setStatus] = useState<RepoStatus | null>(null);
  const [selected, setSelected] = useState<FileChange | null>(null);
  const [history, setHistory] = useState<HistoryState>(initialHistoryState);
  const [commits, setCommits] = useState<CommitSummary[]>([]);
  const [diff, setDiff] = useState<string>("Select a changed file to inspect its diff");
  const [commitMessage, setCommitMessage] = useState("");
  const [remoteUrl, setRemoteUrl] = useState("");
  const [cloneUrl, setCloneUrl] = useState("");
  const [cloneDestination, setCloneDestination] = useState("");
  const [cloneDialogOpen, setCloneDialogOpen] = useState(false);
  const [githubVisibility, setGithubVisibility] = useState<"private" | "public" | "internal">("private");
  const [projectName, setProjectName] = useState("");
  const [flWindow, setFlWindow] = useState<FlWindowInfo | null>(null);
  const [projectLock, setProjectLock] = useState<ProjectLock>({ active: false });
  const [isDocked, setIsDocked] = useState(false);
  const [busy, setBusy] = useState<BusyAction>("boot");
  const [notice, setNotice] = useState<string>("");
  const [error, setError] = useState<string>("");
  const overlayDrag = useRef({ x: 0, y: 0, dragged: false });

  const repoRoot = binding?.repoRoot ?? config.repoRoot ?? null;
  const hasRepo = Boolean(status?.isRepo);
  const hasRemote = Boolean(status?.hasRemote);

  const refreshStatus = useCallback(
    async (root = repoRoot) => {
      if (!root) return;
      setBusy("refresh");
      setError("");
      try {
        const next = await repoStatus(root);
        setStatus(next);
        if (next.isRepo) {
          setCommits(await listCommits(root));
          setProjectLock(await readProjectLock(root));
        }
      } catch (err) {
        setError(String(err));
      } finally {
        setBusy(null);
      }
    },
    [repoRoot],
  );

  useEffect(() => {
    let cancelled = false;
    async function boot() {
      setBusy("boot");
      try {
        const loaded = await getConfig();
        if (cancelled) return;
        setConfig(loaded);
        const [nextPrereqs, windowInfo] = await Promise.all([
          checkPrerequisites(loaded),
          detectFlStudioWindow(),
        ]);
        if (cancelled) return;
        setPrereqs(nextPrereqs);
        setFlWindow(windowInfo);
        setProjectName(loaded.projectName ?? loaded.repoRoot?.split(/[\\/]/).pop() ?? "");
        await setOverlayExpanded(loaded.overlayExpanded);
        if (loaded.repoRoot) {
          const nextStatus = await repoStatus(loaded.repoRoot);
          if (!cancelled) {
            setStatus(nextStatus);
            if (nextStatus.isRepo) {
              setCommits(await listCommits(loaded.repoRoot));
              setProjectLock(await readProjectLock(loaded.repoRoot));
            }
          }
        }
      } catch (err) {
        if (!cancelled) setError(String(err));
      } finally {
        if (!cancelled) setBusy(null);
      }
    }
    boot();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!repoRoot) return;
    startProjectWatch(repoRoot);
    let unlisten: (() => void) | undefined;
    listen("project-changed", () => {
      refreshStatus(repoRoot);
    }).then((dispose) => {
      unlisten = dispose;
    });
    const timer = window.setInterval(() => {
      refreshStatus(repoRoot);
    }, 15000);
    return () => {
      window.clearInterval(timer);
      if (unlisten) unlisten();
      stopProjectWatch();
    };
  }, [refreshStatus, repoRoot]);

  useEffect(() => {
    if (!notice && !error) return;
    const timer = window.setTimeout(() => {
      setNotice("");
      setError("");
    }, 4200);
    return () => window.clearTimeout(timer);
  }, [notice, error]);

  const visibleChanges = useMemo(() => status?.changes.filter((change) => !isInternalGitChange(change.path)) ?? [], [status]);
  const groups = useMemo(() => groupChanges(visibleChanges), [visibleChanges]);
  const fileSummary = useMemo(() => summarizeSurfaceChanges(visibleChanges), [visibleChanges]);
  const displayedDiff = useMemo(() => {
    const includeWorkingSummary = history.mode === "single" && !history.selectedCommit && Boolean(selected);
    const parts = [includeWorkingSummary ? fileSummary : "", diff].filter((part) => part.trim().length > 0);
    return parts.join("\n\n");
  }, [diff, fileSummary, history.mode, history.selectedCommit, selected]);
  const diffTitle = useMemo(
    () => makeDiffTitle({ repoRoot, selected, history, status, diff }),
    [repoRoot, selected, history, status, diff],
  );
  const stagedCount = visibleChanges.filter((change) => change.staged).length;
  const dirtyCount = visibleChanges.length;
  const hasConflict = Boolean(status?.mergeInProgress || status?.rebaseInProgress || status?.changes.some((c) => c.conflicted));
  const canReset = canResetSelectedCommit(history, commits, status, busy);

  async function runAction(action: BusyAction, work: () => Promise<void>) {
    setBusy(action);
    setError("");
    setNotice("");
    try {
      await work();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  }

  async function onBindProject() {
    await runAction("bind", async () => {
      const path = await chooseProjectPath();
      if (!path) {
        setNotice("No project selected");
        return;
      }
      const result = await bindProject(path);
      setSelected(null);
      setHistory(initialHistoryState());
      setCommits([]);
      setDiff("Select a changed file to inspect its diff");
      setCommitMessage("");
      setRemoteUrl("");
      setProjectLock({ active: false });
      setBinding(result);
      setConfig((previous) => ({
        ...previous,
        boundProjectPath: result.boundProjectPath,
        repoRoot: result.repoRoot,
        projectName: result.projectName,
      }));
      setProjectName(result.projectName ?? shortPath(result.repoRoot));
      const nextStatus = await repoStatus(result.repoRoot);
      setStatus(nextStatus);
      if (nextStatus.isRepo) {
        setCommits(await listCommits(result.repoRoot));
        setProjectLock(await readProjectLock(result.repoRoot));
      } else {
        setCommits([]);
        setProjectLock({ active: false });
      }
      setNotice(`Bound ${shortPath(result.boundProjectPath)}`);
    });
  }

  async function onChooseCloneDestination() {
    const path = await chooseCloneDestination();
    if (path) setCloneDestination(path);
  }

  function onOpenCloneDialog() {
    setCloneDialogOpen(true);
    setError("");
    setNotice("");
  }

  function onCloseCloneDialog() {
    if (busy === "clone") return;
    setCloneDialogOpen(false);
  }

  async function onCloneProject() {
    await runAction("clone", async () => {
      const result = await cloneProject(cloneUrl, cloneDestination);
      setSelected(null);
      setHistory(initialHistoryState());
      setCommits([]);
      setDiff("Select a changed file to inspect its diff");
      setCommitMessage("");
      setRemoteUrl("");
      setCloneUrl("");
      setCloneDestination("");
      setCloneDialogOpen(false);
      setProjectLock({ active: false });
      setBinding(result);
      setConfig((previous) => ({
        ...previous,
        boundProjectPath: result.boundProjectPath,
        repoRoot: result.repoRoot,
        projectName: result.projectName,
      }));
      setProjectName(result.projectName ?? shortPath(result.repoRoot));
      const nextStatus = await repoStatus(result.repoRoot);
      setStatus(nextStatus);
      if (nextStatus.isRepo) {
        setCommits(await listCommits(result.repoRoot));
        setProjectLock(await readProjectLock(result.repoRoot));
      }
      setNotice(
        result.flpFiles.length === 0
          ? "Clone complete, but no FLP file was found"
          : `Cloned and bound ${shortPath(result.repoRoot)}`,
      );
    });
  }

  async function onSetupRepo() {
    if (!repoRoot) return;
    await runAction("setup", async () => {
      const output = await setupRepo(repoRoot);
      setNotice(output.ok ? "Git LFS and flpdiff setup completed" : output.stderr || output.stdout);
      await refreshStatus(repoRoot);
      const nextPrereqs = await checkPrerequisites(config);
      setPrereqs(nextPrereqs);
    });
  }

  async function onInitializeRepo() {
    if (!repoRoot) return;
    await runAction("init", async () => {
      const name = projectName.trim() || shortPath(repoRoot);
      const output = await initializeProjectRepo(repoRoot, name, "main");
      showOutput(output, "Repo initialized for FL Studio project");
      setConfig((previous) => ({ ...previous, projectName: name }));
      await refreshStatus(repoRoot);
      const nextPrereqs = await checkPrerequisites(config);
      setPrereqs(nextPrereqs);
    });
  }

  async function onSelectChange(change: FileChange) {
    if (!repoRoot) return;
    setSelected(change);
    setHistory(initialHistoryState());
    await runAction("diff", async () => {
      const output = await diffPath(repoRoot, change.path, change.staged && !change.unstaged);
      setDiff(output.stdout.trim() || output.stderr.trim() || "No textual diff was returned");
    });
  }

  async function onSelectCommit(commit: CommitSummary) {
    if (!repoRoot) return;
    const result = handleHistorySingleClick(history, commit, commits);
    setHistory(result.state);
    if (result.action === "compare") {
      await runAction("diff", async () => {
        const output = await diffCommits(repoRoot, result.baseCommit.sha, result.headCommit.sha);
        setDiff(output.stdout.trim() || output.stderr.trim() || "No diff was returned for this commit range");
      });
      return;
    }
    await runAction("diff", async () => {
      const output = selected
        ? await diffRevisionPath(repoRoot, commit.sha, selected.path)
        : await showCommit(repoRoot, commit.sha);
      setDiff(output.stdout.trim() || output.stderr.trim() || "No diff was returned for this revision");
    });
  }

  function onCommitDoubleClick(commit: CommitSummary) {
    setHistory(handleHistoryDoubleClick(commit));
    setDiff(`Select another commit to compare against ${commit.shortSha}`);
  }

  async function onResetToSelectedCommit() {
    if (!repoRoot || !history.selectedCommit || !canReset) return;
    const confirmed = window.confirm(
      `Reset this local repo to ${history.selectedCommit.shortSha}?\n\nThis uses git reset --hard and is only available when the working tree is clean`,
    );
    if (!confirmed) return;
    await runAction("reset", async () => {
      const output = await resetToCommit(repoRoot, history.selectedCommit!.sha);
      showOutput(output, `Reset local repo to ${history.selectedCommit!.shortSha}`);
      setSelected(null);
      setHistory(initialHistoryState());
      await refreshStatus(repoRoot);
    });
  }

  async function onStage(change: FileChange) {
    if (!repoRoot) return;
    await runAction("stage", async () => {
      const output = change.staged
        ? await unstagePaths(repoRoot, [change.path])
        : await stagePaths(repoRoot, [change.path]);
      showOutput(output, change.staged ? "Unstaged file" : "Staged file");
      await refreshStatus(repoRoot);
    });
  }

  async function onCommit() {
    if (!repoRoot) return;
    await runAction("commit", async () => {
      const output = await commitChanges(repoRoot, commitMessage);
      showOutput(output, "Committed staged changes");
      if (output.ok) setCommitMessage("");
      await refreshStatus(repoRoot);
    });
  }

  async function onPull() {
    if (!repoRoot) return;
    await runAction("pull", async () => {
      const output = await pullRepo(repoRoot);
      showOutput(output, "Pulled latest changes");
      await refreshStatus(repoRoot);
    });
  }

  async function onPush() {
    if (!repoRoot) return;
    await runAction("push", async () => {
      const output = await pushRepo(repoRoot);
      showOutput(output, "Pushed commits");
      await refreshStatus(repoRoot);
    });
  }

  async function onSetRemote() {
    if (!repoRoot) return;
    await runAction("push", async () => {
      const output = await setRemote(repoRoot, remoteUrl);
      showOutput(output, "Origin remote saved");
      if (output.ok) setRemoteUrl("");
      await refreshStatus(repoRoot);
    });
  }

  async function onPublishGithub() {
    if (!repoRoot) return;
    await runAction("push", async () => {
      const name = projectName.trim() || shortPath(repoRoot);
      const output = await githubPublishRepo(repoRoot, name, githubVisibility);
      showOutput(output, "GitHub repository created and pushed");
      await refreshStatus(repoRoot);
    });
  }

  async function onAcquireLock() {
    if (!repoRoot) return;
    await runAction("lock", async () => {
      const output = await acquireProjectLock(repoRoot);
      showOutput(output, "Project lock acquired");
      setProjectLock(await readProjectLock(repoRoot));
      await refreshStatus(repoRoot);
    });
  }

  async function onReleaseLock() {
    if (!repoRoot) return;
    await runAction("lock", async () => {
      const output = await releaseProjectLock(repoRoot);
      showOutput(output, "Project lock released");
      setProjectLock(await readProjectLock(repoRoot));
      await refreshStatus(repoRoot);
    });
  }

  async function onResolveConflict(choice: "local" | "remote") {
    if (!repoRoot || !selected) return;
    await runAction("resolve", async () => {
      const output = await resolveConflictPath(repoRoot, selected.path, choice);
      showOutput(output, choice === "local" ? "Resolved using local file" : "Resolved using remote file");
      await refreshStatus(repoRoot);
    });
  }

  async function onAbortMerge() {
    if (!repoRoot) return;
    await runAction("resolve", async () => {
      const output = await abortMergeOrRebase(repoRoot);
      showOutput(output, "Merge/rebase aborted");
      await refreshStatus(repoRoot);
    });
  }

  async function onAnchor() {
    await runAction("anchor", async () => {
      if (isDocked) {
        await undockOverlay();
        setIsDocked(false);
        setNotice("Undocked from FL Studio");
        return;
      }
      const next = await anchorToFlStudio(config.overlayPlacement);
      setFlWindow(next);
      setIsDocked(next.found);
      setNotice(next.found ? "Anchored to FL Studio" : "FL Studio window not found");
    });
  }

  async function onExpandOverlay() {
    await setOverlayExpanded(true);
    setConfig((previous) => ({ ...previous, overlayExpanded: true }));
  }

  async function onCollapseOverlay() {
    overlayDrag.current.dragged = false;
    await setOverlayExpanded(false);
    setConfig((previous) => ({ ...previous, overlayExpanded: false }));
  }

  function onDragControlPointerDown(event: React.PointerEvent<HTMLButtonElement>) {
    overlayDrag.current = { x: event.clientX, y: event.clientY, dragged: false };
  }

  function onWindowDragPointerDown(event: React.PointerEvent<HTMLElement>) {
    const target = event.target as HTMLElement;
    if (target.closest("button,input,select,textarea")) return;
    overlayDrag.current = { x: event.clientX, y: event.clientY, dragged: false };
  }

  function onWindowDragPointerMove(event: React.PointerEvent<HTMLElement>) {
    const target = event.target as HTMLElement;
    if (target.closest("button,input,select,textarea")) return;
    const dx = Math.abs(event.clientX - overlayDrag.current.x);
    const dy = Math.abs(event.clientY - overlayDrag.current.y);
    if (!overlayDrag.current.dragged && dx + dy > 5) {
      overlayDrag.current.dragged = true;
      startOverlayDrag();
    }
  }

  function onDragControlPointerMove(event: React.PointerEvent<HTMLButtonElement>) {
    const dx = Math.abs(event.clientX - overlayDrag.current.x);
    const dy = Math.abs(event.clientY - overlayDrag.current.y);
    if (!overlayDrag.current.dragged && dx + dy > 5) {
      overlayDrag.current.dragged = true;
      startOverlayDrag();
    }
  }

  function onCollapsedButtonClick(event: React.MouseEvent<HTMLButtonElement>) {
    if (overlayDrag.current.dragged) {
      event.preventDefault();
      overlayDrag.current.dragged = false;
      return;
    }
    onExpandOverlay();
  }

  function onCollapseButtonClick(event: React.MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    onCollapseOverlay();
  }

  function showOutput(output: GitOutput, success: string) {
    if (output.ok) {
      setNotice(success);
    } else {
      setError(output.stderr || output.stdout || "Command failed");
    }
  }

  if (!config.overlayExpanded) {
    return (
      <div className="overlay-launcher">
        <button
          className="overlay-open"
          onPointerDown={onDragControlPointerDown}
          onPointerMove={onDragControlPointerMove}
          onClick={onCollapsedButtonClick}
          title="Open FLgit"
        >
          <img className="overlay-icon" src="/icon.ico" alt="" draggable={false} />
          {dirtyCount > 0 && <span className="dirty-dot">{dirtyCount}</span>}
        </button>
      </div>
    );
  }

  return (
    <div className={`app-shell ${isDocked ? "docked" : ""}`}>
      {(notice || error) && (
        <div className="toast-stack">
          {notice && <div className="toast success">{notice}</div>}
          {error && <div className="toast error">{error}</div>}
        </div>
      )}
      <aside className="sidebar">
        <header className="topbar" onPointerDown={onWindowDragPointerDown} onPointerMove={onWindowDragPointerMove}>
          <div className="title-block">
            <div className="app-title">
              <span>FLgit</span>
            </div>
            <p>
              {repoRoot ? shortPath(repoRoot) : "No project bound"}
              {status?.remoteVisibility ? <em className={status.remoteVisibility}>{formatVisibility(status.remoteVisibility)}</em> : null}
            </p>
          </div>
          <button
            className="icon-button"
            onClick={onCollapseButtonClick}
            onPointerDown={(event) => event.stopPropagation()}
            onPointerMove={(event) => event.stopPropagation()}
            disabled={busy !== null}
            title="Collapse"
          >
            <Minimize2 size={16} />
          </button>
          <button className="icon-button close-button" onClick={closeWindow} title="Close FLgit" aria-label="Close FLgit">
            <X size={16} />
          </button>
        </header>

        <section className="toolbar">
          <button onClick={() => refreshStatus()} disabled={!repoRoot || busy !== null} title="Refresh">
            {busy === "refresh" ? <Loader2 className="spin" size={15} /> : <RefreshCw size={15} />}
          </button>
          <button onClick={onBindProject} disabled={busy !== null} title="Bind Project" aria-label="Bind Project">
            <FileCode2 size={15} />
          </button>
          <button onClick={onOpenCloneDialog} disabled={busy !== null} title="Clone Project" aria-label="Clone Project">
            <Download size={15} />
          </button>
          <button
            onClick={onSetupRepo}
            disabled={!repoRoot || busy !== null || hasRepo}
            title="Setup Repo"
            aria-label="Setup Repo"
          >
            <GitCompare size={15} />
          </button>
          <button onClick={onAnchor} disabled={busy !== null} title={isDocked ? "Undock from FL Studio" : "Anchor to FL Studio"} aria-label={isDocked ? "Undock from FL Studio" : "Anchor to FL Studio"}>
            <PanelRight size={15} />
          </button>
        </section>

        <StatusStrip
          prereqs={prereqs}
          status={status}
          flWindow={flWindow}
          dirtyCount={dirtyCount}
          hasConflict={hasConflict}
        />

        {!hasRepo && repoRoot && <section className="init-box">
          <input
            value={projectName}
            onChange={(event) => setProjectName(event.target.value)}
            placeholder="Project/repo name"
          />
          <button onClick={onInitializeRepo} disabled={!repoRoot || busy !== null || projectName.trim().length === 0} title="Initialize Repo" aria-label="Initialize Repo">
            <Save size={15} />
          </button>
        </section>}

        {hasRepo && <section className="sync-row">
          <button
            onClick={onResetToSelectedCommit}
            disabled={!canReset}
          >
            <RotateCcw size={15} />
            Reset
          </button>
          <button onClick={onPull} disabled={!repoRoot || busy !== null}>
            <Download size={15} />
            Pull
          </button>
          <button onClick={onPush} disabled={!repoRoot || busy !== null || (status?.behind ?? 0) > 0}>
            <Upload size={15} />
            Push
          </button>
        </section>}

        {hasRepo && !hasRemote && <section className="remote-box">
          <input
            value={remoteUrl}
            onChange={(event) => setRemoteUrl(event.target.value)}
            placeholder="GitHub remote URL"
          />
          <button onClick={onSetRemote} disabled={!repoRoot || busy !== null || remoteUrl.trim().length === 0} title="Set Origin" aria-label="Set Origin">
            <Cloud size={15} />
          </button>
        </section>}

        {hasRepo && !hasRemote && (
          <section className="github-box">
            <select value={githubVisibility} onChange={(event) => setGithubVisibility(event.target.value as typeof githubVisibility)}>
              <option value="private">Private</option>
              <option value="public">Public</option>
              <option value="internal">Internal</option>
            </select>
            <button onClick={onPublishGithub} disabled={!repoRoot || busy !== null || projectName.trim().length === 0} title="Publish to GitHub" aria-label="Publish to GitHub">
              <ExternalLink size={15} />
            </button>
          </section>
        )}

        {hasRepo && hasRemote && <section className="lock-box">
          <div>
            <strong>{projectLock.active ? "Locked" : "Unlocked"}</strong>
            <span>{projectLock.active ? projectLock.owner ?? "Unknown collaborator" : "No active edit lock"}</span>
          </div>
          {projectLock.active ? (
            <button onClick={onReleaseLock} disabled={!repoRoot || busy !== null} title="Release Edit Lock" aria-label="Release Edit Lock">
              <Unlock size={15} />
            </button>
          ) : (
            <button onClick={onAcquireLock} disabled={!repoRoot || busy !== null} title="Lock Editing" aria-label="Lock Editing">
              <Lock size={15} />
            </button>
          )}
        </section>}

        <section className="changes">
          <div className="section-title section-title-stacked">
            <div>
              <span>Changes</span>
            </div>
            {dirtyCount > 0 && <strong>{dirtyCount}</strong>}
          </div>
          {repoRoot && status?.isRepo && dirtyCount === 0 && <EmptyState text="Working tree clean" />}
          {!repoRoot && <EmptyState text="Bind an FLP or project folder to start version control" />}
          {repoRoot && status && !status.isRepo && <EmptyState text="This folder is not a Git repo, run Setup Repo to initialize it" />}
          {groups.map((group) => (
            <div className="change-group" key={group.category}>
              <div className="group-title">
                <ChevronRight size={14} />
                <span>{group.label}</span>
              </div>
              {group.changes.map((change) => (
                <div
                  className={`change-row ${selected?.path === change.path ? "selected" : ""}`}
                  key={`${change.status}-${change.path}`}
                  onClick={() => onSelectChange(change)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") onSelectChange(change);
                  }}
                  role="button"
                  tabIndex={0}
                >
                  <span className="change-main">
                    <span className="filename">{shortPath(change.path)}</span>
                    <span className="filepath">{change.path}</span>
                  </span>
                  <span className={`badge ${change.conflicted ? "conflict" : ""}`}>{statusLabel(change)}</span>
                  <button
                    className="mini-button"
                    onClick={(event) => {
                      event.stopPropagation();
                      onStage(change);
                    }}
                    title={change.staged ? "Unstage" : "Stage"}
                  >
                    {change.staged ? <Check size={14} /> : <Plus size={14} />}
                  </button>
                </div>
              ))}
            </div>
          ))}
        </section>

        {hasRepo && <section className="commit-box">
          <textarea
            value={commitMessage}
            onChange={(event) => setCommitMessage(event.target.value)}
            placeholder="Commit message"
            rows={3}
          />
          <button onClick={onCommit} disabled={!repoRoot || busy !== null || stagedCount === 0 || commitMessage.trim().length === 0}>
            <GitCommitHorizontal size={15} />
            Commit {stagedCount > 0 ? `${stagedCount}` : ""}
          </button>
        </section>}
      </aside>

      {!isDocked && <main className="diff-panel">
        <div className="diff-header" onPointerDown={onWindowDragPointerDown} onPointerMove={onWindowDragPointerMove}>
          <div>
            <h1>{diffTitle}</h1>
          </div>
          <div className="branch-pill">
            <GitBranch size={14} />
            {status?.branch ?? "No branch"}
          </div>
        </div>
        {hasConflict && (
          <div className="conflict-panel">
            <AlertTriangle size={16} />
            <span>Conflict or rebase state detected. FLP binary merges are manual in v1</span>
            {selected?.conflicted && (
              <div className="conflict-actions">
                <button onClick={() => onResolveConflict("local")} disabled={busy !== null}>Use Local</button>
                <button onClick={() => onResolveConflict("remote")} disabled={busy !== null}>Use Remote</button>
              </div>
            )}
            <button onClick={onAbortMerge} disabled={busy !== null}>Abort</button>
          </div>
        )}
        <pre className="diff-output">{busy === "diff" ? "Loading diff..." : displayedDiff}</pre>
        <section className="history-panel">
          <div className="section-title">
            <span>Commit History</span>
            <strong>{commits.length}</strong>
          </div>
          <div className="history-actions">
            <span>
              {history.mode === "comparePending" && history.compareBase
                ? `Compare from ${history.compareBase.shortSha}`
                : history.mode === "compareResult" && history.compareBase && history.compareHead
                  ? `${history.compareBase.shortSha} <---> ${history.compareHead.shortSha}`
                  : "Double click a commit to compare"}
            </span>
          </div>
          {commits.length === 0 && <EmptyState text="No commits yet" />}
          {commits.map((commit) => (
            <button
              key={commit.sha}
              className={`commit-row ${history.selectedCommit?.sha === commit.sha ? "selected" : ""} ${history.compareBase?.sha === commit.sha || history.compareHead?.sha === commit.sha ? "compare-selected" : ""}`}
              onClick={() => onSelectCommit(commit)}
              onDoubleClick={() => {
                if (history.mode !== "comparePending") onCommitDoubleClick(commit);
              }}
            >
              <span>
                <strong>{commit.subject}</strong>
                <em>{commit.author} - {commit.relativeTime}</em>
              </span>
              <code>{commit.shortSha}</code>
            </button>
          ))}
        </section>
      </main>}

      {cloneDialogOpen && (
        <div className="modal-backdrop" onClick={onCloseCloneDialog}>
          <section className="clone-dialog" onClick={(event) => event.stopPropagation()}>
            <div className="modal-title">
              <div>
                <strong>Clone Project</strong>
              </div>
              <button className="icon-button" onClick={onCloseCloneDialog} disabled={busy === "clone"} title="Close" aria-label="Close Clone Dialog">
                <X size={15} />
              </button>
            </div>
            <label>
              <span>GitHub repo URL</span>
              <input
                value={cloneUrl}
                onChange={(event) => setCloneUrl(event.target.value)}
                placeholder="https://github.com/user/project.git"
                autoFocus
              />
            </label>
            <label>
              <span>Local destination folder</span>
              <div className="clone-destination-row">
                <input
                  value={cloneDestination}
                  onChange={(event) => setCloneDestination(event.target.value)}
                  placeholder="C:\Users\User\FL Projects\project"
                />
                <button onClick={onChooseCloneDestination} disabled={busy === "clone"} title="Choose Destination" aria-label="Choose Destination">
                  <FolderOpen size={15} />
                </button>
              </div>
            </label>
            <div className="modal-actions">
              <button
                onClick={onCloneProject}
                disabled={busy !== null || cloneUrl.trim().length === 0 || cloneDestination.trim().length === 0}
              >
                Clone Project
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

function isInternalGitChange(path: string): boolean {
  const normalized = path.replace(/\\/g, "/");
  const basename = normalized.split("/").pop()?.toLowerCase();
  return (
    basename === ".gitignore" ||
    basename === ".gitattributes" ||
    basename === ".gitmodules" ||
    basename === ".lfsconfig" ||
    normalized.startsWith(".flgit/")
  );
}

function summarizeSurfaceChanges(changes: FileChange[]): string {
  const surfaceChanges = changes.filter((change) => change.category !== "flp");
  if (surfaceChanges.length === 0) return "";
  const lines = ["Surface File Changes:"];
  for (const change of surfaceChanges) {
    const label = statusLabel(change).toLowerCase();
    lines.push(`- ${label}: ${change.path}`);
  }
  return lines.join("\n");
}

function formatVisibility(value: string): string {
  if (value.length === 0) return value;
  return `${value[0].toUpperCase()}${value.slice(1)}`;
}

function StatusStrip({
  prereqs,
  status,
  flWindow,
  dirtyCount,
  hasConflict,
}: {
  prereqs: PrerequisiteStatus | null;
  status: RepoStatus | null;
  flWindow: FlWindowInfo | null;
  dirtyCount: number;
  hasConflict: boolean;
}) {
  const missing = [
    prereqs && !prereqs.gitAvailable ? "Git" : null,
    prereqs && !prereqs.gitLfsAvailable ? "Git LFS" : null,
    prereqs && !prereqs.githubCliAvailable ? "GitHub CLI" : null,
    prereqs && !prereqs.flpdiffAvailable ? "flpdiff" : null,
  ].filter(Boolean);

  return (
    <section className="status-strip">
      <div className="section-title">
            <span>Status</span>
          </div>
      <div className="status-line">
        <GitBranch size={14} />
        <span>{status?.branch ?? "No repo"}</span>
        {status?.ahead ? <em>{status.ahead} ahead</em> : null}
        {status?.behind ? <em>{status.behind} behind</em> : null}
      </div>
      <div className="status-line">
        <ArrowDownUp size={14} />
        <span>{dirtyCount === 0 ? "Clean" : `${dirtyCount} changed`}</span>
        {hasConflict && <em className="bad">Conflict</em>}
      </div>
      <div className="status-line">
        <Cloud size={14} />
        <span>{missing.length ? `Missing ${missing.join(", ")}` : "Git tools ready"}</span>
      </div>
      <div className="status-line">
        <PanelRight size={14} />
        <span>{flWindow?.found ? flWindow.title ?? "FL Studio found" : "FL Studio not detected"}</span>
      </div>
    </section>
  );
}

function EmptyState({ text }: { text: string }) {
  return <div className="empty-state">{text}</div>;
}
