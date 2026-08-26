## Context

See `proposal.md` for motivation. The node daemon already implements connect and pause, and the human README describes them. The tracked global registry aggregator currently serves only `/nodes` and `/health`; the discovery document observed in production has no source in this checkout. The nested `mpptempos` repository contains the separate directory listing, not TempVPN's OpenAPI source.

## Goals / Non-Goals

**Goals:**

- Put the public OpenAPI document under TempVPN source control.
- Make the fixed-session purchase, connect, and pause sequence independently usable by an OpenAPI-driven agent.
- Test semantic details rather than only checking that path strings exist.

**Non-Goals:**

- Changing node lifecycle handlers or payment behavior.
- Proxying paid session calls through the registry.
- Updating the separately versioned MPP directory entry.
- Recreating or changing the existing human `/docs` presentation.

## Decisions

### Serve a checked-in OpenAPI 3.1 JSON document

The aggregator embeds a reviewed JSON file at compile time and serves it from `GET /openapi.json`. A checked-in document is easy for registry operators and external validators to inspect, and avoids adding a schema-generation dependency to the small Rust service.

Generating the document from aggregator routes was rejected because the paid routes live on node daemons, not the aggregator. Generating from Rust node types was also rejected for this fix because it would couple the global discovery service build to daemon internals without eliminating the need for operation descriptions and selected-node server semantics.

### Use operation-level selected-node servers

The root server remains `https://registry.tempvpn.xyz` for discovery. Node lifecycle operations declare an operation-level `{api_url}` server variable whose default is an HTTPS node hostname and whose description requires the selected `/nodes` record's `api_url`. This prevents agents from attempting paid calls against the registry aggregator.

### Share schemas and parameters

Connect and pause reference the same `session_id` parameter and `Session` response schema as creation. The session schema explicitly permits null key/address fields while paused and documents that connect populates them. Connect defines a required JSON public-key body. Pause defines no body and no administrative security requirement.

The document contains no private keys, wallet credentials, daemon-admin tokens, or registry-write secrets. Linux and macOS behavior remains identical because both already use these runtime endpoints.

## Risks / Trade-offs

- [The static document can drift from daemon behavior] → Contract tests assert required lifecycle operations, methods, parameters, bodies, selected-node servers, and response schemas; future endpoint changes must update both.
- [A default node hostname becomes unavailable] → Treat it only as the syntactically valid OpenAPI server-variable default and require clients to substitute the selected live node's `api_url`.
- [Production currently serves discovery files from an unknown layer] → Deploy only after comparing the generated route response with the current live document and preserving `/docs` and `/llms.txt` ownership.

## Migration Plan

1. Add and validate the source-controlled document locally.
2. Deploy the aggregator image only after confirming the production owner of `/docs` and `/llms.txt` so they are not displaced.
3. Verify `/openapi.json` includes connect and pause and that paid operation servers resolve to the selected node URL.
4. Update and reindex the separate MPP directory entry.

Rollback restores the previous aggregator image. Node session routes and active sessions are unaffected.
