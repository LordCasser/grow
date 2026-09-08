## Why
The rewind mode picker checks only the selected prompt's has_file_changes. Actual rewind restores checkpoints at or after the target. Selecting a prompt without its own changes therefore hides FilesOnly even when later prompts changed files that would be rewound.

## What Changes
Determine mode availability from the target-and-later range, both when selecting a target and when returning from preview. Preserve per-prompt count labels, target selection fallback and inline edit's intentional FilesOnly exclusion.
