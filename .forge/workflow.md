# ForgeP2P Agent Workflow

> This document defines how the coding agent operates. It is the source of truth
> for all workflow rules. Read it completely before any action.

## 1. Session Initialization

Every session begins with this exact sequence:

```
1. Read CLAUDE.md                          -- project context
2. Read .forge/registry.toml               -- what apps exist, their status
3. Read library-feedback.md                -- known issues from previous builds
4. IF resuming an app:
     Read apps/<n>/forge-state.toml        -- where you left off
     Read apps/<n>/plan.toml               -- what comes next
     Run: git status && git log --oneline -5
5. IF starting a new app:
     Read .forge/templates/plan.toml       -- plan format
     Read .forge/swarm-nl-reference.md     -- API patterns
```

Never skip steps. Never assume state from a previous session.

Step 3 is mandatory. `library-feedback.md` contains verified workarounds for
issues discovered during previous builds. Apply them proactively in your code.
Do not rediscover the same problems.

## 2. The Execution Loop

For each step in a plan:

```
+---------------------------------------------+
|  READ: forge-state.toml -- current step      |
|  READ: plan.toml -- step spec                |
+----------------------------------------------+
|  IMPLEMENT: write code for this step only    |
|  (max ~100 lines of new code)                |
+----------------------------------------------+
|  VALIDATE (all must pass to advance):        |
|    1. cargo check          (compiles?)       |
|    2. cargo clippy          (no warnings?)   |
|    3. cargo test            (tests pass?)    |
+----------------------------------------------+
|  IF ALL PASS:                                |
|    -- Update forge-state.toml                |
|    -- Update decisions.md if choices were made|
|    -- git add -A && git commit               |
|    -- Continue to next step                  |
|                                              |
|  IF ANY FAIL:                                |
|    -- Fix and re-validate (max 3 attempts)   |
|    -- After 3 failures:                      |
|       - Set status = "blocked" in state      |
|       - Log blocker details in state         |
|       - If library issue: log in             |
|         library-feedback.md AND              |
|         sync feedback to main (see sec 6)    |
|       - STOP and report to user              |
+----------------------------------------------+
|  IF STEP IS LAST:                            |
|    -- Run full validation suite              |
|    -- Update registry.toml status -- complete|
|    -- Write/update app README.md             |
|    -- Sync feedback to main (see sec 6)      |
|    -- Final commit on dev branch             |
+----------------------------------------------+
```

## 3. State Management

### forge-state.toml (Machine-Readable)

This is the ONLY file the agent reads to determine where it is. It uses TOML
because TOML is unambiguous to parse and hard to accidentally corrupt.

```toml
[app]
name = "echo-gossip"
branch = "dev/echo-gossip"
created = "2026-04-12T10:00:00Z"

[progress]
total_steps = 5
current_step = 3
status = "in-progress"  # "not-started" | "in-progress" | "blocked" | "complete"
last_completed_step = 2
last_updated = "2026-04-12T14:30:00Z"

[current_step]
title = "Implement gossip broadcast"
attempt = 1  # resets to 1 on each new step, increments on failure
files_modified = ["src/main.rs", "src/gossip.rs"]

[blocker]
# Only populated when status = "blocked"
step = 0
description = ""
is_library_issue = false

[validation]
last_cargo_check = "pass"   # "pass" | "fail" | "not-run"
last_cargo_clippy = "pass"
last_cargo_test = "pass"
```

### decisions.md (Human-Readable)

For design decisions that matter to a human reviewer. Append-only log.

```markdown
## Step 2 -- Chose gossip over RPC for state sync
- **Context**: Needed to broadcast game state to all peers
- **Options**: Gossip (fan-out) vs RPC (point-to-point loop)
- **Decision**: Gossip -- fewer round trips, SwarmNL handles fan-out
- **Trade-off**: No delivery guarantee per-peer
```

### registry.toml (Repo-Level Catalog)

