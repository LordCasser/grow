# Project Instructions (`AGENTS.md`)

Project instructions teach Grow the conventions, build commands, safety constraints, and architecture of a directory tree. Grow has two canonical sources:

- `AGENTS.md` for directory-scoped instructions;
- `.grow/rules/*.md` for a sorted collection of rules at the same scope.

No alternate vendor filenames or vendor configuration directories are scanned.

---

## Scope and precedence

An `AGENTS.md` applies to the directory where it lives and every descendant. A `.grow/rules/*.md` file has the same directory scope as its containing `.grow` directory.

Inside a git worktree, Grow walks from the repository root to the session working directory. Outside git, it inspects only the working directory. User-global instructions load from `$GROW_HOME/AGENTS.md` and `$GROW_HOME/rules/*.md` first.

Files load in this order:

1. `$GROW_HOME/AGENTS.md`;
2. `$GROW_HOME/rules/*.md`, sorted by filename;
3. each project directory from repository root to current working directory;
4. that directory's `AGENTS.md`, followed by `.grow/rules/*.md` sorted by filename.

Deeper files appear later and therefore win when two instructions conflict. Direct user instructions still take precedence over repository files.

---

## Example

```text
my-app/
  AGENTS.md
  .grow/
    rules/
      10-style.md
      20-testing.md
  src/
    AGENTS.md
    components/
      AGENTS.md
```

Starting Grow in `src/components` loads the root instructions and rules, then `src/AGENTS.md`, then `src/components/AGENTS.md`.

---

## Dynamic discovery

The initial Timeline projection contains every instruction visible from the session working directory. If a tool later accesses another subtree in the same worktree, Grow checks the newly entered ancestor chain for applicable `AGENTS.md` and `.grow/rules` files.

Newly discovered paths are announced once through the project-instruction tracker. This is the same canonical discovery model used at startup; tools do not implement vendor-specific or read-only injection paths.

Files ignored by the repository's `.gitignore` are skipped.

---

## Rule file frontmatter

Rule files may use YAML frontmatter for metadata. Grow removes the frontmatter before adding the rule body to model context:

```markdown
---
description: Rust formatting rules
---

Run rustfmt only on files changed by the task.
```

`AGENTS.md` is delivered verbatim; leading Markdown horizontal rules are not treated as frontmatter.

---

## Recommended content

Keep instructions operational and verifiable:

```markdown
# Repository constraints

- Rust edition: 2024
- Format only changed files with `rustfmt --edition 2024 <files...>`
- Run package-local tests before workspace-wide tests
- Do not edit generated protocol bindings by hand
- New service code belongs under `crates/service/`
```

Useful categories include:

- build and test commands;
- directory ownership and dependency boundaries;
- code-generation rules;
- security-sensitive files;
- formatting and naming conventions;
- required validation before a milestone.

Avoid generic advice that the model already knows. Prefer repository facts and commands whose correctness can be checked.

---

## Inspecting loaded instructions

Run:

```bash
grow inspect
```

The report lists each loaded instruction file, scope, byte size, approximate token cost, and whether it came from `AGENTS.md` or `.grow/rules`.

The Trajectory debug page shows the durable Timeline events that publish and revise project-instruction context for a session.
