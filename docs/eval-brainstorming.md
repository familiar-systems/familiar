# Eval Brainstorming

Living scratchpad for AI quality measurement in familiar.systems. Captures challenges, requirements, candidate algorithmic approaches. Not a design doc, not a roadmap. Specific eval implementations spin out into their own docs as they get committed.

## When eval matters

- Closed alpha is vibe-driven. Vibes good = ship. Vibes bad = evals become prerequisite.
- Either way, a coherent eval mechanism (or several) sits between v1 and v1.1.
- Status containment is the exception: probably runs pre-ship regardless of vibes (see below).

## Bandwidth: hand-crafted eval sets don't scale to one person

- One person today. Authoring hand-crafted eval sets at meaningful scale is not viable.
- Effective evals must ride on production traces and customer-base aggregation, not on dedicated labeling.
- Every candidate below is tagged:
  - **`[auto]`**: runs per-campaign from production traces, no labels needed.
  - **`[agg]`**: aggregates across customers; quality scales with customer count.
  - **`[handcraft]`**: requires hand-authored test set or labels.
- Default to `[auto]` and `[agg]`. Reserve `[handcraft]` for evals where stakes justify the cost.
- Design implication: build the data plumbing first. If acceptance traces, edit distances, suggestion outcomes, and rejection reasons are queryable as a stream, most `[auto]` evals fall out cheaply.
- A `[handcraft]`-heavy section is a warning sign: either the dimension can't be measured today, or it needs a creative `[auto]`/`[agg]` substitute, or it has to wait for a labeling budget.

## Why eval is structurally hard for this product

- **Ground truth is contested.** Two GMs given the same transcript disagree on "is this the same NPC." Inter-annotator agreement matters more than absolute accuracy.
- **GM acceptance is noisy.** Tired GM rubber-stamps. Strict GM rejects timing-wrong-but-content-right. Acceptance is leading indicator, not gold.
- **Distribution shift per campaign.** Vocabulary, voice, world model are per-campaign. Cross-campaign aggregates lie.
- **Self-reinforcing loops.** Bad mention extraction in session 3 creates dangling Things that bias session 12's retrieval. Drift evals catch this; point-in-time evals don't.
- **Synthetic data won't generalize.** Real campaigns have a long tail. Players invent words, GMs forget continuity, NPCs get nicknames mid-campaign.
- **Most quality dimensions interact.** Bad retrieval poisons Q&A grounding. Bad disambiguation poisons mention extraction. A single failing eval rarely points cleanly at one component.

## Dimensions worth measuring

Each: what it measures / why it's hard / candidate approaches with sourcing tag. None committed.

### STT and disambiguation

- **What:** Given STT-mangled output ("Marky"), recover canonical reference (Marquis) when available.
- **Hard:**
  - Recovery against known-correct transcript is a different (easier) eval than recovery from production STT.
  - Phonetic similarity is language-dependent; one metric per script family.
  - Negative case ("nothing should match") is real ground truth. Without it, system over-resolves.
- **Candidates:**
  - `[auto]` Per-campaign learned alias table. Every fuzzy resolution that survives GM acceptance becomes a high-confidence alias next session.
  - `[auto]` `[agg]` Confusion-matrix shape: not just right/wrong but *what we resolved to when wrong*. Per-campaign locally; cross-customer once anonymized.
  - `[auto]` Stratify by source: edit-distance match vs phonetic match vs learned alias vs vector fallback.

### Entity extraction

- **What:** Identify all entities (NPCs, locations, items, factions) per session. Link each to the right Thing, or correctly create a new one.
- **Hard:**
  - **False-create is worse than false-link.** Near-duplicate Things pollute retrieval forever. Asymmetry should be in the metric, not buried in P/R tradeoff.
  - Recall is hard to label without a human reading the transcript.
  - "What counts as an entity" is fuzzy. One-shot bartender vs recurring NPC is unknowable on first mention.
