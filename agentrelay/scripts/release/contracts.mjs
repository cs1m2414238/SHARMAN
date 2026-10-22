import assert from 'node:assert/strict';
import { EXPECTED_TOOLS } from './checks.mjs';

export function validateTools(tools) {
  assert.deepEqual([...tools].sort(), [...EXPECTED_TOOLS].sort(), 'Unexpected, duplicated or missing MCP capability');
}

export function validateDenial(workflow) {
  assert.equal(workflow.agent.name, 'VisitorAgent');
  assert.equal(workflow.agent.parentAgentId, 'sharman-supervisor');
  assert.equal(workflow.agent.denialDetail.action, 'door.unlock');
  assert.equal(workflow.agent.denialDetail.requiredPermission, 'HIGH_RISK');
  assert.equal(workflow.agent.denialDetail.delegatedCeiling, 'LOW_RISK');
  assert.equal(workflow.agent.permissionCeiling, 'LOW_RISK');
  assert.equal(workflow.decision, 'denied');
  assert(!workflow.agent.permissions.includes('door.unlock'));
  assert(workflow.agent.forbidden.includes('door.unlock'));
  assert(workflow.agent.stepsUsed <= workflow.agent.maxSteps);
}

export function validateAudit(events, workflow) {
  const denial = events.find((event) => event.tool === 'door.unlock' && (workflow.requestId
    ? event.requestId === workflow.requestId
    : event.at === workflow.agent.createdAt));
  assert(denial);
  assert.equal(denial.permission, 'DENIED');
  assert.equal(denial.requiredPermission, 'HIGH_RISK');
  assert.equal(denial.delegatedCeiling, 'LOW_RISK');
  assert(denial.denialReason && denial.requestId);
  assert(events.some((event) => event.requestId === denial.requestId && event.tool === 'agent.delegate'));
  assert(events.some((event) => event.requestId === denial.requestId && event.tool === 'agent.terminate'));
  assert(Number.isFinite(Date.parse(workflow.agent.terminatedAt)));
  assert(Date.parse(workflow.agent.terminatedAt) >= Date.parse(workflow.agent.createdAt));
  return denial.requestId;
}
