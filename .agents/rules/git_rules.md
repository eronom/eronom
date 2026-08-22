---
trigger: always_on
---

# Git Workflow & Commit Guidelines

Follow these safety rules when executing Git commands in the workspace:

1. **No Automatic Git Push**: Never execute `git push` to GitHub or remote repositories unless explicitly requested or approved by the user.
2. **Conventional Commit Format**: Format all commit messages using standard conventional commit style:
   - `feat(scope): ...` for new capabilities.
   - `fix(scope): ...` for bug fixes.
   - `docs(agents): ...` for agent memory and documentation updates.
3. **Cumulative Pull Request Summaries**: When creating or updating a Pull Request for a branch that builds on top of other unmerged branches, always inspect the full commit log against the target branch (e.g., `git log origin/main..HEAD`) and ensure all changes, features, and commits included in the PR range are explicitly documented in the Pull Request title and summary.
