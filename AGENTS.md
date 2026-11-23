
# Arcanum-RS — Codex Instructions

## Development workflow

- Focus on implementation, debugging, testing and benchmarks.
- Do not inspect Git state at the beginning of a task.
- Do not run Git commands unless explicitly requested or strictly necessary for a specific development issue.
- Never commit, push, merge, rebase, reset, create branches or open pull requests unless explicitly instructed.
- Assume the current working branch is already prepared.
- Leave implementation changes uncommitted for review.
- Minimize unnecessary repository administration, planning and status narration.

## Engineering principles

- Arcanum-RS is an independent native Rust engine.
- Arcanum-CE and TIG are read-only implementation references.
- Preserve compatibility with original game data.
- Never commit original copyrighted game assets.
- Prefer reusable engine systems over milestone-specific demonstrations.
- Avoid speculative fixes, arbitrary offsets and hardcoded object positions.
- Prefer targeted investigation and minimal verified changes.
- Preserve existing regression tests and performance baselines.
- Keep the renderer efficient on low-end hardware.

## Completion reports

- Summarize implemented changes and verified behavior.
- Include relevant test results, measurements and remaining limitations.
- Provide screenshots for visual rendering changes.
- Do not perform Git integration.

