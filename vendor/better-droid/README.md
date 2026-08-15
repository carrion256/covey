# Vendored: better-droid

This directory is a **snapshot** of the `better-droid` crate from the
`carrion256/mutai` repository, taken at commit `da66707` (2026-08-15). It is
vendored so this repository builds standalone with no path or git dependency
outside itself.

## Purpose

`better-droid` is the compiler side of the OpenSpec work-packet pipeline:

- `better-droid compile <change-id>` lints an OpenSpec change and emits the
  compiled mission artifact set under
  `.codex/state/better-droid/<change-id>/mission/` (mission, traceability,
  validation, path-policy, review-rubric, assumptions, compile-report, and
  mission packet).
- `covey import openspec` validates and deterministically imports that
  compiled set.

This vendored crate is what exercises the compile path in this repository's
tests, and its `better-droid` binary is available from this workspace as
`cargo run --bin better-droid -- compile <change-id>`.

## Refresh procedure

Do not edit vendored sources casually. If a fix must be applied here, mirror
it back upstream when possible. To refresh the snapshot:

```bash
# from a clone of carrion256/mutai at the desired commit:
git archive <commit> better-droid | tar -x -C <covey-clone>/vendor
```

Then restore the tree shape expected here: sources live directly under this
directory (there is no outer `better-droid/` wrapper), and this `README.md`
is preserved. Run the full workspace test suite after any refresh.

## License

Distributed under AGPL-3.0-or-later, the same license as the rest of this
repository. Upstream `carrion256/mutai` does not declare a license for this
crate; this snapshot is relicensed by this repository's terms with attribution
to the mutAI project.
