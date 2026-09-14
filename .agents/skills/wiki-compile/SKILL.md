---
name: wiki-compile
description: Parse raw logs to extract, synthesize, and deprecate project knowledge. Generates high-density grounded wiki pages and maintains the central index.
skill-version: 2
---
# When to use
- After accumulating raw logs in doc/raw/
- When establishing project-wide rules or identifying recurring failures
- Periodically to restructure the knowledge graph based on new evidence

# Action

## Phase 0: Preparation (Automation)
1. **Identify New Logs**: Run `llm-wiki compile list`. Detection is date **OR** content-hash
   based (`doc/wiki/compile-state.json`) — same-day appends to an already-compiled log are
   caught too. The listed logs are a real to-do list for Phases 1–3, not a formality.
2. **Context Loading**: Read the identified logs and the current `doc/wiki/index.md`.

## Phase 1: High-Density Extraction (LLM)
1. For each Case, extract:
   - **Grounding**: Git hash, Confidence score (1-5), Evidence links.
   - **Analytical Core**: The "Why" (First Principles) and "Trade-offs".
   - **Taxonomy**: Concepts, Patterns, Anti-patterns.
2. Merge similar cases, favoring the one with the highest **Confidence** and most recent **Git Hash**.
3. **Disposition (MANDATORY, per Case)** — classify BEFORE writing any page, and state it in your output:
   - **New**: no existing page covers this → create a page.
   - **Update**: extends an existing page → edit that page, append the Case to its Grounding.
   - **Merge**: duplicates/splits an existing page → fold into it; do not spawn a near-duplicate.
   - **No material**: the Case is a one-off with no reusable lesson → skip it. Raw log is its archive.
   A wiki that grows a page per Case diverges; thin pages dilute search. `No material` is a
   first-class outcome, not a failure.

## Phase 2: Knowledge Promotion & Synthesis (LLM)
Synthesize extracted data into structured wiki pages. Use the following high-density templates:

**Verbatim rule**: quote error messages character-for-character from the raw log — never
paraphrase. `llm-wiki lint` back-matches every `### Error` quote against `doc/raw/`; a
summarized quote is reported as an evidence violation.

### Wiki Template (Concept)
```markdown
---
status: active | deprecated | draft
version_context: "e.g., Library X x.y"
tags: [domain, concept]
aliases: [synonyms]
created: YYYY-MM-DD
confidence: [1-5]
---
# [Concept Name]
[1-2 sentence summary]
## First Principles
## Details
## Related
## Grounding (References)
```

### Wiki Template (Pattern)
```markdown
---
status: active | deprecated | draft
version_context: "e.g., Framework X x.y"
tags: [domain, pattern]
aliases: [synonyms]
created: YYYY-MM-DD
confidence: [1-5]
---
# [Pattern Name]
[1-2 sentence summary]
## The Rule
## Why it works
## Trade-offs
## Anti-Pattern
## Related
```

Anti-pattern pages use `tags: [domain, anti-pattern]` and document the failure mode + prevention checklist.

## Phase 3: Conflict Resolution & Deprecation (LLM)
1. **Contradiction Detection**: If a new Case contradicts an old wiki page, mark the old one `status: deprecated` and add `superseded_by: [[new-page]]`.
2. **Merging**: If a new Case extends an old page, update the page and append the new Case to "Grounding (References)".

## Phase 4: Indexing & Health (Automation)
1. **Rebuild Index**: Run `llm-wiki compile index`. This automatically:
   - Scans all files in `doc/wiki/` (including loose root files, classified by `tags`).
   - Rebuilds the tables in `index.md`.
   - Updates `Statistics` and `Last updated` date.
   - Syncs the QMD search index (creates wiki + raw collections if missing, re-indexes, refreshes embeddings).
   - Regenerates `doc/wiki/compile-state.json` (per-file hashes for same-day append detection).

⚠️ **`compile index` is a "compile complete" declaration.** Running only Phase 4 without
Phases 1–3 seals every raw log up to now as *compiled*. If the state file ever merges
badly or goes missing, the resolution is trivial: keep either side (or delete it) and run
`compile index` once — it is regenerated deterministically from the current `doc/raw/`.

---

# QMD (semantic search) notes

`llm-wiki compile index` syncs the QMD search index when `@tobilu/qmd` is installed (optional). Without QMD, the wiki index still rebuilds and `llm-wiki search` falls back to grep.

## GPU vs CPU mode (`QMD_LLAMA_GPU`)

> **Variable name is `QMD_LLAMA_GPU`** — not `NODE_LLAMA_CPP_GPU`. The latter is read by nothing in the codebase and has no effect.

- Default: GPU used automatically when available.
- To force CPU (e.g., transient CUDA driver issue, VRAM pressure, or NVIDIA GPU asleep):
  ```bash
  QMD_LLAMA_GPU=false llm-wiki compile index
  ```
- For a permanent override on a problematic machine, set it in your shell profile or a local `.env`.

## Troubleshooting build-time CUDA failures

Detailed CUDA Toolkit / MSVC / node-llama-cpp prebuilt-binary diagnostics (Windows + NVIDIA specific) live in **[TROUBLESHOOTING.md](../TROUBLESHOOTING.md)** in the package root. Consult it only if `compile index` prints `falling back to using Vulkan` or `CUDA error` messages.
