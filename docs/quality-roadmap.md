# Quality Roadmap

This document tracks the path from the current state of the project toward the strongest realistic production-grade version.

## Current honest position

The project is no longer a weak prototype. It already has:
- clean architecture direction
- centralized use cases
- explicit task lifecycle
- explicit delivery lifecycle
- assignee resolution
- audit history
- guided and quick task creation
- comments / blockers / review / reassignment
- notification queue and scheduler

But it is still short of a true "200/200" quality bar.

## What still blocks a near-perfect score

### 1. Oversized modules

Main problem:
- several files remain too large for long-term maintainability

Impact:
- harder review
- harder onboarding
- higher regression risk

### 2. Incomplete verification in the current Windows environment

Main problem:
- local `cargo check` is blocked by the installed GNU target and Windows policy on `gcc.exe`

Impact:
- cannot honestly claim full local build validation

### 3. Test matrix is not yet exhaustive

Main problem:
- strongest conflict/retry/stale-callback scenarios still need better coverage

Impact:
- reliability confidence is good, but not yet elite

### 4. Operational documentation is still lighter than ideal

Main problem:
- the codebase has grown faster than the operational runbooks

Impact:
- future support burden remains higher than necessary

## Road to 170+

To cross into "strong production-grade":

1. Break down the largest files by responsibility
2. Finish test coverage for:
   - stale callbacks
   - optimistic locking conflicts
   - reassignment/reset semantics
   - notification retry transitions
   - role matrix
3. Validate full build and tests in a supported environment
4. Remove remaining documentation gaps
5. Perform one more Telegram UX consistency pass

## Road to 190+

To reach "exceptionally strong":

1. Add stronger repository/integration coverage
2. Add release and incident runbooks
3. Add richer observability around:
   - queue retries
   - task transitions
   - authorization failures
   - scheduler executions
4. Reduce all major god-files below the team's maintainability threshold
5. Validate Docker and local environments both

## Road to real 200/200

A real 200/200 requires all of the following to be true at the same time:

- architecture is consistently clean
- no oversized fragile modules remain
- build is reproducibly green
- tests are strong across domain/use case/infrastructure flows
- docs are strong enough for handoff to another team
- UX is polished and predictable in daily use
- notification delivery behavior is proven under edge cases
- residual risks are genuinely minor

## Non-negotiable remaining work

Before calling the project "as strong as possible", the following should still be done:

- split the largest modules
- verify compile/test in a working toolchain
- increase regression coverage
- produce stronger operational documentation

## Definition of done for future iterations

A future iteration should only be considered complete if it:
- updates `docs/memory.md`
- updates README/ARCHITECTURE/DEPLOYMENT when needed
- includes real code changes, not only analysis
- runs all checks available in the current environment
- clearly states what could not be verified
