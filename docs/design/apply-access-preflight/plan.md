# Implementation plan

1. Reproduce a denied later destination through the real materializer with an
   earlier output. Baseline returns `compensated_failure` after one prior mutation.
2. Reuse path containment, planning and the installed `tempfile` dependency.
   Add one private access-check module; collect only actual write paths.
3. Probe before journal creation and snapshots. Preserve the action loop and
   its compensation behavior for failures that preflight cannot predict.
4. Exercise actual CLI text/JSON/agent refusal, repair/retry, repeated apply,
   permission and obstruction fixtures, cleanup, no-op and replacement semantics.
5. Run the full workspace suite and public-boundary gate; independent review
   must check both runtime safety and the documented limits before integration.
