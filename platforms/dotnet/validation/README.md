# Manual native binding validation records

Each binding build/test run has a checked-in JSON record conforming to
[`record.schema.json`](record.schema.json). Linux work runs in the rebuilt
project dev container; native macOS and Windows work runs on those hosts.
This directory does not configure or depend on CI.

Record the tested source commit and whether it was dirty. If dirty, include
the SHA256 of its source diff. Record the runtime manifest/fork and the exact
font/corpus inputs, target RID, image digest or host OS/architecture, emulation
status and tool versions. Every command includes its working directory, exact
command, exit code and results. Every delivered artifact includes relative
path, byte size and SHA256; every retained log includes its path and SHA256.

A following commit may add only records/logs for the recorded tested source.
Any code/runtime/font/corpus change requires a new relevant manual run; adding
a record does not validate code written after the run. Local artifact creation
does not authorize publishing or establish an untested platform's support.

Use controlled public fixtures and sanitized logs. Do not check in native
binaries, credentials, private document content or private absolute paths.
State failures, ignored cases, unavailable platforms and emulation honestly.
An emulated or cross-compiled build does not count as native-host evidence.
Do not claim all five RIDs pass unless their manual native records exist.

Validate JSON syntax with `python3 -m json.tool <record>` and review the fields
against the schema. The implementation slice's review verifies the hashes
against actual build outputs, not merely that they have the expected shape.
Release publication remains a separate authorized action.
