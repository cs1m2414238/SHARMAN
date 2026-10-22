# SHARMAN MCP Bridge

This package is the Amazon Alexa+ integration surface for SHARMAN. It exposes a real Streamable HTTP MCP endpoint and a small localhost REST API used by the hackathon web console.

## Run locally

```powershell
npm install
npm run dev
```

- MCP: `http://127.0.0.1:4179/mcp`
- REST status: `http://127.0.0.1:4179/api/status`
- Health: `http://127.0.0.1:4179/health`

The default dataset is explicitly a demo dataset. It lets reviewers exercise the complete permission, planning, tool, result, and reminder flow without granting mailbox access. Production Gmail data remains in the Windows SHARMAN runtime and will be connected through a narrow authenticated adapter after live-inbox validation.

## Exposed tools

- `sharman_get_tasks`
- `sharman_get_action_items`
- `sharman_search_inbox`
- `sharman_create_task`
- `sharman_create_reminder`
- `sharman_open_allowed_url`
- `sharman_home_get_status`
- `sharman_home_handle_ring_event`
- `sharman_home_explain_with_bedrock`

Read tools are annotated read-only. Write tools are idempotent and represent explicit user-approved actions. URL opening only validates an HTTPS target and always leaves the actual launch to the local user.

## Optional Amazon Bedrock explanation

The Home workflow is deterministic: Ring scenario matching, permission decisions, and allowed actions never depend on a model. After a workflow exists, a user may explicitly ask Amazon Bedrock to rewrite seven grounded fields into a short accessible explanation.

The adapter uses the official AWS SDK for JavaScript v3 and its `ConverseCommand`. It reads credentials only through the AWS default provider chain. No access key or secret is accepted by the web UI or stored in SHARMAN state.

```powershell
$env:AWS_REGION = "us-east-1"
$env:SHARMAN_AWS_BEDROCK_ENABLED = "true"
$env:SHARMAN_BEDROCK_MODEL_ID = "amazon.nova-lite-v1:0"
npm run dev
```

The configured AWS identity needs `bedrock:InvokeModel` for the selected model or inference profile. Verify the path with:

```powershell
Invoke-RestMethod http://127.0.0.1:4179/api/aws/status
Invoke-RestMethod -Method Post http://127.0.0.1:4179/api/home/explain
```

A successful credentialed response reports `mode: "bedrock"`. Until then, the endpoint reports `mode: "local_fallback"`; the submission must not claim a live Bedrock call. Model prose is length- and policy-validated and cannot change the recorded workflow decision.

References: [AWS SDK v3 Bedrock Runtime examples](https://docs.aws.amazon.com/sdk-for-javascript/v3/developer-guide/javascript_bedrock-runtime_code_examples.html) and [ConverseCommand API](https://docs.aws.amazon.com/AWSJavaScriptSDK/v3/latest/client/bedrock-runtime/command/ConverseCommand/).

## Amazon submission status

The local endpoint is sufficient for development and demo recording. A production Alexa+ MCP add-on also needs a public HTTPS URL and OAuth 2.1/PKCE resource-server metadata. Those deployment credentials and URLs are intentionally not embedded in this repository.
