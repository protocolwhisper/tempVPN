## 1. Server Contract

- [x] 1.1 Register `/sessions/stream` statically with POST creation, HEAD management, and non-payable GET rejection in enabled and disabled configurations
- [x] 1.2 Parse POST creation parameters from JSON while preserving HEAD management query parameters
- [x] 1.3 Validate canonical 32-byte WireGuard public keys and duration before constructing an MPP challenge

## 2. Verification

- [x] 2.1 Update streaming route fixtures and verify valid POST challenges bind the caller's exact key and duration
- [x] 2.2 Add regression tests for GET 405, invalid-key 400, missing/malformed JSON, and disabled-route isolation without payment challenges
- [x] 2.3 Verify HEAD remains management-only and one-time session tests remain unchanged

## 3. Documentation

- [x] 3.1 Update tracked endpoint documentation to describe POST creation, HEAD management, and the public-key requirement
- [x] 3.2 Record the external registry/OpenAPI/llms.txt update and production reindex as an explicit deployment follow-up

## 4. Quality Gates

- [x] 4.1 Run formatter, focused node route tests, the node daemon test suite, and strict OpenSpec validation