```toml
# Each app gets an entry. The agent updates this when creating or completing apps.

[[apps]]
name = "echo-gossip"
description = "Peers echo gossip messages back to the network"
status = "complete"      # "planned" | "in-progress" | "blocked" | "complete"
branch = "dev/echo-gossip"
pattern = "gossip"       # primary SwarmNL pattern used
steps_total = 5
steps_done = 5
created = "2026-04-12"

[[apps]]
name = "file-index"
description = "Distributed file index using DHT with RPC retrieval"
status = "in-progress"
branch = "dev/file-index"
pattern = "dht+rpc"
steps_total = 8
steps_done = 3
created = "2026-04-13"
```

## 4. Planning Phase

When asked to plan a new app:

1. Generate `plan.toml` using the template at `.forge/templates/plan.toml`
2. Each step must specify:
   - `title` -- what this step does
   - `files` -- which files are created or modified
   - `apis` -- which SwarmNL types/methods are used
   - `test_criteria` -- concrete pass/fail description
   - `test_command` -- exact command to verify (default: `cargo test`)
3. Steps must be ordered so each builds on the last
4. Step 1 is ALWAYS: scaffold + basic node boots + prints PeerID
5. Final step is ALWAYS: integration test with multiple nodes
6. Add an entry to `registry.toml` with status = "planned"
7. Present the plan for user review BEFORE writing any code

## 5. Branching and Commits

```
main                         <-- templates, reference docs, library-feedback.md
 +-- dev/echo-gossip         <-- all work for this app
 +-- dev/file-index          <-- all work for this app
```

### Commit Convention
```
forge: plan echo-gossip
forge: echo-gossip step 1 -- scaffold and basic node
forge: echo-gossip step 2 -- gossip join and broadcast
forge: echo-gossip fix -- port conflict in test
forge: echo-gossip complete -- final validation
forge: feedback -- missing AppData variant docs
```

### Rollback
If a step corrupts state beyond repair:
```bash
git log --oneline -10                    # find last good commit
git reset --hard <commit>               # reset to it
# Update forge-state.toml to match the rolled-back state
```

Document the rollback in decisions.md.

## 6. Library Feedback

`library-feedback.md` is a shared knowledge base that lives on `main` and is
read at the start of every session. It accumulates across all app builds so the
agent never rediscovers the same issue twice.

### When to log feedback

When you encounter:
- An API that behaves differently than documented
- A missing feature that forced a workaround
- A bug or unexpected behavior
- A pattern that should be in the library but isn't

### Format

```markdown
## [YYYY-MM-DD] <app> -- <title>
- **Context**: What you were building
- **Problem**: What went wrong
- **Suggestion**: How the library could improve
- **Workaround**: What you did instead
- **Severity**: nice-to-have | important | blocking
- **Relevant API**: AppData::XYZ / CoreBuilder::with_foo / etc.
```

Also set `is_library_issue = true` in forge-state.toml if it's a blocker.

### Syncing feedback to main

After logging a new entry in `library-feedback.md`, sync it to `main` so
the next app build has access to it:

```bash
# While on dev/<app-name> branch:
git add library-feedback.md
git stash -- library-feedback.md       # stash just the feedback file
git checkout main
git stash pop                          # apply feedback to main
git add library-feedback.md
git commit -m "forge: feedback -- <brief description>"
git checkout dev/<app-name>            # return to dev branch
```

Do this:
- Immediately when logging a blocking issue
- At app completion (final step)
- When explicitly asked by the user

If the `swarm-nl-reference.md` also needs a correction based on the finding,
update it on `main` in the same commit.

## 7. Code Standards

- `anyhow::Result` for binaries, `thiserror` for libraries
- All public functions have `///` doc comments
- Tests use unique ports: `49000 + (test_index * 100) + (node_index * 10)`
- Tests wrap async work in `tokio::time::timeout(Duration::from_secs(30), ...)`
- Print `PeerId` and listen addresses at startup
- Use `tracing` or `println!` for structured logging
- Organize code into modules when a file exceeds ~200 lines

## 8. What the Agent Must Never Do

- Write code before reading the plan, state, and library-feedback.md
- Skip validation steps (check, clippy, test)
- Advance past a failing step
- Modify files outside the current app's directory (except registry.toml,
  library-feedback.md, and swarm-nl-reference.md on main when syncing feedback)
- Commit app code to `main`
- Delete or overwrite another app's directory
- Assume the library works a certain way without checking the reference doc
  AND library-feedback.md for known issues