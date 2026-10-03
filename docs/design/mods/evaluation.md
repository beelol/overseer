# Clear prose evaluation

Draft examples and rubric for the optional Clear prose mod. The rewritten examples illustrate the rules; they are not measured model outputs.

## Examples

| Weak wording | Desired wording |
| --- | --- |
| “Exact-head checks green. Activation paths unchanged.” | “The extension checks passed against the latest commit. I also confirmed that it still activates.” |
| “The editorial-row layout fosters a cohesive experience.” | “Each mod occupies one row, so you can compare its source, scope, and permissions.” |
| “Fixed. Tests pass.” | “I fixed the duplicate notification in Voice Mode. The regression test now passes.” |
| “Boundary-safe token-saver harness injection landed.” | “The harness now compresses supported tool results before sending them to the model. Permission requests still follow the existing approval path.” |
| “We should leverage the framework to unlock seamless modularity.” | “Use one mod manifest and translate its pieces for each harness.” |
| “RTK saves 90%.” | “RTK shortened the command outputs in this fixture by 48%. We have not measured its effect on a complete model turn.” |
| “Full test suite passed.” when only one test ran | “The test for duplicate notifications passed. The full suite has not run.” |
| “Mod code executes out-of-process with scoped permissions.” | “The mod runs as a separate process. It can read its own binaries and write to this turn's scratch folder.” |

Keep precise evidence rather than inventing it: the first, third, fourth, and sixth examples require corresponding actual checks. A requested detailed analysis must remain detailed.

## Frozen task set

Prepare twelve tasks with expected facts, not expected wording: explain a completed fix; report an incomplete test run; explain a failure; compare two choices; describe a new feature for a new reader; give three sequential actions; answer a one-fact question; report a permission block with its reason; state an uncertain finding; write a PR description; answer a voice request; and deliver a detailed technical explanation requested by the owner. Include exact paths, numbers, a quoted error, and a citation among the facts. Include cases where the reader has no earlier conversation.

Use identical prompts and initial data for the off/on conditions. Do not score a generated answer as a pass merely because the instruction file was loaded. No retries or cherry-picking. Keep all outputs in the local evaluation artifact and record the mod digest, harness/model/effort, time, token fields, and task outcome. An authorized future live evaluation uses gpt-5.6-luna at low effort only. Human rating requires no extra paid judge call.

## Rubric

Score each dimension 0, 1, or 2: 0 fails, 1 needs an edit, 2 is clear as written.

1. **Standalone context:** The reader can identify the subject, outcome, and meaningful limit or next action without earlier messages.
2. **Complete sentences:** Grammar and connecting words make the meaning natural. Headings, labels, code, and intentionally parallel list items are exempt.
3. **Plain wording:** Concrete nouns and verbs; no invented noun stacks or obscure jargon without explanation. Established technical phrases are allowed.
4. **Concision:** Every sentence contributes a fact, explanation, or necessary action; no repetitive recap or empty opening. Enough detail remains for the request.
5. **Evidence and accuracy:** All required facts, quotations, numbers, and uncertainties survive. Claims match actual checks and do not imply broader verification.

Proposal for acceptance: each task preserves every required fact, scores at least 8/10, and has no zero in standalone context, complete sentences, or evidence. The aggregate plain-wording/standalone scores should improve over baseline when baseline has headroom; already clear baseline answers need not become shorter. Record both ratings and reasons. The small sample establishes those tasks only; any proposed threshold must be agreed in the criterion, not changed after results arrive.

Programmatic checks can flag missing protected literals or known prohibited example phrases, but they cannot decide whether prose is natural or self-contained. Do not turn a regex lint into the quality claim. Apply the human rubric before comparing token counts; shorter answers that omit necessary context fail.