- **Candidates:**
  - `[handcraft]` Stratified sampling: GMs label 1 in N suggestions for entity-creation correctness.
  - `[auto]` Drift detection: near-duplicate Thing rate over time. Spike = extraction failure even without per-suggestion rejection.
  - `[auto]` Cross-session continuity: any Thing referenced in 3+ sessions becomes an automatic recurring-NPC test case for future sessions.

### Mention extraction

- **What:** From block content, extract every textual reference and resolve to right Thing (campaign-actor-domain-design.md §181-220).
- **Hard:**
  - Anaphora ("she", "the wizard") needs antecedents resolved upstream. Pronoun resolution failure cascades.
  - Mentions span block boundaries. Block-local extraction misses cross-block references.
- **Candidates:**
  - `[handcraft]` Recall@block on hand-labeled held-out sessions.
  - `[handcraft]` Pronoun-specific eval bucket (harder than nominal mentions).
  - `[auto]` Resolution accuracy harvested from downstream signal: where mention-derived links got rejected or rewritten by the GM, the underlying mention was probably wrong.

### Relationship extraction

- **What:** Propose right semantic edges between Things. Higher stakes than mentions: durable graph claims, semantic load.
- **Hard:**
  - Freeform vocabulary by design. "betrayed by" / "betrayed" / "treachery against" / "burned by" are the same edge.
  - Direction matters catastrophically. "A betrays B" vs "B betrays A" inverts ally/enemy.
- **Candidates:**
  - `[auto]` Label-cluster eval: cluster proposed labels in embedding space, count clusters per semantic group. Lower is better. No labels needed.
  - `[auto]` Direction-flip rate among rejected proposals (assumes rejection metadata captures reason).
  - `[auto]` `[agg]` Per-edge-type acceptance: some labels land 80%, others 40%; aggregate hides this. Cross-customer aggregation makes thin per-campaign data viable.

### Journal drafting quality

- **What:** Auto-drafted session journal is faithful, coherent, length-appropriate, voice-matching, and *complete*.
- **Hard:**
  - **Omission is silent.** Journal that skips the climactic combat is wrong; no automated metric flags it without ground-truth checkpoints.
  - Style is subjective. "Sounds like our campaign" doesn't operationalize trivially.
  - Length depends on session intensity. Four hours of social roleplay generates more transcript per beat than four hours of combat.
- **Candidates:**
  - `[handcraft]` Two-stage: extract salient events first, then eval journal coverage of each event. Salient-event ground truth needs labels, unless replaced with a model-extracted proxy (which makes the eval semi-circular).
  - `[auto]` Edit ratio: GM diff between draft and final. High edit = low draft quality.
  - `[auto]` Voice drift: embed drafted journal, compare to embedding centroid of prior approved journals from same campaign.

### Suggestion quality

- **What:** Suggestion outcomes broken down beyond accept/reject.
- **Buckets:**
  - Accept-as-is (verbatim).
  - Edit-then-accept (directionally right; informative middle).
  - Reject (wrong, or wrong moment).
  - Reject-and-redo-manually (almost right, but not enough to scaffold).
- **Hard:**
  - Buckets shade into each other; threshold on edit distance to discriminate accept vs edit-then-accept.
  - Reject-and-redo requires temporal join with subsequent GM activity.
- **Candidates:**
  - `[auto]` `[agg]` Edit-distance histogram per accepted suggestion. Distribution shape > mean. Aggregates cleanly across customers.
  - `[auto]` `[agg]` Time-to-action as confidence proxy: instant-accept vs long-deliberation-accept.

### Q&A grounding

- **What:** Answer cites right blocks, doesn't hallucinate, respects status.
- **Hard:**
  - Citation correctness and answer correctness are independent. Real block + wrong inference is a thing.
  - Graph-level hallucination (fabricated relationships) is worse than prose-level.
