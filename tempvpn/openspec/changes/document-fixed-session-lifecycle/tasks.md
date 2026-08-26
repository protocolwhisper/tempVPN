## 1. OpenAPI Source

- [x] 1.1 Add a checked-in OpenAPI 3.1 document covering node discovery and fixed-session creation
- [x] 1.2 Document `POST /sessions/{session_id}/connect` with the shared path parameter, required public-key body, session response, and retryable errors
- [x] 1.3 Document `POST /sessions/{session_id}/pause` with the shared path parameter, bodyless request, paused-session response, and not-found behavior
- [x] 1.4 Model paused and active session fields accurately and direct lifecycle operations to the selected node's HTTPS `api_url`

## 2. Registry Publication

- [x] 2.1 Serve the checked-in document from the aggregator at `GET /openapi.json` with JSON content type and public CORS behavior
- [x] 2.2 Add contract tests for the fixed-session path sequence, connect request body, pause semantics, shared schemas, and selected-node server

## 3. Documentation and Verification

- [x] 3.1 Update registry documentation with the OpenAPI route and the separate directory reindex follow-up
- [x] 3.2 Run formatting, aggregator tests, workspace checks relevant to the change, JSON validation, and strict OpenSpec validation
