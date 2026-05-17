# When to escalate

Pause and get explicit approval before rewriting migration history, dropping columns with data, or changing locking/transaction semantics. Summarize blast radius, rollback plan, and whether production has applied prior versions of the migration.
