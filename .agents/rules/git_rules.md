---
trigger: always_on
---

# Git Workflow & Commit Guidelines

Follow these safety rules when executing Git commands in the workspace:

1. **No Automatic Git Push**: Never execute `git push` to GitHub or remote repositories unless explicitly requested or approved by the user.
2. **Conventional Single-Line Commit Format**: Always keep commit messages to strictly **ONE LINE** (no multi-line descriptions or bullet points). Format using conventional commit style:
   - `feat(scope): <concise summary>`
   - `fix(scope): <concise summary>`
   - `refactor(scope): <concise summary>`
   - `perf(scope): <concise summary>`
   - `test(scope): <concise summary>`
   - `docs(scope): <concise summary>`
3. **Cumulative Pull Request Summaries**: When creating or updating a Pull Request for a branch that builds on top of other unmerged branches, always inspect the full commit log against the target branch (e.g., `git log origin/main..HEAD`) and ensure all changes, features, and commits included in the PR range are explicitly documented in the Pull Request title and summary.
4. **Clean Staging & No Scratch Files**: Always inspect `git status` before `git add` to avoid staging temporary debug scripts, scratch files, core dumps, logs, or unneeded binary outputs. Keep repository trees clean.
5. **No Destructive Operations**: Never execute destructive commands (`git reset --hard`, `git clean -fd`, `git checkout -- .`, `git push --force`) without explicit confirmation from the user to prevent accidental data or work loss.
6. **Descriptive Branch Naming**: Name feature and fix branches with clear semantic prefixes:
   - `feat/<feature-name>` (e.g. `feat/operators-and-collection-iteration`)
   - `fix/<bug-description>` (e.g. `fix/const-assignment-panic`)
   - `refactor/<subsystem>` (e.g. `refactor/vendor-native-deps`)
7. **Pre-Commit Verification**: Ensure the codebase builds cleanly (`cargo build`) and tests pass before committing to ensure the commit history remains in a green, working state.
8. **Credential & Secret Protection**: Never commit environment files (`.env`), API keys, access tokens, credentials, or private keys to the repository. Ensure sensitive files remain in `.gitignore`.
