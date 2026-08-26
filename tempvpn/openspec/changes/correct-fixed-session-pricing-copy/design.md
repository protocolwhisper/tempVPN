## Context

See `proposal.md` for motivation. TempVPN's OpenAPI/registry documentation is tracked in the main repository, while the directory `amountHint` is tracked in the nested `mpptempos` repository on its own branch. Production already enforces and advertises a 60-second minimum and 60-second multiples.

## Goals / Non-Goals

**Goals:**

- Use identical, unambiguous pricing copy in both publication surfaces.
- Keep schema metadata aligned with existing production validation.

**Non-Goals:**

- Changing prices, rounding, validation, payments, or client behavior.
- Combining the two repositories or deploying either change.

## Decisions

Use the reviewer-supplied sentence verbatim: “$0.01 per minute; duration must be a whole number of minutes.” In OpenAPI, pair that sentence with `minimum: 60` and `multipleOf: 60` so machines receive the same constraint as humans. In the directory, use the same sentence as `amountHint` rather than paraphrasing it.

No secrets, persistent state, cleanup behavior, or Linux/macOS differences are involved.

## Risks / Trade-offs

- [Only one repository is published] → Report both repository states separately and keep deployment/reindex follow-ups explicit.
- [Copy drifts again] → Add exact-string and schema assertions to the existing OpenAPI contract test and run directory schema tests.

## Migration Plan

Publish the TempVPN registry/OpenAPI change, then publish and reindex the directory branch. Either side can be rolled back independently because runtime pricing is unchanged.
