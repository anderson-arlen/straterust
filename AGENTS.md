# Agent instructions

## Keep the implementation plan current

- Read `README.md` before implementation work, along with local `IMPLEMENTATION_PLAN.md` and `ARCHITECTURE.md` when present. The README explains the project and how to run it; the plan tracks work and status, and the architecture document records design decisions. The latter two documents are intentionally ignored by Git; create or maintain them locally as needed.
- Update `IMPLEMENTATION_PLAN.md` as part of the same change whenever work advances, requirements change, or an implementation decision changes the plan. Do not leave progress reporting only in chat or commit messages.
- Clearly distinguish **Not started**, **In progress**, **Complete**, **Blocked**, and **Deferred** work. Track individual deliverables within a milestone as work begins so completed work and remaining work are both visible.
- Mark a deliverable or milestone complete only when it is implemented and its applicable completion criteria have been verified. Record concise evidence, such as relevant checks, results, or file references. Partial implementation or unverified behavior remains incomplete.
- Record remaining work and concrete blockers for unfinished items, along with the next useful action. Revise obsolete decisions and dependencies so the plan describes the current intended implementation.
- Keep updates concise. Do not create duplicate tracking documents or rewrite unchanged sections just to record activity.

## Source control

- Track source, tests, build configuration, tooling, the public README and original redistributable fixtures.
- Keep `docs/`, the implementation plan, architecture document, research workspaces, imported proprietary content and build artifacts ignored. Public documentation must not link to ignored local documents or disclose planned direction.

## Simplicity is king

- Build the simplest clear implementation that meets the actual requirement. Every component must earn its place through real value.
- Be able to explain the concrete problem solved by every new crate, module, type, trait, subsystem, dependency, or abstraction, and why a simpler existing structure is insufficient. Explain non-obvious architectural choices in the change summary; a separate design document is not required for routine work.
- Create enough abstraction to separate reusable engine behavior from game-specific rules, content, and presentation without building speculative infrastructure.
- Prefer explicit data structures, ordinary functions, direct control flow, and small interfaces. Introduce an abstraction when it enforces a needed boundary, removes meaningful duplication, or supports a concrete requirement.
- Do not add unused extension points, empty scaffolding, generic frameworks, plugin systems, or layers whose only justification is that they might be useful someday. A component listed in the plan is not permission to build it before it is needed.
- Reuse or extend existing components when that stays clear. Simplify or remove unnecessary indirection encountered within the scope of the work. If a component cannot be justified, do not create it.

## Source file size

- Try to keep source files below 1,000 lines. At 1,000 lines, consider splitting the file into smaller modules when that makes organizational sense.
- The hard limit is 1,500 lines. When a source file reaches that size, find a sensible way to split it before adding more code.
- Split by responsibility or cohesive test groups; preserve clear module boundaries and avoid arbitrary fragments or extra abstraction just to reduce line counts.

## Saved-game compatibility

- Endeavor to preserve saves across updates. Ordinary engine and content fixes must not invalidate local saves solely because a simulation revision or gameplay hash changed.
- Keep saved-game migrations separate from strict multiplayer and replay identity checks. Load progress against current installed definitions so fixes apply to resumed games.
- When changing serialized state, content IDs or indexed mission/AI data, supply defaults, aliases or explicit migrations as needed and verify a representative older save. Preserve original save files on load and report specific incompatible changes.

## Command output

Command output may be redirected to keep session context concise. Use a fixed, predictable log path based on the project and command, and reuse that exact path on every run (for example, `cargo test` in StrateRust uses `/tmp/stratarust-cargo-test.log`). Do not use timestamps, random suffixes, task-specific names, or changing filenames. Keep the command invocation stable so an approval can be reused. When requesting approval, offer a narrowly scoped reusable rule that includes the fixed redirect when the approval system requires it. Avoid concurrent runs that write to the same log.
