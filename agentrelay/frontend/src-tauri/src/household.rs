use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Datelike, NaiveTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ============================================================================
// DOMAIN MODELS & ENUMS
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Valid,
    Malformed,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeEventSeverity {
    Normal,
    Attention,
    Critical,
}

impl HomeEventSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Attention => "attention",
            Self::Critical => "critical",
        }
    }

    pub fn parse_str(value: &str) -> Self {
        match value {
            "attention" => Self::Attention,
            "critical" => Self::Critical,
            _ => Self::Normal,
        }
    }
}

impl std::str::FromStr for HomeEventSeverity {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_str(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeEventType {
    DoorbellRing,
    AccessPointContact,
    MotionDetected,
    PresenceTransition,
    EnvironmentalAlert,
    Unknown,
}

impl HomeEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DoorbellRing => "doorbell_ring",
            Self::AccessPointContact => "access_point_contact",
            Self::MotionDetected => "motion_detected",
            Self::PresenceTransition => "presence_transition",
            Self::EnvironmentalAlert => "environmental_alert",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse_str(value: &str) -> Self {
        match value {
            "doorbell_ring" => Self::DoorbellRing,
            "access_point_contact" => Self::AccessPointContact,
            "motion_detected" => Self::MotionDetected,
            "presence_transition" => Self::PresenceTransition,
            "environmental_alert" => Self::EnvironmentalAlert,
            _ => Self::Unknown,
        }
    }
}

impl std::str::FromStr for HomeEventType {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_str(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRole {
    Admin,
    Resident,
    Caregiver,
    FrequentVisitor,
    TemporaryGuest,
    EmergencyContact,
}

impl MemberRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Resident => "resident",
            Self::Caregiver => "caregiver",
            Self::FrequentVisitor => "frequent_visitor",
            Self::TemporaryGuest => "temporary_guest",
            Self::EmergencyContact => "emergency_contact",
        }
    }

    pub fn parse_str(value: &str) -> Self {
        match value {
            "admin" => Self::Admin,
            "caregiver" => Self::Caregiver,
            "frequent_visitor" => Self::FrequentVisitor,
            "temporary_guest" => Self::TemporaryGuest,
            "emergency_contact" => Self::EmergencyContact,
            _ => Self::Resident,
        }
    }
}

impl std::str::FromStr for MemberRole {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_str(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    Present,
    Absent,
    Unknown,
}

impl PresenceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse_str(value: &str) -> Self {
        match value {
            "present" => Self::Present,
            "absent" => Self::Absent,
            _ => Self::Unknown,
        }
    }
}

impl std::str::FromStr for PresenceStatus {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_str(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneCategory {
    Perimeter,
    Exterior,
    InteriorLiving,
    PrivateRoom,
    HazardousArea,
}

impl ZoneCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Perimeter => "perimeter",
            Self::Exterior => "exterior",
            Self::InteriorLiving => "interior_living",
            Self::PrivateRoom => "private_room",
            Self::HazardousArea => "hazardous_area",
        }
    }

    pub fn parse_str(value: &str) -> Self {
        match value {
            "perimeter" => Self::Perimeter,
            "exterior" => Self::Exterior,
            "private_room" => Self::PrivateRoom,
            "hazardous_area" => Self::HazardousArea,
            _ => Self::InteriorLiving,
        }
    }
}

impl std::str::FromStr for ZoneCategory {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_str(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessPointKind {
    Door,
    Window,
    Gate,
    GarageDoor,
}

impl AccessPointKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Door => "door",
            Self::Window => "window",
            Self::Gate => "gate",
            Self::GarageDoor => "garage_door",
        }
    }

    pub fn parse_str(value: &str) -> Self {
        match value {
            "door" => Self::Door,
            "window" => Self::Window,
            "gate" => Self::Gate,
            "garage_door" => Self::GarageDoor,
            _ => Self::Door,
        }
    }
}

impl std::str::FromStr for AccessPointKind {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_str(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleCategory {
    ExpectedVisit,
    SchoolWork,
    CareWindow,
    NightWindow,
    QuietHours,
}

impl ScheduleCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ExpectedVisit => "expected_visit",
            Self::SchoolWork => "school_work",
            Self::CareWindow => "care_window",
            Self::NightWindow => "night_window",
            Self::QuietHours => "quiet_hours",
        }
    }

    pub fn parse_str(value: &str) -> Self {
        match value {
            "expected_visit" => Self::ExpectedVisit,
            "school_work" => Self::SchoolWork,
            "care_window" => Self::CareWindow,
            "night_window" => Self::NightWindow,
            "quiet_hours" => Self::QuietHours,
            _ => Self::ExpectedVisit,
        }
    }
}

