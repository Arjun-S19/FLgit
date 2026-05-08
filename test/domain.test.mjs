import assert from "node:assert/strict";
import { test } from "node:test";

const labels = {
  flp: "FL Studio Projects",
  samples: "Samples",
  exports: "Exports",
  metadata: "Metadata",
  other: "Other Files",
};

function groupChanges(changes) {
  return ["flp", "samples", "exports", "metadata", "other"]
    .map((category) => ({
      category,
      label: labels[category],
      changes: changes.filter((change) => change.category === category),
    }))
    .filter((group) => group.changes.length > 0);
}

test("groups changes in source-control display order", () => {
  const grouped = groupChanges([
    { path: "notes.md", category: "metadata" },
    { path: "track.flp", category: "flp" },
    { path: "Samples/kick.wav", category: "samples" },
  ]);

  assert.deepEqual(
    grouped.map((group) => group.category),
    ["flp", "samples", "metadata"],
  );
});
