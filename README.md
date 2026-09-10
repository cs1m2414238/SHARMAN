# Sharman

Personal AI Command Center & Autonomous Agent Framework.

## Architecture

```
sharman/
├── sharman-core/          # planner, task model, tool router
├── sharman-windows/       # file, process, Git, Maven, app tools
├── sharman-safety/        # risk policy, approvals, audit
├── sharman-mcp/           # Spring AI MCP / Alexa+ gateway
├── sharman-browser/       # Playwright Java
├── sharman-sandbox/       # Docker / Windows isolation
├── sharman-state/         # PostgreSQL, AgentSync
├── sharman-web/           # React dashboard
├── docs/                  # architecture + threat model + demo flow
└── README.md
```

## Modules

- **`sharman-core`**: Core planner, task models, and tool routing engine.
- **`sharman-windows`**: System automation tools for file management, process handling, Git, Maven, and OS-level operations.
- **`sharman-safety`**: Risk policies, execution approvals, and security auditing.
- **`sharman-mcp`**: Spring AI Model Context Protocol (MCP) server & Alexa+ gateway integration.
- **`sharman-browser`**: Browser automation engine powered by Playwright Java.
- **`sharman-sandbox`**: Containerized isolation environments via Docker & Windows sandbox.
- **`sharman-state`**: Persistence layer using PostgreSQL and state synchronization (AgentSync).
- **`sharman-web`**: Interactive React-based dashboard UI.
- **`docs`**: Architectural specifications, threat models, and demonstration workflows.
