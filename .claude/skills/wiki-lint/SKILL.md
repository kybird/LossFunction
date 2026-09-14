---
name: wiki-lint
description: Validate knowledge graph integrity, detect logical conflicts, and generate a professional health report with Mermaid visualizations.
skill-version: 1
---
# When to use
- Before starting a major feature or refactor
- After a `wiki-compile` run to ensure quality
- Periodically to prune dead or contradictory knowledge

# Action

## Phase 1: Automated Integrity Check (Automation)
1. **Run Linter**: Run `llm-wiki lint`.
2. **Review Output**:
   - **Broken Links**: List of `[[wikilinks]]` that point to non-existent files.
   - **Missing Metadata**: Files missing required YAML fields or headers.
   - **Version Drift**: Discrepancies between the project package manifest (package.json / pubspec.yaml / Cargo.toml / pyproject.toml) and wiki `version_context`.

## Phase 2: Knowledge Health & Staleness (LLM)
- Identify pages with `created:` dates older than 6 months for review.
- Flag concepts that may have been superseded by newer entries but lack `status: deprecated`.

## Phase 3: Conflict & Consistency Check (LLM)
- Detect pages that directly contradict each other (e.g., one says "always use X", another says "avoid X").
- When a contradiction is found, decide which is current and mark the other `deprecated` with `superseded_by:` pointing to the winner.

## Phase 4: Professional Health Report
Generate a summary in `doc/wiki/health_report.md` (or output to console):

### Report Template
```markdown
# 🩺 Wiki Health Report (YYYY-MM-DD)

## 📊 Knowledge Distribution
\`\`\`mermaid
pie title Knowledge Types
    "Active Concepts" : [count]
    "Active Patterns" : [count]
    "Deprecated" : [count]
    "Drafts" : [count]
\`\`\`

## ⚠️ Critical Issues
- **Broken Links**: [List]
- **Version Drift**: [List]
- **Conflicts**: [List]

## 🛠️ Auto-Fix Summary
- [x] Fixed YAML indentation in 3 files
- [x] Normalized 2 concept titles
```

## Phase 5: Auto-Fixing
- Automatically fix common YAML formatting errors.
- Normalize link casing to match file names.
- Update `index.md` if any titles were normalized (run `llm-wiki compile index`).
