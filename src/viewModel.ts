import { shortPath, type FileChange } from "./domain";
import type { CommitSummary, RepoStatus } from "./api";

export type HistoryMode = "single" | "comparePending" | "compareResult";

export interface HistoryState {
  mode: HistoryMode;
  selectedCommit: CommitSummary | null;
  compareBase: CommitSummary | null;
  compareHead: CommitSummary | null;
}

export type HistoryClickResult =
  | { action: "single"; state: HistoryState; commit: CommitSummary }
  | { action: "compare"; state: HistoryState; baseCommit: CommitSummary; headCommit: CommitSummary };

export function initialHistoryState(): HistoryState {
  return {
    mode: "single",
    selectedCommit: null,
    compareBase: null,
    compareHead: null,
  };
}

export function handleHistorySingleClick(
  current: HistoryState,
  commit: CommitSummary,
  commits: CommitSummary[],
): HistoryClickResult {
  if (current.mode === "comparePending" && current.compareBase) {
    if (current.compareBase.sha === commit.sha) {
      return {
        action: "single",
        commit,
        state: {
          mode: "single",
          selectedCommit: commit,
          compareBase: null,
          compareHead: null,
        },
      };
    }
    const [baseCommit, headCommit] = orderCommitPair([current.compareBase, commit], commits);
    return {
      action: "compare",
      baseCommit,
      headCommit,
      state: {
        mode: "compareResult",
        selectedCommit: headCommit,
        compareBase: baseCommit,
        compareHead: headCommit,
      },
    };
  }

  return {
    action: "single",
    commit,
    state: {
      mode: "single",
      selectedCommit: commit,
      compareBase: null,
      compareHead: null,
    },
  };
}

export function handleHistoryDoubleClick(commit: CommitSummary): HistoryState {
  return {
    mode: "comparePending",
    selectedCommit: commit,
    compareBase: commit,
    compareHead: null,
  };
}

export function canResetSelectedCommit(
  history: HistoryState,
  commits: CommitSummary[],
  status: RepoStatus | null,
  busy: string | null,
): boolean {
  if (busy !== null || history.mode !== "single" || !history.selectedCommit || !status?.isRepo) return false;
  if (status.mergeInProgress || status.rebaseInProgress) return false;
  if (status.changes.length > 0) return false;
  return commits.findIndex((commit) => commit.sha === history.selectedCommit?.sha) > 0;
}

export function makeDiffTitle({
  repoRoot,
  selected,
  history,
  status,
  diff,
}: {
  repoRoot: string | null;
  selected: FileChange | null;
  history: HistoryState;
  status: RepoStatus | null;
  diff: string;
}): string {
  if (selected?.conflicted || status?.changes.some((change) => change.conflicted && change.path === selected?.path)) {
    return selected ? `Conflict Diff for ${shortPath(selected.path)}` : "Conflict Diff";
  }
  if (history.mode === "comparePending" && history.compareBase) {
    return `Select Commit to Compare Against ${history.compareBase.shortSha}`;
  }
  if (history.mode === "compareResult" && history.compareBase && history.compareHead) {
    return `Semantic Diff Between ${history.compareBase.shortSha} and ${history.compareHead.shortSha}`;
  }
  if (history.selectedCommit) {
    if (diff.trim().startsWith("This is the first commit")) {
      return `Initial Commit ${history.selectedCommit.shortSha}`;
    }
    if (selected) {
      return `${isFlpPath(selected.path) ? "Semantic Diff" : "Diff"} For ${shortPath(selected.path)} At ${history.selectedCommit.shortSha}`;
    }
    return `Commit Diff for ${history.selectedCommit.shortSha}`;
  }
  if (selected) {
    return `${isFlpPath(selected.path) ? "Semantic Diff" : "Diff"} For ${shortPath(selected.path)}`;
  }
  if (!repoRoot || !status?.isRepo) {
    return "Semantic Diff";
  }
  return "Semantic Diff";
}

export function orderCommitPair(pair: [CommitSummary, CommitSummary], commits: CommitSummary[]): [CommitSummary, CommitSummary] {
  const firstIndex = commits.findIndex((commit) => commit.sha === pair[0].sha);
  const secondIndex = commits.findIndex((commit) => commit.sha === pair[1].sha);
  if (firstIndex === -1 || secondIndex === -1) return pair;
  return firstIndex > secondIndex ? [pair[0], pair[1]] : [pair[1], pair[0]];
}

export function isFlpPath(path: string): boolean {
  return path.toLowerCase().endsWith(".flp");
}
