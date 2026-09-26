# Public implementation plan

1. Reproduce through the built CLI using disposable projects with an invalid
   state path and a directory that denies writes.
2. Introduce a typed lock-acquisition error behind the existing anyhow result
   API. Map typed evidence, not message substrings, to CLI diagnostics.
3. Install the lock guard immediately after exclusive file creation so payload
   failures release the newly created lock. Keep existing-lock refusal intact.
4. Document stable codes, age limitations, recovery, and deferred core work.
5. Run focused tests, full Rust tests, formatting, public-boundary checks,
   contracts and CLI smoke checks; independently review the final diff.
6. Publish the verified branch and PR for review. No merge or release.

Dependencies: reproduction -> implementation -> regression verification;
documentation depends on the behavior contract; review and PR require both.
Replan if any case incorrectly claims contention or changes ownership semantics.
Rollback: revert this additive diagnostic change; no stored-data migration.
