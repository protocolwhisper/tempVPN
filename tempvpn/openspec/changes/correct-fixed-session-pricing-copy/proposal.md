## Why

The human pricing copy says “per started minute,” which implies partial minutes round up even though fixed sessions require whole-minute durations and charge linearly. The OpenAPI-backed docs and directory hint must describe the same constraint accurately.

## What Changes

- Replace the misleading phrase with “$0.01 per minute; duration must be a whole number of minutes.”
- Align the checked-in OpenAPI duration schema with the published 60-second minimum and 60-second multiple.
- Update the separately versioned MPP directory entry's `amountHint` with the same wording.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change corrects documentation and schema metadata to match existing production behavior.

## Impact

- TempVPN OpenAPI and registry documentation.
- The nested `mpptempos` service-directory entry.
- No node daemon, client, agent skill, configuration, infrastructure, payment calculation, credential, expiry, or routing behavior changes.
- Compatibility is additive documentation correction; rollback restores only the misleading copy.
