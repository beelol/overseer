# Clear prose evaluation inputs, version 1

These are the twelve fixed tasks required by [AC-270](../../../../overseer-rfc.md), derived from the [evaluation design](../../evaluation.md) and [saved prose rules](../../clear-prose-rules.md). They are **inputs and blank records, not evaluated model outputs**. Every scenario, number, path, observation and quoted refusal is synthetic. No account, private audio, credential or production data appears here.

| Task | Category | What must survive |
| --- | --- | --- |
| CP-01 | Completed fix | The duplicate trigger, change, focused evidence and full-suite gap |
| CP-02 | Incomplete test run | Completed/planned stages, dependency failure and unrun UI |
| CP-03 | Failure | Exact file path/error, no mutation and recovery |
| CP-04 | Two choices | Next-turn need, overhead, unverified alternative and unchanged approvals |
| CP-05 | New-reader feature | Install versus enable, scopes and desired versus recorded delivery |
| CP-06 | Three sequential actions | Ordered preview/install/enable and exact fixture commands |
| CP-07 | One fact | Exact maximum size, answered as a complete sentence |
| CP-08 | Permission block | Exact refusal, unchanged bindings and explicit owner recovery |
| CP-09 | Uncertain finding | Observed transcript versus unexamined model request |
| CP-10 | PR description | Concrete trigger/change, exact evidence and rendered-check gap |
| CP-11 | Voice request | Natural spoken status with no prior chat context |
| CP-12 | Detailed explanation | Connected causal explanation, timeline, immutable state, unknowns and follow-up checks |

The CP-12 citation is supplied as `https://example.invalid/synthetic-fixtures/turn-contract-v1`. It is deliberately fictional, uses the reserved `.invalid` domain, and has no live page. Its source content is included in the prompt. It tests preservation and attribution of supplied evidence; it must not be described as real external verification. No web access is needed for these inputs. The `fixturectl` commands are fictional and must not be executed.

Each task JSON contains a **single condition-independent `prompt`**. Submit that decoded string, encoded as UTF-8, without rewriting it for either condition. `input_facts` are the reviewer's structured copy of facts already in the prompt; do not append them again. `required_facts_for_human_review` describes meaning to preserve, rather than expected wording. `literal_checks` only ask whether selected supplied paths, commands, errors, numbers, markup or the citation survive exactly. They are advisory diagnostics, never automatic acceptance, prose scoring or a fact-preservation verdict. A human may mark a semantic fact preserved despite a literal flag, such as a correctly spelled-out number. Use exact literals chiefly where the prompt requests verbatim evidence; do not add a semantic regex scorer. A number's presence can occur in the wrong context or inside another number; human review must establish its meaning. Do not invent an expected answer or make sentence structure match a golden response.

## Provenance and freezing

The authoring baseline is `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2`. `provenance.json` records source-file hashes, the bundled mod's version/file hashes, the daemon's content fingerprint and each decoded prompt hash. `SHA256SUMS` covers all twelve task files, provenance and the blank run-record template. It also covers this README. The digest of the manifest itself can be recorded before an authorized evaluation:

```sh
shasum -a 256 SHA256SUMS
shasum -a 256 -c SHA256SUMS
```

The source style hash is `ed7218d7d22358d55b994bdb17a43e614849304afae9c8e0f4f767301d4431f8`; the bundled version-1 fingerprint is `86911aa28dbf2da86311b9f9fb289db4cb049c761b7c5de41254b3ce2f09e64a`. The bundle fingerprint includes `mod.toml` and `style.md` using the daemon's sorted path/length/content hash, not merely the style-file digest. Record the **actual effective delivery snapshot and fingerprint** during evaluation; matching these reference hashes alone does not prove delivery.

After review, treat v1 task and template bytes as frozen. A later change gets a new set version and new provenance; never edit inputs after looking at results. Record the evaluation's actual Git commit and input-manifest hash separately. No criterion status or ledger changes accompany this preparation.

## Future authorized comparison

No paid turns, daemon, model calls, harness runner or judge have run or been added here. A future comparison needs its own authorized isolated dev setup and must obey repository paid-turn rules: **gpt-5.6-luna, low reasoning effort, one attempt per task per condition**. No Claude paid turns, retries, paid judge or selected best answer. Preserve every output, including failed or incomplete attempts. An interrupted/missing output stays missing; do not create a replacement attempt or a successful score.

Use an independently reset, identical initial harness/conversation/environment state for each condition. The sole intended difference is optional Clear prose delivery: off has no effective Clear prose binding, while on has the recorded bundled fingerprint delivered through a qualified route. Use a separate explicit binding when the chosen evaluation subject is Overseer; all agents does not cover it. Never let the on condition inherit previous instruction history from an off/on trial. Preserve permission, role, repository policy, model and effort. If installed native configuration/global suppression or other conditions differ or cannot be qualified, record that limitation; do not claim an isolated style-only effect.

Copy `run-record-template.json` into the **private local evaluation artifact** and keep prompts, original outputs, usage evidence and human notes there. This template intentionally defaults status to `not_run` and ratings/fact outcomes/token fields to `null`; unknown usage is never zero. It includes prompt/delivery hashes, harness version, effort, scope, immutable-state reference, output files/hashes, reported usage/cost and human reasons. Exclude account login identifiers and credentials from records. Never commit generated private evaluation outputs or credentials as part of these input fixtures.

A human rates **both conditions** on the five dimensions in the existing rubric: standalone context, complete sentences, plain wording, concision, evidence/accuracy. Each dimension is 0 (fails), 1 (needs an edit), or 2 (clear as written), with a reason. Check every semantic required fact separately. Literal checks may flag omissions, but passing them cannot produce a quality score or acceptance decision.

The existing AC-270 target applies to every **on** result: all required facts preserved, at least 8/10, and no zero for standalone context, complete sentences or evidence. Assess aggregate plain-wording/standalone improvement where the baseline has headroom; an already clear answer need not become shorter. The paired comparison fields record observations, not a new demand that each task improve. Apply the human rubric before comparing token counts. CP-12's concision score rewards necessary connected explanation and all requested detail; replacing it with a terse summary fails even if shorter. This input set defines no word cap and changes no rubric threshold.

Even a completed comparison establishes only these twelve synthetic tasks under the recorded conditions. Preparing the set establishes no prose-quality result, token saving, native/global/child support or verified AC-270 claim.
