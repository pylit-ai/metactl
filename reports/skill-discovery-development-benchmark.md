# Skill discovery development benchmark

Command: `python3 scripts/benchmark_skill_discovery.py --repeats 3 --distractors 200`

Offline run, September 20, 2026. 208 descriptors, 12 authored development cases,
3 timing repetitions per arm. These are 12 quality examples, not 36 independent
examples. Raw rows, fixture/binary hashes and environment are in the adjacent JSON.
No provider or coding-model calls. Ranking was not retuned to improve this result.

| Metric | Existing route | Deterministic discovery host |
|---|---:|---:|
| Relevant-skill recall at five | 35% | 75% |
| Precision at five (fixed denominator) | 8% | 16% |
| Mean reciprocal rank | 0.400 | 0.658 |
| No-match accuracy (2 cases) | 100% | 100% |
| Median lookup latency | 59.4 ms | 126.2 ms |
| 95th percentile lookup latency | 64.4 ms | 130.5 ms |

The comparison isolates the implemented route, including fresh policy/content
validation and a cold CLI process. More complete descriptions improve this small
development set; descriptor validation/reloads cost about 67 ms at the median.
This is not an end-to-end speed improvement. Some relevant skills still miss the
top five; do not promote the router based on this suite.

The synthetic full metadata view is 32,002 UTF-8 bytes. The bootstrap plus two
tool schemas total 1,051 bytes, a 96.7% reduction for that boundary **only**.
Search results, loaded bodies, required governance, other installed skills,
host wrappers and caching are not removed by this comparison. Actual native
prompt tokens, task success, dollars and total latency remain unmeasured.

Jev is optional and disabled by default. Fake-response tests verify fallback and
reordering, not Jev accuracy, latency or value. No live Jev benchmark was run.
Its use does not explain any measured gain in this report.

## Verification findings retained

Initial end-user fixtures exposed incorrect JSON-envelope parsing and incomplete
fixture library setup; both were corrected before this run. Independent review
found approval/revocation, prerequisite, query-argv and exclusion-order issues;
regression tests now cover them. A timeout test intermittently took five seconds,
including after process-group cleanup was added. Instrumentation located the delay
inside the initial `communicate(timeout=0.1)` call, not process startup or cleanup.
The final transport gives the caller an independent event deadline while a daemon
thread exchanges pipe data; it kills the isolated group and bounds cleanup on
expiry. The test includes a descendant retaining output pipes. This is POSIX
evidence, not Windows process-tree coverage or a hard real-time guarantee.

## Decision

Keep the feature opt-in. Run the actual-host prompt-exclusion gate and independently
labeled held-out full sessions before adopting it broadly or changing any default.
See `docs/design/optional-skill-discovery.md` for the preregistered metrics and
`docs/user/skill-discovery.md` for the explicit core-only projection recipe.
