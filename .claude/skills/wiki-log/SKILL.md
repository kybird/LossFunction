---
name: wiki-log
description: Record knowledge into raw memory with Karpathy-inspired Agentic Memory format. Captures errors, decisions, and discoveries with high-density grounding.
skill-version: 1
---
# When to use
- After an error occurs or a bug is fixed
- After making a design decision or architectural choice
- After verifying a hypothesis or benchmarking performance
- When discovering non-obvious behavior (library quirk, platform limitation)
- When documenting "why" something was done a certain way

# Input
- type: error | design | verification | discovery (default: error)
- task: what you tried to do or decided
- error: (for type=error) error message or incorrect behavior
- wrong_code: (for type=error) incorrect code that caused the issue
- fix_code: corrected or final code
- decision: (for type=design) the decision made and alternatives considered
- result: (for type=verification) test results, benchmarks, or measurements
- finding: (for type=discovery) what was discovered
- environment: key package versions or context
- is_migration_fix: boolean (true if due to version upgrade or deprecation)
- git_hash: (optional) current git commit hash for grounding
- confidence: (1-5) how certain are we about this knowledge

# Action
1. Determine Date Filename
- Use current date in format: YYYY-MM-DD.md
- File path: doc/raw/YYYY-MM-DD.md

2. Concept Drift Protection (MANDATORY)
- **Search before naming**: Use `llm-wiki search` or `ls doc/wiki/concepts/` to find existing concepts.
- Reuse existing names to maintain a dense, high-utility knowledge graph.
- **Collect aliases**: While searching, note every other name the same concept travels under
  (Korean/English variants, error codes, product shorthand). Record them so compile can put
  `aliases: [...]` in the wiki page frontmatter — search fails when the writer's word and the
  searcher's word differ (plan.md 5.1(3)).

3. Verbatim Error Preservation (MANDATORY)
- The `### Error` section must contain the error message **character-for-character** — copy-paste,
  never paraphrase or summarize. Later searches paste real error strings; a summarized message
  deletes exactly the strings they would match.

4. Create Entry — choose format based on type:

### Type: error
```markdown
## Case N: [Title]

### Grounding
- Git Context: `hash:[git_hash]`
- Evidence: [path/to/artifact or log]
- Confidence: [1-5]/5

### Error
[error message or incorrect behavior]

### Environment Context
- Packages: [environment]
- Migration Issue: [Yes/No]

### Fix Code
[corrected code]

### Analysis
- Root cause: [brief explanation]
- Why it failed: [First principles analysis]
- How it was fixed: [explanation]

### Related Knowledge
- Concepts: [[concept-name]]
- Patterns: [[pattern-name]]
- **Anti-Patterns**: [[what-to-avoid]]
```

### Type: design
```markdown
## Case N: [Title]

### Grounding
- Git Context: `hash:[git_hash]`
- Confidence: [1-5]/5

### Design Decision
[what was decided]

### Alternatives Considered
- Option A: [description] — [why rejected]
- Option B: [description] — [why rejected]
- Selected: [why chosen]

### Decision Code
[code implementing the decision]

### Analysis
- Context: [situation/constraint]
- **Trade-offs**: [Memory vs Speed / Cost vs Complexity]
- Reversibility: [High/Low]

### Related Knowledge
- Concepts: [[concept-name]]
- Patterns: [[pattern-name]]
- **Anti-Patterns**: [[rejected-design-pattern]]
```

### Type: verification
```markdown
## Case N: [Title]

### Grounding
- Git Context: `hash:[git_hash]`
- Artifact: [path/to/benchmark_result.json]
- Confidence: [1-5]/5

### Hypothesis
[what was being tested]

### Results
[measurements, benchmarks]

### Analysis
- Confirmed/Refuted: [result]
- Key metrics: [numbers]
- Implications: [project impact]

### Related Knowledge
- Concepts: [[concept-name]]
- Patterns: [[pattern-name]]
```

### Type: discovery
```markdown
## Case N: [Title]

### Grounding
- Git Context: `hash:[git_hash]`
- Evidence: [logs or data]
- Confidence: [1-5]/5

### Discovery
[what was found - non-obvious behavior]

### Analysis
- Why non-obvious: [Surprise factor]
- Impact: [how this affects the project]
- Action taken: [mitigation/leveraging]

### Related Knowledge
- Concepts: [[concept-name]]
- Patterns: [[pattern-name]]
```

# Output
- File created or updated: doc/raw/YYYY-MM-DD.md
- Case anchor for direct linking: #case-N
- **Index Update**: Run `llm-wiki compile index` to make the new case searchable (rebuilds wiki index + syncs QMD search index). If QMD is not installed, the case is still found via grep fallback.
