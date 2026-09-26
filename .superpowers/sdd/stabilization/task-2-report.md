# Task 2 — scheduled task isolation

The engine now derives one Task Scheduler owner from its configured `AppHandle` data directory. It canonicalizes the directory, normalizes Windows casing and separators, hashes it with SHA-256, and uses exact `\Search\<digest>\Reminders\` and `\Search\<digest>\Cron\` folders. Scope is never taken from a tool-page argument; resolution errors stop the operation. Cron create, delete, run, and list, plus reminder delete and reconcile, use that owner. List and reconcile filter the returned `TaskPath` for exact equality. Legacy `\KeepItLocal\` tasks are untouched.

Reminder creation now returns an honest unavailable error because Search has no reminder activation/display path. It never registers an engine executable as a reminder action. Task IDs retain their validation; reconciliation rejects an invalid active ID rather than pruning based on a partial set.

Red/green: `scheduler_paths_isolate_data_roots_and_task_kinds` failed against the former shared `\KeepItLocal\` path (`assertion failed: a_cron.starts_with("\\Search\\")`), then passed after the scoped owner was implemented. Synthetic Windows tests run generated operations against fake PowerShell cmdlets. They verify exact-folder reminder pruning, cron create/run/delete/list isolation, legacy-task preservation, reminder refusal, and missing-scope failure. No test called the real Task Scheduler or launched a reminder.

Validation: focused scheduler tests passed. `cargo test --no-default-features --quiet` with `CARGO_TARGET_DIR=C:/Users/user/Projects/Search/searchwin/Engine/target` passed in the normal Windows user context: **389 passed, 4 ignored, 0 failed**. A default-sandbox run had six unrelated DPAPI failures (`0x80070002`); rerunning outside that sandbox passed, matching the baseline's execution context. `rustfmt --edition 2021 --check` passed for the three edited Rust modules. `git -c core.whitespace=cr-at-eol diff --check` passed; the existing scheduler files are tracked as CRLF.

Limitation: the tests use synthetic Task Scheduler fixtures and do not verify live Windows Task Scheduler registration. Reminder activation remains deferred by design.
