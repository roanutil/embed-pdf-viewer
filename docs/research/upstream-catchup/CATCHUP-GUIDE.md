# Catch-up guide

This guide describes how to bring a research branch up to a pinned upstream target. It is a method, not a record. Progress, decisions and evidence for one catch-up belong in that catch-up's ledger.

## Before you start

1. Fix the targets. Record the upstream commits you are catching up to, for the viewer and any runtime it depends on. Do not move them while the catch-up runs. If upstream moves, the targets stay pinned and you plan a separate step.
2. Record the starting points of every branch the catch-up touches, so you can later prove a result against them.
3. Read the plan, the parity rules for platform packages, and the ledger. Check `git status` and `git log` before doing anything. Do not repeat a task whose evidence is already committed.

## Where work happens

- Use one checkout that the work owns, and say which one it is in the ledger. Do not edit an unrelated checkout.
- Keep long builds and test runs in the container or environment the project defines. Pin the runtime library path for tests that load native code, and record that path.
- Use a separate worktree for the baseline (see below). Link any generated build outputs it needs rather than rebuilding them.

## Commits and pushes

- Commit bounded steps. Each commit states what changed and the evidence for it: test counts, the failing names that remain, and the source the change follows.
- Push each commit to the fork that the user owns. Never push to the canonical upstream.
- Credentials for pushing come from the user's host. Do not start, configure or work around the host credential path yourself. If a push fails, report it.
- Follow the repository's attribution rules for commit messages.
- Do not stash. Do not reset or clean a worktree that holds uncommitted work.

## Baseline first

Before you change code, measure the starting point:

1. Create a worktree at the starting commit.
2. Link the same generated outputs the main checkout uses.
3. Run the full test suite there and save the list of failing tests by name.

Then run the same suite on the current tree and diff the lists. A failure that is in both lists is old. A failure only in the current tree is new. Do not judge a failure without this comparison.

## Conformance vectors

Vectors record the output of the reference implementation (the TypeScript oracle). A committed vector changes only when that reference behavior legitimately changed.

When a conformance replay fails, investigate in this order before you ask anyone:

1. Did the reference change? Look at the history of the reference source and of the generator between the starting commit and now. A vector written before a relevant reference change is stale, and regenerating it is correct.
2. Is it a bug in the port? Read the port's path, find the first value that differs, and compare it against the reference output.
3. Is it flakiness or environment? Regenerate the group twice and compare hashes. Run the replay twice. A result that depends on the environment is not a reference change.

If the reference changed, regenerate the group from the oracle, then review the semantic difference. Ignore text that is not behavior (such as descriptions). Fix the port to match.

Do not regenerate a vector to make a replay pass unless the reference changed. Do not restore an old key to make a port pass.

A regenerated answer that contradicts the case's own description is a signal, not a result. Check the description before you accept it.

Generators can be stale too. A generator that sends a removed or renamed input produces a failure that looks like behavior. Check that the generator's inputs match the current reference API, and fix the generator, not the reference.

When you cannot place a failure after these steps, bring it to the user with one of these labels: reference bug, port bug, flakiness, environment, or something else. Say what you checked.

Anything that needs an exception (the port deliberately deviating from the reference, or a change to the reference) needs the user's approval before it lands.

## Renames and new fields

Upstream renames keys and adds fields. For each one:

- Find the upstream commit that introduced it.
- Check whether that commit is in the starting commit. If it is, any vector written before it is stale.
- Check the reference source for the new name before you change the port.
- Expect test fixtures and expected values in the port's own tests to carry the old names. Update them to the reference names.

## Expected API that no longer exists

Conformance tests and vectors can name a function, type, field or module that the reference source no longer has. Do not delete the test or patch it blindly. Find out why the API went away, then change the test to match.

1. Confirm it is gone from the source, not moved or renamed. Search the packages for the name, including re-exports and `dist` outputs. Then search history: `git log -S '<name>' --all --oneline`, and read the commit that removed it.
2. Read the removal commit's message and diff. Classify the cause:
   - Renamed: the same behavior has a new name. Repoint the test to the new name. Check the shape, not just the name.
   - Moved: the behavior lives in another module. Repoint the import. If the module is not exported, it is an export decision (see below).
   - Merged or replaced: a new function covers the same job with a different contract. Rewrite the case against the new contract. Do not force the old expected values onto it.
   - Removed with no replacement: the behavior is gone. Drop the cases, and record in the ledger which coverage was lost and why.
   - Already gone before the starting commit: the tooling was stale before the catch-up. Say so in the ledger, because it changes how much of the failure the catch-up caused.
3. Check whether a removed symbol is still used by the reference suites or only by this tooling. If the reference still has behavior the tooling checked, that behavior needs a case in the reference, not only in the tooling.
4. Export decisions. Exporting an internal symbol from a package changes that package's public surface. Ask the user before doing it, unless they have already said export that kind of symbol.
5. Record each decision in the ledger with: the old name, the commit that removed it, the replacement or none, and any coverage lost.

Do not:

- Keep a stale expected value so the test still passes.
- Assume a similar name is the same behavior. Compare signatures and return shapes first.
- Regenerate a vector to remove a case you could not replace, unless the reference change justifies it.

## Golden values

A golden digest in a port's tests is only as good as its source. Before you trust one:

- Find the reference test that holds the same digest.
- Run that reference test, and dump the exact text it hashes, using a temporary change that you revert afterwards.
- Confirm the digest with `sha256sum` over that text before you change the golden.

Hashed text depends on key order, so a port that orders keys differently from the reference fails even when every value matches. Compare orders as well as values.

## Debugging patterns

- Run one replay with `cargo test -q --test <name>`, with the runtime library path set.
- Compare JSON semantically. Walk both trees, skip description fields, and list added, removed and changed paths. Also compare key order when a hash is involved.
- Find the first byte that differs in two texts. It usually points straight at the cause.
- When a removal from an ordered map reorders the rest, use an order-preserving removal.
- Change vectors through their generator, then compare with the committed version.
- For multi-line edits in a container, write the script to a file on the host, copy it in, and run it. Inline quoting breaks on apostrophes and backticks.
- Format the code after each edit set.

## Pitfalls

- A command that waits on a file must not sleep in the foreground. Use a background wait that notifies on completion.
- Files copied into a container can be owned by root. Fix ownership before appending to them.
- A test that passed at the starting commit and fails now is a regression until shown otherwise, even if the failure looks like a test expectation.

## Finishing

A catch-up is finished when:

- The full suite passes, or each remaining failure is recorded with its category and the decision made on it.
- Every changed vector traces to a reference change.
- Every platform package and consumer the change touches is current, per the parity rules.
- The fork holds the final commits, and the ledger records the evidence.