- **Candidates:**
  - `[auto]` Citation-presence audit: ungrounded answers flagged.
  - `[auto]` Held-out fact-recall: questions auto-generated from existing journal entries; system must recover the journaled fact. Auto-generation from journals dodges hand-authored test sets.
  - `[handcraft]` Adversarial probes designed to elicit hallucination ("population of Foo"). Red-team prompts have to be authored.

### Status containment

- **What:** GM-only content never leaks to player-facing surfaces.
- **Why separate:** Not a quality regression, a product-killer. Trust is binary.
- **Hard:**
  - Indirect leakage. "Merchant seems nervous" implies merchant has secrets the player wasn't supposed to know.
  - Cumulative leakage. No single answer leaks; the pattern narrows player hypothesis space too far.
- **Candidates:**
  - `[handcraft]` Red-team probes: GM seeds honeypot GM-only Things, queries player surfaces nearby. Any nonzero leakage = P0. Cost is justified at one-person scale; the alternative is shipping a known leak.
  - `[auto]` Counterfactual eval: same player question with and without GM-only content present. Answer must not change. Fully automated once probes exist.
  - Probably runs pre-ship regardless of vibes.

### Retrieval quality

- **What:** Right blocks surfaced for a given query. Substrate everything else depends on.
- **Candidates:**
  - `[auto]` Recall@k on known-relevant blocks built from production mentions and acceptance traces. Ground truth harvested, not authored.
  - `[handcraft]` Precision@k from GM-judged top results.
  - `[auto]` Latency-quality curve as corpus grows: where does brute-force break, where does ANN help.

### Long-horizon coherence

- **What:** Six months in, AI's mental model of an NPC stays consistent with what was established session 3.
- **Hard:**
  - Continuity errors are easy to introduce, hard to detect without time-aware probes.
  - System should *also* update on canon changes (retcons, reversals). Distinguish drift from authorized retcon.
e- **Candidates:**
  - `[auto]` Time-windowed Q&A: same question across campaign-state versions. Consistency unless retcon explains the drift.
  - `[auto]` Cross-reference Q&A drift against suggestion log of authorized retcons.

### Latency

Not quality but on the same dashboard. All `[auto]`, just measurement:
- SessionIngest end-to-end per hour of audio.
- P&R first-token latency.
- Q&A round-trip.
- Suggestion generation per session.

## Cross-language

**Rule:** language is a column on every chart, never a row in the aggregate.

- **STT baselines vary by orders of magnitude.** Whisper near-human English, decent major European, weak low-resource. A "90%" headline that's 95% English / 60% Japanese is worse than no number.
- **Phonetic similarity is language-dependent.**
  - Latin script: edit distance.
  - Japanese: hiragana/katakana variant analysis.
  - Mandarin: STT loses tone, so two transcribed strings can be visually identical but originally distinct words.
  - Fuzzy-match impl needs language-aware metrics; eval has to test each.
- **Code-switching is the norm in TTRPGs.** Players roleplay in English, discuss mechanics in native language. NPC names cross language boundaries. STT models fail at code-switch boundaries; this is a real eval case, not a corner case.
- **Proper noun preservation across language adaptation.** French campaign with English-named NPC: STT might preserve "Marquis" or render "Marquise" (gendered French equivalent). Eval distinguishes "STT mangled" from "language-appropriate adaptation GM didn't want."
- **Cultural priors in entity classification.** Japanese honorifics get attached to names; German compounds form names by concatenation; Spanish multi-part surnames. The "is this a name" classifier carries implicit cultural assumptions.
- **Ground truth is a moat.** English STT eval datasets exist publicly. TTRPG-multilingual ground truth is built from scratch. Production-instrumented stratified sampling > synthetic held-out test set; held-out won't cover the long tail.
- **Embedding model choice cascades.** Multilingual embeddings work but weaker per-language. Language-specific embeddings need a routing layer. Retrieval-quality eval has to be redone per embedding swap.
- **Language stratification compounds the bandwidth problem.** Per-language test sets multiply labeling demand by language-count. `[agg]` evals depending on customer base need enough customers *per language* to be statistically useful. Implication: defer language-stratified evaluation until the customer base supports it, but tag every event with language from day one so the data is recoverable later.

