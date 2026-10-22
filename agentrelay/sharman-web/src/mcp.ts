import type { ChatResult } from './types';

const endpoint = '/mcp';
const protocolVersion = '2025-11-25';
let nextId = 1;
let connecting: Promise<McpConnection> | null = null;

export type McpConnection = { protocolVersion: string; tools: string[]; endpoint: string };
type Envelope<T> = { jsonrpc: string; id: number; result?: T; error?: { message: string } };

async function rpc<T>(method: string, params: Record<string, unknown>, notification = false): Promise<T> {
  const id = nextId++;
  const response = await fetch(endpoint, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'application/json, text/event-stream', 'MCP-Protocol-Version': protocolVersion },
    body: JSON.stringify({ jsonrpc: '2.0', ...(notification ? {} : { id }), method, params }),
    signal: AbortSignal.timeout(15_000),
  });
  if (!response.ok) throw new Error(`MCP ${method} failed (${response.status}).`);
  if (notification) return undefined as T;
  const raw = await response.text();
  const messages = response.headers.get('content-type')?.includes('text/event-stream')
    ? raw.split(/\r?\n\r?\n/).map((event) => event.split(/\r?\n/).filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trimStart()).join('\n')).filter(Boolean)
    : [raw];
  for (const message of messages) {
    const envelope = JSON.parse(message) as Envelope<T>;
    if (envelope.id !== id) continue;
    if (envelope.error) throw new Error(envelope.error.message);
    if (envelope.result === undefined) throw new Error('MCP response did not include a result.');
    return envelope.result;
  }
  throw new Error('MCP returned no matching response.');
}

export function connectAlexaMcp(): Promise<McpConnection> {
  connecting ??= (async () => {
    const initialized = await rpc<{ protocolVersion: string }>('initialize', {
      protocolVersion, capabilities: {}, clientInfo: { name: 'sharman-alexa-web', version: '0.23.0-alpha.1' },
    });
    if (initialized.protocolVersion !== protocolVersion) throw new Error('The server did not negotiate MCP 2025-11-25.');
    await rpc('notifications/initialized', {}, true);
    const tools: string[] = [];
    let cursor: string | undefined;
    for (let page = 0; page < 20; page++) {
      const result = await rpc<{ tools: Array<{ name: string }>; nextCursor?: string }>('tools/list', cursor ? { cursor } : {});
      tools.push(...result.tools.map((tool) => tool.name));
      cursor = result.nextCursor;
      if (!cursor) break;
    }
    if (cursor || !tools.includes('sharman_assistant_request')) throw new Error('SHARMAN command discovery is incomplete.');
    return { protocolVersion, tools, endpoint };
  })().catch((error) => { connecting = null; throw error; });
  return connecting;
}

export async function runAlexaCommand(message: string): Promise<ChatResult> {
  await connectAlexaMcp();
  try {
    const result = await rpc<{ structuredContent?: ChatResult; isError?: boolean; content?: Array<{ text?: string }> }>('tools/call', {
      name: 'sharman_assistant_request', arguments: { message },
    });
    if (result.isError || !result.structuredContent) throw new Error(result.content?.[0]?.text || 'SHARMAN returned no command result.');
    return { ...result.structuredContent, transport: 'streamable_http_mcp', mcpTools: ['sharman_assistant_request', ...(result.structuredContent.mcpTools ?? [])] };
  } catch (error) {
    // A write may have committed even if its response was lost. Never replay it.
    connecting = null;
    throw new Error(`${error instanceof Error ? error.message : 'MCP request failed.'} No command was retried. Check Tasks/Activity before repeating a write.`);
  }
}
