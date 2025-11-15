# git-stk

**One commit, one PR. Ship faster with stacked changes.**

Break down large features into reviewable chunks. Each commit becomes its own pull request, stacked on the previous one.

---

## Installation

### From Source

```bash
git clone https://github.com/derdaele/git-stk.git
cd git-stk
cargo install --path .
```

### Pre-built Binaries

Download pre-built binaries from the [releases page](https://github.com/derdaele/git-stk/releases).

Or use the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/derdaele/git-stk/main/install.sh | bash
```

---

## Quick Start

```bash
# Create a feature branch and make commits
git switch -c my-feature
git commit -m "feat: add user model"
git commit -m "feat: add auth endpoints"
git commit -m "feat: add login UI"

# View your stack
git stk view

# Export to GitHub (creates 3 PRs)
git stk export

# Merge the bottom PR and rebase the stack
git stk land
```

---

## Why Stacked PRs?

Traditional workflows force you to choose between:
- **Large PRs** that are hard to review and slow to merge
- **Multiple branches** that quickly become a merge conflict nightmare

Stacked PRs give you the best of both worlds:
- Small, focused PRs that are easy to review
- Linear commit history that's easy to manage
- Automatic rebasing when the base changes

---

## How It Works

### Commits and Slots

Each commit in your stack gets assigned a **slot** - a unique identifier that determines its branch name on GitHub. By default, slots are auto-assigned as sequential numbers (`01`, `02`, `03`), but you can assign custom slots for better organization:

```bash
# Auto-assigned slots
git stk export
# Creates: my-feature--01, my-feature--02, my-feature--03

# Custom slots
git stk set slot abc1234 PROJ-123
git stk export
# Creates: my-feature--PROJ-123
```

Slots enable git-stk to:
- Track which commit corresponds to which PR
- Update the correct branches when you amend commits
- Handle rebases and commit reordering

### Metadata Storage with Git Notes

All commit metadata (PR numbers, slots, branch names) are stored using **git notes** - a built-in Git feature that attaches arbitrary data to commits without modifying them.

git-stk uses the `refs/notes/git-stk` ref to store JSON metadata for each commit:

```bash
# View raw metadata for a commit
git notes --ref=refs/notes/git-stk show <commit-sha>
```

**Why git notes?**
- **Rebase-safe**: Notes automatically follow commits through rebases and cherry-picks
- **Non-invasive**: Doesn't modify commit messages or SHAs
- **Local-first**: Metadata stays in your local repo, not in commit history
- **Portable**: Standard Git feature, works everywhere

When you rebase or amend commits, Git automatically updates the note references to point to the new commit SHAs, ensuring your PR links stay intact.

---

## Workflow

### 1. Create Your Stack

Build your feature as a series of logical commits:

```bash
git switch -c my-feature
git commit -m "feat: add user model"
git commit -m "feat: add auth endpoints"
git commit -m "feat: add login UI"
```

### 2. View Your Stack

See all commits and their PR status:

```bash
$ git stk view
┌─ main
│
├─● abc1234  feat: add user model  [?→01]
│  <PR to be created>
│
├─● def5678  feat: add auth endpoints  [?→02]
│  <PR to be created>
│
└─● ghi9012  feat: add login UI  [?→03]
   <PR to be created>
```

### 3. Export to GitHub

Create branches and PRs for each commit:

```bash
$ git stk export
Pushing branches...
  ✓ my-feature--01
  ✓ my-feature--02
  ✓ my-feature--03

Creating PRs...
  ✓ PR #42: feat: add user model
  ✓ PR #43: feat: add auth endpoints
  ✓ PR #44: feat: add login UI
```

Each PR is automatically configured to:
- Depend on the previous PR (stacked)
- Include only the changes from its commit
- Link back to your working branch via git notes

### 4. Update Commits

Make changes to any commit in your stack:

```bash
# Fix something in the second commit
git commit --fixup def5678
git rebase -i --autosquash main

# Re-export updates all affected PRs
git stk export
```

git-stk automatically:
- Detects which commits changed
- Force-pushes updated branches
- Updates PR descriptions
- Rebases dependent PRs

### 5. Land PRs

When a PR is approved and merged:

```bash
$ git stk land
Merging PR #42...
  ✓ Merge initiated
  ✓ PR merged successfully!

Pulling latest changes...
  ✓ Updated main

Rebasing stack...
  ✓ Rebased 2 commits on main

Re-exporting...
  ✓ Updated PR #43
  ✓ Updated PR #44
```

The `land` command:
1. Merges the bottom PR
2. Waits for merge completion
3. Pulls the latest main
4. Rebases remaining commits
5. Re-exports the stack

---

## Commands

### `git stk view`

View the current stack of commits and their PR status.

**Options:**
- Read-only, never modifies your repository
- Shows commit SHAs, messages, slots, and PR links
- Displays stack as a tree structure

### `git stk export`

Export the stack to GitHub by creating/updating branches and PRs.

**Options:**
- `--draft` - Create PRs as drafts
- `--ready` - Mark PRs as ready for review
- `--open` - Open created/updated PRs in browser
- `--dry-run` - Show what would be done without making changes

### `git stk land`

Merge the bottom PR, wait for completion, rebase stack, and re-export.

**Options:**
- `--skip-wait` - Don't wait for merge to complete

### `git stk landed`

Run post-merge operations after a PR was manually merged outside of git-stk. Pulls changes, rebases, and re-exports.

### `git stk set slot <commit> <slot>`

Manually assign a custom slot to a commit.

**Arguments:**
- `<commit>` - Commit SHA, stack index (1, 2, 3), or git ref (HEAD, branch name)
- `<slot>` - Custom slot identifier (alphanumeric with hyphens/underscores)

**Options:**
- `-y, --yes` - Skip confirmation prompts

**Note:** Changing a slot for a commit with an existing PR will close that PR and create a new one on the next export (GitHub PR head refs are immutable).

---

## FAQ

### What happens if I squash commits?

When squashing commits during an interactive rebase:
- The metadata (slot and PR number) from the **earliest commit** is preserved
- Any PRs for the squashed-away commits become orphaned and must be manually closed
- Run `git stk view` after squashing to review your stack state

### Can I reorder commits?

Yes! git notes survive rebases, including commit reordering. git-stk will detect the changes and update the appropriate branches and PRs on the next `export`.

### What if I amend a commit?

Amending a commit creates a new commit SHA, but git automatically moves the note to the new commit. Run `git stk export` to push the changes and update the PR.

### How do I delete a commit from the stack?

Use interactive rebase to drop the commit:

```bash
git rebase -i main  # Mark commit as 'drop'
git stk export      # Updates remaining PRs
```

Manually close the orphaned PR on GitHub.

### Can I use git-stk with an existing branch?

Yes! git-stk works with any branch. Just run `git stk export` and it will analyze your commits and create PRs for any that don't have them yet.

---

## Configuration

git-stk stores configuration in `.git/config` under the `[stk]` section:

```toml
[stk]
    # Remote to push branches to (default: origin)
    remote = origin

    # Base branch for PRs (default: main)
    base = main
```

You can also set these via git config:

```bash
git config stk.remote upstream
git config stk.base develop
```

---

## Technical Details

### GitHub Authentication

git-stk uses the GitHub CLI (`gh`) for authentication. Make sure you're logged in:

```bash
gh auth login
```

Alternatively, set the `GITHUB_TOKEN` environment variable.

### Branch Naming

Branches follow the format: `{your-branch}--{slot}`

Examples:
- `my-feature--01`, `my-feature--02`, `my-feature--03` (auto-assigned)
- `my-feature--PROJ-123`, `my-feature--auth-refactor` (custom slots)

### PR Dependencies

git-stk configures PR base branches to create dependencies:
- PR #1 → targets `main`
- PR #2 → targets branch from PR #1
- PR #3 → targets branch from PR #2

When PR #1 merges, git-stk rebases the stack and updates PR #2 to target `main`.