## Component coupling: every eval is a stack-level eval

**Rule:** any metric is a measurement of the *whole stack* unless you've held the rest of the stack fixed. End-to-end metrics tell you the system works or doesn't; they don't tell you which component to fix.

- **Pipelines stack many models.** STT, embedding, fuzzy matcher, mention extractor, persona-specific subagents, retrieval, generation. Each stage's quality is a confound for every downstream eval.
- **Silent attribution.** A better STT (less mangled output) silently improves fuzzy-matching numbers without the matcher having changed. The metric moves; the cause isn't visible from the metric alone. Good for users, ambiguous for engineering.
- **Model heterogeneity per persona.** Subagents use different models. A "Librarian" persona on a retrieval-tuned backbone (e.g., IBM Granite 4.1 30B) has a different envelope than the main agent on a generalist (e.g., Deepseek 4 Flash) doing nominally the same retrieval. Aggregate metrics hide this; per-persona stratification is mandatory.
- **Tools are part of the stack.** Two personas calling differently-named tools that both "retrieve" are not running the same eval even if input/output shapes match. Tool definitions get versioned alongside model choices.
- **Implications:**
  - Tag every eval event with the full stack: STT model, embedding model, fuzzy-match impl, persona, persona model, tool definitions, prompt version. Stratify reports by stack.
  - To attribute deltas, hold the rest of the stack fixed. If you can't, you can't attribute. End-to-end-only is fine for "is the product getting better"; useless for "which component to invest in."
  - Keep historical artifacts (raw STT outputs, prior embeddings, prior tool-call traces). Without them, swapping the embedding model invalidates retrieval baselines forever and cross-version comparisons are impossible.
  - Per-stage evals on *fixed inputs* are the only mechanism for clean attribution: eval the fuzzy matcher against a frozen corpus of STT-mangled strings, separately from the end-to-end pipeline. Same for retrieval against a fixed embedding corpus, etc.
  - Treat every model swap as an explicit eval-baseline-reset event for affected metrics. Log the cutover so future analyses don't span the discontinuity silently.
- **Compounds bandwidth.** Per-stack-version stratification multiplies cohort count. Each cohort has fewer samples. `[agg]` evals slow to converge; `[handcraft]` test sets need re-running per swap.
- **Compounds cross-language.** Per-language × per-stack × per-customer = three-dimensional stratification with thin per-cell counts. Report only the stratifications you have data to defend.

## Methodology pitfalls

- Don't aggregate cross-campaign without stratifying. Each campaign is its own distribution.
- Don't aggregate cross-language without stratifying. Same logic, harsher consequences.
- Treat GM acceptance as leading indicator, not ground truth. Sample-and-label for gold.
- Build eval sets from production traces, not synthetic transcripts. The long tail is the point.
- Watch self-reinforcing loops: drift evals catch what point-in-time evals miss.
- Eval the negative cases. "Should resolve to nothing" is a real label. Without it, the system over-resolves into hallucination.
- Beware LLM-as-judge for systems built on the same model family. Useful as triage layer, not as gold.

## Open questions

- Calibration: should the system surface its own uncertainty ("I'm 60% confident this is Marquis")? If yes, calibration of stated confidence vs actual accept rate becomes another eval dimension.
- LLM-as-judge for which dimensions? Cheap, biased same direction as the system being evaluated. Probably useful as triage before human eval.
- How does retcon interact with eval replay? Re-running an eval against the campaign state at session N requires either preserved snapshots or a deterministic replay of the session log. Affects whether eval is a build-time or runtime concern.
- Model-swap discipline: how do we mark a baseline reset cleanly? Which version bumps reset which metrics? A minor STT version bump might preserve fuzzy-match comparability; a major one almost certainly doesn't. Needs a per-component policy, not a global one.
