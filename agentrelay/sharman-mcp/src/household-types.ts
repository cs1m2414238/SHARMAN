/**
 * SHARMAN Home — Household Foundation Types (Checkpoint 1 / Phase A)
 *
 * Matching Rust models in `agentrelay/frontend/src-tauri/src/household.rs`.
 * Release Target: v0.26.0-home.1
 */

export type ValidationStatus = 'valid' | 'malformed' | 'unsupported';

export type HomeEventSeverity = 'normal' | 'attention' | 'critical';

export type HomeEventType =
  | 'doorbell_ring'
  | 'door_open'
  | 'door_close'
  | 'motion_detected'
  | 'occupancy_change'
  | 'environmental_alert'
  | 'system_ping'
  | 'unknown';

export type MemberRole =
  | 'owner'
  | 'resident'
  | 'caregiver'
  | 'guest'
  | 'service_provider'
  | 'child'
  | 'elderly';

export type PresenceStatus = 'present' | 'absent' | 'unknown';

export type ZoneCategory =
  | 'interior'
  | 'exterior'
  | 'perimeter'
  | 'shared'
  | 'restricted';

export type AccessPointKind =
  | 'exterior_door'
  | 'interior_door'
  | 'garage_door'
  | 'gate'
  | 'window';

export type ScheduleCategory =
  | 'curfew'
  | 'expected_window'
  | 'routine'
  | 'quiet_hours';

export interface HomeEvent {
  id: string;
  schema_version: number;
  source: string;
  source_event_id: string;
  source_event_hash: string;
  occurred_at: string; // ISO 8601 UTC
  received_at: string; // ISO 8601 UTC
  sequence_number: number;
  event_type: HomeEventType;
  zone_id?: string | null;
  access_point_id?: string | null;
  subject_id?: string | null;
  severity: HomeEventSeverity;
  payload: Record<string, unknown> | null;
  validation_status: ValidationStatus;
}

export interface NewHomeEvent {
  source: string;
  source_event_id: string;
  occurred_at: string; // ISO 8601 UTC
  event_type: HomeEventType;
  zone_id?: string | null;
  access_point_id?: string | null;
  subject_id?: string | null;
  severity: HomeEventSeverity;
  payload?: Record<string, unknown> | null;
}

export interface HouseholdMember {
  id: string;
  name: string;
  role: MemberRole;
  presence: PresenceStatus;
  initial_presence: PresenceStatus;
  presence_updated_at: string; // ISO 8601 UTC
  enrolled_credentials: string[];
  created_at: string; // ISO 8601 UTC
}

export interface HouseholdZone {
  id: string;
  name: string;
  category: ZoneCategory;
}

export interface AccessPoint {
  id: string;
  zone_id: string;
  name: string;
  kind: AccessPointKind;
  is_monitored: boolean;
}

export interface HouseholdSchedule {
  id: string;
  label: string;
  category: ScheduleCategory;
  iana_timezone: string;
  start_time: string; // "HH:MM"
  end_time: string; // "HH:MM"
  days_of_week: number[]; // 1 (Mon) ..= 7 (Sun)
  target_zone_id?: string | null;
  is_active: boolean;
}

export interface OccupancyOverview {
  present_count: number;
  absent_count: number;
  unknown_count: number;
  uncertainty_visible: boolean;
}

export interface ReconstructedHouseholdState {
  timestamp: string; // ISO 8601 UTC
  members: HouseholdMember[];
  active_schedules: HouseholdSchedule[];
  events_count: number;
  occupancy: OccupancyOverview;
}

export interface IngestResult {
  status: 'ingested' | 'duplicate_ignored' | 'malformed_rejected';
  event?: HomeEvent | null;
  sequence_number?: number | null;
  rejection_reason?: string | null;
}
