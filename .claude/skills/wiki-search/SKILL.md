---
name: wiki-search
description: Retrieve relevant past knowledge. Filters out deprecated practices and follows migration trails.
skill-version: 1
---
# When to use
- BEFORE writing any code
- At the start of every task

# Input
- current task description

# Action
1. **Search via CLI (Recommended)**:
   - Run: `llm-wiki search "<task keywords>"`
   - Always runs BOTH grep exact matching AND QMD semantic search (when installed) and merges
     them — grep results are ranked by matched-keyword count with line snippets.
   - Prefer pasting the **verbatim error string** as the query; exact strings are what grep
     guarantees to find.

2. **Follow Links**:
   - Use bidirectional links `[[link]]` and tags to navigate related knowledge.

3. **Deprecation Check (CRITICAL)**:
   - For every page found, check the YAML `status`.
   - If `status: deprecated`, follow `superseded_by`.

# Output Format
- Warnings: Include any anti-patterns or recently deprecated methods found.
- Recommendations: Based ONLY on `status: active` patterns.
- Related cases: [[case-link]]

# Answer Archiving
- If this search produced a genuinely useful **synthesized answer** (spanning several pages,
  the kind a future session will re-derive at real cost), promote it:
  - Write `doc/wiki/answers/<topic>.md` — frontmatter: `status: active`, `created: YYYY-MM-DD`,
    `tags: [...]`, `aliases: [...]`, plus the answer body with `[[links]]` back to sources.
  - Next `llm-wiki compile index` lists it in the Answers section and it becomes searchable.
- Archive answers, not raw search dumps: a question answered in one page does not need this.
