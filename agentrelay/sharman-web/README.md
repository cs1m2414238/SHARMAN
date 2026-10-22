# SHARMAN Web Console

A demo-first interface for the Amazon Alexa+ track. This is a presentation surface over the SHARMAN MCP bridge; it does not replace the Windows/Tauri runtime.

## Run

In one terminal:

```powershell
cd ..\sharman-mcp
npm install
npm run dev
```

In another terminal:

```powershell
npm install
npm run dev
```

Open `http://127.0.0.1:4180` or go directly to `http://127.0.0.1:4180/alexa` for the simulated Alexa+ surface.

The UI clearly labels sample mailbox data. Tool calls, permissions, and results come from the running bridge rather than being hard-coded into the visual components.
