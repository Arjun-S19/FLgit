export type ChangeCategory = "flp" | "samples" | "exports" | "metadata" | "other";

export interface FileChange {
  path: string;
  originalPath?: string | null;
  status: string;
  staged: boolean;
  unstaged: boolean;
  conflicted: boolean;
  category: ChangeCategory;
}

export interface ChangeGroup {
  category: ChangeCategory;
  label: string;
  changes: FileChange[];
}

const labels: Record<ChangeCategory, string> = {
  flp: "FL Studio Projects",
  samples: "Samples",
  exports: "Exports",
  metadata: "Metadata",
  other: "Other Files",
};

const order: ChangeCategory[] = ["flp", "samples", "exports", "metadata", "other"];

export function groupChanges(changes: FileChange[]): ChangeGroup[] {
  return order
    .map((category) => ({
      category,
      label: labels[category],
      changes: changes.filter((change) => change.category === category),
    }))
    .filter((group) => group.changes.length > 0);
}

export function statusLabel(change: FileChange): string {
  if (change.conflicted) return "Conflict";
  if (change.status.includes("?")) return "Unstaged";
  if (change.status.includes("D")) return "Deleted";
  if (change.status.includes("A")) return "Staged";
  if (change.status.includes("R")) return "Renamed";
  if (change.status.includes("M")) return "Modified";
  return change.status.trim() || "Changed";
}

export function shortPath(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
