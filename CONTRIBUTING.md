# Contributing to ForgeP2P

The single most valuable thing you can contribute is a new entry in [`library-feedback.md`](library-feedback.md) — the shared log of SwarmNL API papercuts, surprises, and workarounds. Every entry saves the next person (or the next agent build) from rediscovering the same issue.

## Found a SwarmNL papercut?

If you hit something — an undocumented behavior, a signature mismatch, a missing error variant, a timing quirk — please PR an entry. You do not need to fix SwarmNL; just document what happened and what you did about it. That record is already enough to help everyone else.

You don't need to be using this repo to contribute. If you built something on top of SwarmNL anywhere, feedback is still welcome here.

## The 60-second PR recipe (from a fork)

A PR that touches **only** `library-feedback.md` is the fastest to review and merge. If you've been building an app in a fork, your fork will contain extra artifacts (your `apps/<name>/` tree, `forge-state.toml`, etc.) — those stay in your fork. Only the feedback entry travels upstream.

```bash
# 1. Make sure you have an "upstream" remote pointing at the root repo.
git remote add upstream https://github.com/<root>/forge-p2p.git
git fetch upstream

# 2. Branch off a clean upstream/main so nothing from your fork sneaks in.
git checkout -b feedback/<short-title> upstream/main

# 3. Pull your new entry over from wherever you logged it.
#    (If you edited library-feedback.md directly on your dev branch, this
#    copies the whole file — then hand-edit to keep ONLY the new block.)
git checkout <your-app-branch> -- library-feedback.md
# edit: strip out anything that was already on upstream/main.

git add library-feedback.md
git commit -m "feedback: <one-line summary>"
git push origin feedback/<short-title>

# 4. Open a PR against <root>/forge-p2p:main.
```

That's it. If the PR diff contains anything other than `library-feedback.md`, expect a request to split it.

## What belongs in an entry

Use the format already in [`library-feedback.md`](library-feedback.md):

```markdown
## [YYYY-MM-DD] <app-or-project-name> — <short title>

- **Context**: What you were trying to build when you hit this.
- **Problem**: What went wrong. Be specific — paste the signature or error.
- **Suggestion**: What SwarmNL could do differently.
- **Workaround**: What you did instead, so the next reader can ship.
- **Severity**: `nice-to-have` | `important` | `blocking`
- **Relevant API**: `AppData::…` / `CoreBuilder::…` / etc. (optional but useful)
```

Keep it factual. Workarounds are the most-read part of each entry — make sure yours is copy-pasteable.

## Building an example app in a fork?

Go ahead. Forks used for agentic app-building will diverge from upstream — `apps/<your-app>/`, a populated `forge-state.toml`, your own `decisions.md`, maybe new deps. None of that is expected to come back upstream.

Only your `library-feedback.md` additions are. The recipe above is designed around that: branch off `upstream/main` (not your fork's `main`), and only carry the feedback diff across.

If you do want to propose a *new* example app for the root repo, open an issue first so we can discuss scope and naming before you invest in it.

## Other changes

Bug fixes to `shared/forge-ui/`, clarifications to `CLAUDE.md`, fixes to `.forge/swarm-nl-reference.md`, etc. — PRs welcome. Run `cargo test` and `cargo clippy --all-targets -- -D warnings` before pushing.