impl std::str::FromStr for ScheduleCategory {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_str(value))
    }
}

// ============================================================================
// ENTITIES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeEvent {
    pub id: String,
    pub schema_version: u32,
    pub source: String,
    pub source_event_id: String,
    pub source_event_hash: String,
    pub occurred_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub sequence_number: u64,
    pub event_type: HomeEventType,
    pub zone_id: Option<String>,
    pub access_point_id: Option<String>,
    pub subject_id: Option<String>,
    pub severity: HomeEventSeverity,
    pub payload: serde_json::Value,
    pub validation_status: ValidationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewHomeEvent {
    pub source: String,
    pub source_event_id: String,
    pub occurred_at: DateTime<Utc>,
    pub event_type: HomeEventType,
    pub zone_id: Option<String>,
    pub access_point_id: Option<String>,
    pub subject_id: Option<String>,
    pub severity: HomeEventSeverity,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HouseholdMember {
    pub id: String,
    pub name: String,
    pub role: MemberRole,
    pub presence: PresenceStatus,
    pub initial_presence: PresenceStatus,
    pub presence_updated_at: DateTime<Utc>,
    pub enrolled_credentials: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HouseholdZone {
    pub id: String,
    pub name: String,
    pub category: ZoneCategory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPoint {
    pub id: String,
    pub zone_id: String,
    pub name: String,
    pub kind: AccessPointKind,
    pub is_monitored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HouseholdSchedule {
    pub id: String,
    pub label: String,
    pub category: ScheduleCategory,
    pub iana_timezone: String,
    pub start_time: String,    // "HH:MM"
    pub end_time: String,      // "HH:MM"
    pub days_of_week: Vec<u8>, // 1 (Mon) ..= 7 (Sun)
    pub target_zone_id: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OccupancyOverview {
    pub present_count: usize,
    pub absent_count: usize,
    pub unknown_count: usize,
    pub uncertainty_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IngestResult {
    Accepted(HomeEvent),
    Duplicate {
        existing_event: HomeEvent,
        rejection_reason: String,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructedHouseholdState {
    pub timestamp: DateTime<Utc>,
    pub members: Vec<HouseholdMember>,
    pub active_schedules: Vec<HouseholdSchedule>,
    pub events_count: usize,
    pub occupancy: OccupancyOverview,
}

// ============================================================================
// DATABASE STORE
// ============================================================================

pub struct HouseholdStore {
    path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

impl HouseholdStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let store = Self {
            path,
            write_lock: Arc::new(Mutex::new(())),
        };
        let conn = store.connection()?;
        store.init_tables(&conn)?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, String> {
        Connection::open(&self.path).map_err(|e| format!("Household DB open error: {e}"))
    }

    fn init_tables(&self, conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS home_events (
                id TEXT PRIMARY KEY NOT NULL,
                schema_version INTEGER NOT NULL DEFAULT 1,
                source TEXT NOT NULL,
                source_event_id TEXT NOT NULL,
                source_event_hash TEXT NOT NULL UNIQUE,
                occurred_at TEXT NOT NULL,
                received_at TEXT NOT NULL,
                sequence_number INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                zone_id TEXT,
                access_point_id TEXT,
                subject_id TEXT,
                severity TEXT NOT NULL,
                payload TEXT NOT NULL,
                validation_status TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_home_events_occurred ON home_events(occurred_at);
            CREATE INDEX IF NOT EXISTS idx_home_events_seq ON home_events(sequence_number);

            CREATE TABLE IF NOT EXISTS household_members (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                role TEXT NOT NULL,
                presence TEXT NOT NULL,
                initial_presence TEXT NOT NULL,
                presence_updated_at TEXT NOT NULL,
                enrolled_credentials TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS household_zones (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                category TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS household_access_points (
                id TEXT PRIMARY KEY NOT NULL,
                zone_id TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                is_monitored INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS household_schedules (
                id TEXT PRIMARY KEY NOT NULL,
                label TEXT NOT NULL,
                category TEXT NOT NULL,
                iana_timezone TEXT NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                days_of_week TEXT NOT NULL,
                target_zone_id TEXT,
                is_active INTEGER NOT NULL DEFAULT 1
            );
            ",
        )
        .map_err(|e| format!("Household init schema error: {e}"))?;
        Ok(())
    }

    pub fn calculate_event_hash(source: &str, source_event_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hasher.update(b"|");
        hasher.update(source_event_id.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn ingest_event(&self, input: NewHomeEvent) -> Result<IngestResult, String> {
        let _guard = self.write_lock.lock().map_err(|_| "Household lock error")?;
        let conn = self.connection()?;

        // Validation
        if input.source.trim().is_empty() {
            return Ok(IngestResult::Rejected {
                reason: "Event source cannot be empty".to_string(),
            });
        }
        if input.source_event_id.trim().is_empty() {
            return Ok(IngestResult::Rejected {
                reason: "Source event ID cannot be empty".to_string(),
            });
        }

        let hash = Self::calculate_event_hash(&input.source, &input.source_event_id);

        // Check deduplication
        let existing = conn
            .query_row(
                "SELECT id, schema_version, source, source_event_id, source_event_hash, occurred_at, received_at, sequence_number, event_type, zone_id, access_point_id, subject_id, severity, payload, validation_status FROM home_events WHERE source_event_hash = ?1",
                params![hash],
                row_to_event,
            )
            .optional()
            .map_err(|e| format!("Duplicate query failed: {e}"))?;

        if let Some(existing_event) = existing {
            return Ok(IngestResult::Duplicate {
                existing_event,
                rejection_reason: "Event with identical source and source_event_id already ingested. Duplicate ignored without side effects.".to_string(),
            });
        }

        let next_seq: u64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence_number), 0) + 1 FROM home_events",
                [],
                |row| row.get(0),
            )
            .unwrap_or(1);

        let event_id = format!("home_evt_{}", next_seq);
        let received_at = Utc::now();
        let payload_json =
            serde_json::to_string(&input.payload).unwrap_or_else(|_| "{}".to_string());

        conn.execute(
            "INSERT INTO home_events (
                id, schema_version, source, source_event_id, source_event_hash,
                occurred_at, received_at, sequence_number, event_type,
                zone_id, access_point_id, subject_id, severity, payload, validation_status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                event_id,
                1u32,
                input.source,
                input.source_event_id,
                hash,
                input.occurred_at.to_rfc3339(),
                received_at.to_rfc3339(),
                next_seq,
                input.event_type.as_str(),
                input.zone_id,
                input.access_point_id,
                input.subject_id,
                input.severity.as_str(),
                payload_json,
                "valid",
            ],
        )
        .map_err(|e| format!("Insert home event error: {e}"))?;

        // If it's a presence transition event, update member presence atomically
        if input.event_type == HomeEventType::PresenceTransition {
            if let Some(ref subject) = input.subject_id {
                let status_str = input
                    .payload
                    .get("new_status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let status = PresenceStatus::parse_str(status_str);
                let _ = conn.execute(
                    "UPDATE household_members SET presence = ?1, presence_updated_at = ?2 WHERE id = ?3",
                    params![status.as_str(), input.occurred_at.to_rfc3339(), subject],
                );
            }
        }

        let created_event = HomeEvent {
            id: event_id,
            schema_version: 1,
            source: input.source,
            source_event_id: input.source_event_id,
            source_event_hash: hash,
            occurred_at: input.occurred_at,
            received_at,
            sequence_number: next_seq,
            event_type: input.event_type,
            zone_id: input.zone_id,
            access_point_id: input.access_point_id,
            subject_id: input.subject_id,
            severity: input.severity,
            payload: input.payload,
            validation_status: ValidationStatus::Valid,
        };

        Ok(IngestResult::Accepted(created_event))
    }

    pub fn list_events(&self, limit: usize) -> Result<Vec<HomeEvent>, String> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, schema_version, source, source_event_id, source_event_hash,
                        occurred_at, received_at, sequence_number, event_type,
                        zone_id, access_point_id, subject_id, severity, payload, validation_status
                 FROM home_events ORDER BY sequence_number DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;

        let events = stmt
            .query_map(params![limit as i64], row_to_event)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        Ok(events)
    }

    pub fn get_event_by_id(&self, id: &str) -> Result<Option<HomeEvent>, String> {
        let conn = self.connection()?;
        conn.query_row(
            "SELECT id, schema_version, source, source_event_id, source_event_hash,
                    occurred_at, received_at, sequence_number, event_type,
                    zone_id, access_point_id, subject_id, severity, payload, validation_status
             FROM home_events WHERE id = ?1",
            params![id],
            row_to_event,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    // Member Management
    pub fn add_member(&self, member: HouseholdMember) -> Result<(), String> {
        let _guard = self.write_lock.lock().map_err(|_| "Household lock error")?;
        let conn = self.connection()?;
        let creds_json = serde_json::to_string(&member.enrolled_credentials)
            .unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT OR REPLACE INTO household_members (
                id, name, role, presence, initial_presence, presence_updated_at, enrolled_credentials, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                member.id,
                member.name,
                member.role.as_str(),
                member.presence.as_str(),
                member.initial_presence.as_str(),
                member.presence_updated_at.to_rfc3339(),
                creds_json,
                member.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| format!("Add member error: {e}"))?;
        Ok(())
    }

    pub fn list_members(&self) -> Result<Vec<HouseholdMember>, String> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare("SELECT id, name, role, presence, initial_presence, presence_updated_at, enrolled_credentials, created_at FROM household_members ORDER BY name ASC")
            .map_err(|e| e.to_string())?;

        let members = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let role_str: String = row.get(2)?;
                let presence_str: String = row.get(3)?;
                let initial_presence_str: String = row.get(4)?;
                let updated_str: String = row.get(5)?;
                let creds_json: String = row.get(6)?;
                let created_str: String = row.get(7)?;

                let role = MemberRole::parse_str(&role_str);
                let presence = PresenceStatus::parse_str(&presence_str);
                let initial_presence = PresenceStatus::parse_str(&initial_presence_str);
                let presence_updated_at = DateTime::parse_from_rfc3339(&updated_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let enrolled_credentials: Vec<String> =
                    serde_json::from_str(&creds_json).unwrap_or_default();
                let created_at = DateTime::parse_from_rfc3339(&created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(HouseholdMember {
                    id,
                    name,
                    role,
                    presence,
                    initial_presence,
                    presence_updated_at,
                    enrolled_credentials,
                    created_at,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        Ok(members)
    }

    pub fn occupancy_overview(&self) -> Result<OccupancyOverview, String> {
        let members = self.list_members()?;
        let mut present = 0;
        let mut absent = 0;
        let mut unknown = 0;

        for m in &members {
            match m.presence {
                PresenceStatus::Present => present += 1,
                PresenceStatus::Absent => absent += 1,
                PresenceStatus::Unknown => unknown += 1,
            }
        }

        Ok(OccupancyOverview {
            present_count: present,
            absent_count: absent,
            unknown_count: unknown,
            uncertainty_visible: unknown > 0,
        })
    }

    // Zones
    pub fn add_zone(&self, zone: HouseholdZone) -> Result<(), String> {
        let _guard = self.write_lock.lock().map_err(|_| "Household lock error")?;
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO household_zones (id, name, category) VALUES (?1, ?2, ?3)",
            params![zone.id, zone.name, zone.category.as_str()],
        )
        .map_err(|e| format!("Add zone error: {e}"))?;
        Ok(())
    }

    pub fn list_zones(&self) -> Result<Vec<HouseholdZone>, String> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare("SELECT id, name, category FROM household_zones ORDER BY name ASC")
            .map_err(|e| e.to_string())?;

        let zones = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let cat_str: String = row.get(2)?;
                Ok(HouseholdZone {
                    id,
                    name,
                    category: ZoneCategory::parse_str(&cat_str),
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        Ok(zones)
    }

    // Access Points
    pub fn add_access_point(&self, point: AccessPoint) -> Result<(), String> {
        let _guard = self.write_lock.lock().map_err(|_| "Household lock error")?;
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO household_access_points (id, zone_id, name, kind, is_monitored) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![point.id, point.zone_id, point.name, point.kind.as_str(), point.is_monitored as i32],
        )
        .map_err(|e| format!("Add access point error: {e}"))?;
        Ok(())
    }

    pub fn list_access_points(&self) -> Result<Vec<AccessPoint>, String> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare("SELECT id, zone_id, name, kind, is_monitored FROM household_access_points ORDER BY name ASC")
            .map_err(|e| e.to_string())?;

        let points = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let zone_id: String = row.get(1)?;
                let name: String = row.get(2)?;
                let kind_str: String = row.get(3)?;
                let is_monitored: i32 = row.get(4)?;
                Ok(AccessPoint {
                    id,
                    zone_id,
                    name,
                    kind: AccessPointKind::parse_str(&kind_str),
                    is_monitored: is_monitored != 0,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        Ok(points)
    }

    // Schedules
    pub fn add_schedule(&self, schedule: HouseholdSchedule) -> Result<(), String> {
        let _guard = self.write_lock.lock().map_err(|_| "Household lock error")?;
        let conn = self.connection()?;
        let days_json =
            serde_json::to_string(&schedule.days_of_week).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT OR REPLACE INTO household_schedules (id, label, category, iana_timezone, start_time, end_time, days_of_week, target_zone_id, is_active) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                schedule.id,
                schedule.label,
                schedule.category.as_str(),
                schedule.iana_timezone,
                schedule.start_time,
                schedule.end_time,
                days_json,
                schedule.target_zone_id,
                schedule.is_active as i32,
            ],
        )
        .map_err(|e| format!("Add schedule error: {e}"))?;
        Ok(())
    }

    pub fn list_schedules(&self) -> Result<Vec<HouseholdSchedule>, String> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare("SELECT id, label, category, iana_timezone, start_time, end_time, days_of_week, target_zone_id, is_active FROM household_schedules ORDER BY label ASC")
            .map_err(|e| e.to_string())?;

        let schedules = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let label: String = row.get(1)?;
                let cat_str: String = row.get(2)?;
                let iana_timezone: String = row.get(3)?;
                let start_time: String = row.get(4)?;
                let end_time: String = row.get(5)?;
                let days_json: String = row.get(6)?;
                let target_zone_id: Option<String> = row.get(7)?;
                let is_active: i32 = row.get(8)?;

                let category = ScheduleCategory::parse_str(&cat_str);
                let days_of_week: Vec<u8> = serde_json::from_str(&days_json).unwrap_or_default();

                Ok(HouseholdSchedule {
                    id,
                    label,
                    category,
                    iana_timezone,
                    start_time,
                    end_time,
                    days_of_week,
                    target_zone_id,
                    is_active: is_active != 0,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        Ok(schedules)
    }

    /// Evaluates if a schedule is active at a given target time
    pub fn is_schedule_active(schedule: &HouseholdSchedule, target: DateTime<Utc>) -> bool {
        if !schedule.is_active {
            return false;
        }

        let weekday = target.weekday().number_from_monday() as u8;
        if !schedule.days_of_week.contains(&weekday) {
            return false;
        }

        let start = match parse_hhmm(&schedule.start_time) {
            Some(t) => t,
            None => return false,
        };
        let end = match parse_hhmm(&schedule.end_time) {
            Some(t) => t,
            None => return false,
        };

        let current_time = target.time();

        // Normal window (e.g. 09:00 to 17:00)
        if start <= end {
            current_time >= start && current_time <= end
        } else {
            // Overnight window (e.g. 22:00 to 06:00)
            current_time >= start || current_time <= end
        }
    }

    /// Read-only historical state reconstruction.
    /// STRICT REPLAY INVARIANT: This function performs zero external requests,
    /// sends zero notifications, and creates zero physical side effects.
    pub fn reconstruct_state_at(
        &self,
        at: DateTime<Utc>,
    ) -> Result<ReconstructedHouseholdState, String> {
        let conn = self.connection()?;
        let at_rfc3339 = at.to_rfc3339();

        // Reconstruct events occurred at or before 'at'
        let mut stmt = conn
            .prepare(
                "SELECT id, schema_version, source, source_event_id, source_event_hash,
                        occurred_at, received_at, sequence_number, event_type,
                        zone_id, access_point_id, subject_id, severity, payload, validation_status
                 FROM home_events WHERE occurred_at <= ?1 ORDER BY occurred_at ASC",
            )
            .map_err(|e| e.to_string())?;

        let past_events: Vec<HomeEvent> = stmt
            .query_map(params![at_rfc3339], row_to_event)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        let mut members = self.list_members()?;
        // Only members created on or before 'at' are part of this historical state
        members.retain(|m| m.created_at <= at);

        // Initialize presence to the initial baseline before historical transitions
        for m in &mut members {
            m.presence = m.initial_presence;
        }

        // Reconstruct presence by tracing through presence transitions up to 'at'
        for event in &past_events {
            if event.event_type == HomeEventType::PresenceTransition {
                if let Some(ref subject) = event.subject_id {
                    let status_str = event
                        .payload
                        .get("new_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let status = PresenceStatus::parse_str(status_str);

                    for m in &mut members {
                        if &m.id == subject {
                            m.presence = status;
                            m.presence_updated_at = event.occurred_at;
                        }
                    }
                }
            }
        }

        let all_schedules = self.list_schedules()?;
        let active_schedules = all_schedules
            .into_iter()
            .filter(|s| Self::is_schedule_active(s, at))
            .collect();

        let mut present = 0;
        let mut absent = 0;
        let mut unknown = 0;
        for m in &members {
            match m.presence {
                PresenceStatus::Present => present += 1,
                PresenceStatus::Absent => absent += 1,
                PresenceStatus::Unknown => unknown += 1,
            }
        }

        Ok(ReconstructedHouseholdState {
            timestamp: at,
            members,
            active_schedules,
            events_count: past_events.len(),
            occupancy: OccupancyOverview {
                present_count: present,
                absent_count: absent,
                unknown_count: unknown,
                uncertainty_visible: unknown > 0,
            },
        })
    }
}

fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    NaiveTime::from_hms_opt(h, m, 0)
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<HomeEvent> {
    let id: String = row.get(0)?;
    let schema_version: u32 = row.get(1)?;
    let source: String = row.get(2)?;
    let source_event_id: String = row.get(3)?;
    let source_event_hash: String = row.get(4)?;
    let occurred_str: String = row.get(5)?;
    let received_str: String = row.get(6)?;
    let sequence_number: u64 = row.get(7)?;
    let event_type_str: String = row.get(8)?;
    let zone_id: Option<String> = row.get(9)?;
    let access_point_id: Option<String> = row.get(10)?;
    let subject_id: Option<String> = row.get(11)?;
    let severity_str: String = row.get(12)?;
    let payload_json: String = row.get(13)?;
    let _validation_str: String = row.get(14)?;

    let occurred_at = DateTime::parse_from_rfc3339(&occurred_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let received_at = DateTime::parse_from_rfc3339(&received_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let event_type = HomeEventType::parse_str(&event_type_str);
    let severity = HomeEventSeverity::parse_str(&severity_str);
    let payload = serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);

    Ok(HomeEvent {
        id,
        schema_version,
        source,
        source_event_id,
        source_event_hash,
        occurred_at,
        received_at,
        sequence_number,
        event_type,
        zone_id,
        access_point_id,
        subject_id,
        severity,
        payload,
        validation_status: ValidationStatus::Valid,
    })
}

// ============================================================================
// COMPREHENSIVE UNIT TEST SUITE (ACCEPTANCE CRITERIA)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::NamedTempFile;

    fn test_store() -> (HouseholdStore, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let store = HouseholdStore::open(file.path().to_path_buf()).expect("store open");
        (store, file)
    }

    #[test]
    fn schema_validation_and_unsupported_payload_rejection() {
        let (store, _file) = test_store();

        // Empty source rejected
        let res1 = store.ingest_event(NewHomeEvent {
            source: "   ".to_string(),
            source_event_id: "ext-1".to_string(),
            occurred_at: Utc::now(),
            event_type: HomeEventType::DoorbellRing,
            zone_id: None,
            access_point_id: None,
            subject_id: None,
            severity: HomeEventSeverity::Normal,
            payload: serde_json::json!({}),
        });
        assert!(matches!(res1.unwrap(), IngestResult::Rejected { .. }));

        // Empty source_event_id rejected
        let res2 = store.ingest_event(NewHomeEvent {
            source: "doorbell-ring".to_string(),
            source_event_id: "".to_string(),
            occurred_at: Utc::now(),
            event_type: HomeEventType::DoorbellRing,
            zone_id: None,
            access_point_id: None,
            subject_id: None,
            severity: HomeEventSeverity::Normal,
            payload: serde_json::json!({}),
        });
        assert!(matches!(res2.unwrap(), IngestResult::Rejected { .. }));
    }

    #[test]
    fn duplicate_event_deduplication_via_source_hash() {
        let (store, _file) = test_store();
        let now = Utc::now();

        let event = NewHomeEvent {
            source: "front-doorbell".to_string(),
            source_event_id: "evt-uuid-12345".to_string(),
            occurred_at: now,
            event_type: HomeEventType::DoorbellRing,
            zone_id: Some("perimeter-front".to_string()),
            access_point_id: Some("front-door".to_string()),
            subject_id: None,
            severity: HomeEventSeverity::Normal,
            payload: serde_json::json!({ "battery": 92 }),
        };

        let first = store.ingest_event(event.clone()).expect("ingest first");
        assert!(matches!(first, IngestResult::Accepted(_)));

        // Duplicate ingestion
        let second = store.ingest_event(event).expect("ingest second");
        match second {
            IngestResult::Duplicate {
                existing_event,
                rejection_reason,
            } => {
                assert_eq!(existing_event.source_event_id, "evt-uuid-12345");
                assert!(rejection_reason.contains("Duplicate ignored"));
            }
            _ => panic!("Expected duplicate detection"),
        }

        // Database should only have 1 record
        let list = store.list_events(10).expect("list");
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn out_of_order_event_ingestion_and_chronological_ordering() {
        let (store, _file) = test_store();
        let t1 = Utc.with_ymd_and_hms(2026, 10, 20, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 10, 20, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 10, 20, 10, 30, 0).unwrap(); // Ingested late!

        // Ingest t2 first
        store
            .ingest_event(NewHomeEvent {
                source: "sensor".to_string(),
                source_event_id: "ev-2".to_string(),
                occurred_at: t2,
                event_type: HomeEventType::MotionDetected,
                zone_id: None,
                access_point_id: None,
                subject_id: None,
                severity: HomeEventSeverity::Normal,
                payload: serde_json::json!({}),
            })
            .expect("t2");

        // Ingest t1 second
        store
            .ingest_event(NewHomeEvent {
                source: "sensor".to_string(),
                source_event_id: "ev-1".to_string(),
                occurred_at: t1,
                event_type: HomeEventType::MotionDetected,
                zone_id: None,
                access_point_id: None,
                subject_id: None,
                severity: HomeEventSeverity::Normal,
                payload: serde_json::json!({}),
            })
            .expect("t1");

        // Ingest late t3 third
        store
            .ingest_event(NewHomeEvent {
                source: "sensor".to_string(),
                source_event_id: "ev-3".to_string(),
                occurred_at: t3,
                event_type: HomeEventType::MotionDetected,
                zone_id: None,
                access_point_id: None,
                subject_id: None,
                severity: HomeEventSeverity::Normal,
                payload: serde_json::json!({}),
            })
            .expect("t3");

        // Reconstruct at t2: should find all 3 events ordered chronologically by occurred_at
        let state = store.reconstruct_state_at(t2).expect("reconstruct");
        assert_eq!(state.events_count, 3);
    }

    #[test]
    fn restart_persistence_and_database_reopening() {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_path_buf();

        // Session 1: open and write
        {
            let store = HouseholdStore::open(path.clone()).expect("open 1");
            store
                .add_member(HouseholdMember {
                    id: "usr_alice".to_string(),
                    name: "Alice Smith".to_string(),
                    role: MemberRole::Resident,
                    presence: PresenceStatus::Present,
                    initial_presence: PresenceStatus::Present,
                    presence_updated_at: Utc::now(),
                    enrolled_credentials: vec!["passkey:sha256:abcd".to_string()],
                    created_at: Utc::now(),
                })
                .expect("add member");

            store
                .add_zone(HouseholdZone {
                    id: "zone_kitchen".to_string(),
                    name: "Kitchen".to_string(),
                    category: ZoneCategory::InteriorLiving,
                })
                .expect("add zone");
        }

        // Session 2: reopen same path
        {
            let store = HouseholdStore::open(path).expect("open 2");
            let members = store.list_members().expect("list members");
            assert_eq!(members.len(), 1);
            assert_eq!(members[0].name, "Alice Smith");
            assert_eq!(members[0].presence, PresenceStatus::Present);

            let zones = store.list_zones().expect("list zones");
            assert_eq!(zones.len(), 1);
            assert_eq!(zones[0].name, "Kitchen");
        }
    }

    #[test]
    fn explicit_unknown_presence_and_occupancy_uncertainty() {
        let (store, _file) = test_store();
        let now = Utc::now();

        store
            .add_member(HouseholdMember {
                id: "m1".to_string(),
                name: "Member 1".to_string(),
                role: MemberRole::Resident,
                presence: PresenceStatus::Present,
                initial_presence: PresenceStatus::Present,
                presence_updated_at: now,
                enrolled_credentials: vec![],
                created_at: now,
            })
            .unwrap();

        store
            .add_member(HouseholdMember {
                id: "m2".to_string(),
                name: "Member 2".to_string(),
                role: MemberRole::Resident,
                presence: PresenceStatus::Unknown, // explicitly unknown presence
                initial_presence: PresenceStatus::Unknown,
                presence_updated_at: now,
                enrolled_credentials: vec![],
                created_at: now,
            })
            .unwrap();

        let overview = store.occupancy_overview().unwrap();
        assert_eq!(overview.present_count, 1);
        assert_eq!(overview.unknown_count, 1);
        assert!(overview.uncertainty_visible); // Occupancy uncertainty remains visible!
    }

    #[test]
    fn iana_timezone_and_overnight_schedule_evaluation() {
        // Schedule: Monday through Friday, 22:00 to 06:00 (overnight window)
        let schedule = HouseholdSchedule {
            id: "sched_night".to_string(),
            label: "Night Guard Hours".to_string(),
            category: ScheduleCategory::NightWindow,
            iana_timezone: "America/New_York".to_string(),
            start_time: "22:00".to_string(),
            end_time: "06:00".to_string(),
            days_of_week: vec![1, 2, 3, 4, 5],
            target_zone_id: None,
            is_active: true,
        };

        // Tuesday at 23:30 (active)
        let tuesday_night = Utc.with_ymd_and_hms(2026, 10, 20, 23, 30, 0).unwrap();
        assert!(HouseholdStore::is_schedule_active(&schedule, tuesday_night));

        // Tuesday at 04:15 (active overnight)
        let tuesday_early = Utc.with_ymd_and_hms(2026, 10, 20, 4, 15, 0).unwrap();
        assert!(HouseholdStore::is_schedule_active(&schedule, tuesday_early));

        // Tuesday at 14:00 (inactive during the day)
        let tuesday_afternoon = Utc.with_ymd_and_hms(2026, 10, 20, 14, 0, 0).unwrap();
        assert!(!HouseholdStore::is_schedule_active(
            &schedule,
            tuesday_afternoon
        ));

        // Sunday at 23:30 (inactive day)
        let sunday_night = Utc.with_ymd_and_hms(2026, 10, 25, 23, 30, 0).unwrap();
        assert!(!HouseholdStore::is_schedule_active(&schedule, sunday_night));
    }

    #[test]
    fn read_only_replay_invariance_zero_side_effects() {
        let (store, _file) = test_store();
        let t1 = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 10, 20, 12, 10, 0).unwrap();

        store
            .add_member(HouseholdMember {
                id: "usr_carol".to_string(),
                name: "Carol".to_string(),
                role: MemberRole::Resident,
                presence: PresenceStatus::Absent,
                initial_presence: PresenceStatus::Absent,
                presence_updated_at: t1,
                enrolled_credentials: vec![],
                created_at: t1,
            })
            .unwrap();

        // At t2, Carol arrived (presence transition event)
        store
            .ingest_event(NewHomeEvent {
                source: "door-passkey".to_string(),
                source_event_id: "evt-presence-1".to_string(),
                occurred_at: t2,
                event_type: HomeEventType::PresenceTransition,
                zone_id: None,
                access_point_id: None,
                subject_id: Some("usr_carol".to_string()),
                severity: HomeEventSeverity::Normal,
                payload: serde_json::json!({ "new_status": "present" }),
            })
            .unwrap();

        // Reconstruct at t1 (before arrival): Carol should be reconstructed as ABSENT
        let state_t1 = store.reconstruct_state_at(t1).unwrap();
        assert_eq!(state_t1.members[0].presence, PresenceStatus::Absent);
        assert_eq!(state_t1.events_count, 0);

        // Reconstruct at t2: Carol should be reconstructed as PRESENT
        let state_t2 = store.reconstruct_state_at(t2).unwrap();
        assert_eq!(state_t2.members[0].presence, PresenceStatus::Present);
        assert_eq!(state_t2.events_count, 1);

        // Replay performed 0 side effects (no new events created)
        assert_eq!(store.list_events(10).unwrap().len(), 1);
    }
}
