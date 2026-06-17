use crate::error::Result as CoveyResult;

use super::*;

impl Event {
    /// Deserializes the raw JSON payload into the typed payload matching `event_type`.
    pub fn typed(&self) -> CoveyResult<TypedEvent> {
        let payload = EventPayload::from_json(self.event_type(), self.payload_json())?;
        Ok(TypedEvent {
            seq: self.seq,
            object_id: self.object_id.clone(),
            actor: self.actor.clone(),
            payload,
            created_at: self.created_at,
        })
    }
}

impl EventPayload {
    /// Parses a JSON payload according to the provided event kind.
    pub fn from_json(event_type: EventType, payload_json: &str) -> CoveyResult<Self> {
        let payload = match event_type {
            EventType::SessionRegistered => {
                Self::SessionRegistered(serde_json::from_str(payload_json)?)
            }
            EventType::SessionHeartbeat => {
                Self::SessionHeartbeat(serde_json::from_str(payload_json)?)
            }
            EventType::SessionExited => Self::SessionExited(serde_json::from_str(payload_json)?),
            EventType::RuntimeAttestationRecorded => {
                Self::RuntimeAttestationRecorded(serde_json::from_str(payload_json)?)
            }
            EventType::MetaTaskSubmitted => {
                Self::MetaTaskSubmitted(serde_json::from_str(payload_json)?)
            }
            EventType::MetaTaskCancelled => {
                Self::MetaTaskCancelled(serde_json::from_str(payload_json)?)
            }
            EventType::SubtaskCreated => Self::SubtaskCreated(serde_json::from_str(payload_json)?),
            EventType::SubtaskClaimed => Self::SubtaskClaimed(serde_json::from_str(payload_json)?),
            EventType::SubtaskStarted => Self::SubtaskStarted(serde_json::from_str(payload_json)?),
            EventType::SubtaskAbandoned => {
                Self::SubtaskAbandoned(serde_json::from_str(payload_json)?)
            }
            EventType::ClaimReleased => Self::ClaimReleased(serde_json::from_str(payload_json)?),
            EventType::ClaimRenewed => Self::ClaimRenewed(serde_json::from_str(payload_json)?),
            EventType::ArtifactPublished => {
                Self::ArtifactPublished(serde_json::from_str(payload_json)?)
            }
            EventType::ReviewRequested => {
                Self::ReviewRequested(serde_json::from_str(payload_json)?)
            }
            EventType::ReviewDecided => Self::ReviewDecided(serde_json::from_str(payload_json)?),
            EventType::PermissiveLandingRecorded => {
                Self::PermissiveLandingRecorded(serde_json::from_str(payload_json)?)
            }
            EventType::ReadyQueueEnqueued => {
                Self::ReadyQueueEnqueued(serde_json::from_str(payload_json)?)
            }
            EventType::ReadyQueueInFlight => {
                Self::ReadyQueueInFlight(serde_json::from_str(payload_json)?)
            }
            EventType::ApplyVerificationRecorded => {
                Self::ApplyVerificationRecorded(serde_json::from_str(payload_json)?)
            }
            EventType::ReadyQueueApplied => {
                Self::ReadyQueueApplied(serde_json::from_str(payload_json)?)
            }
            EventType::OpenSpecArchiveStatusRecorded => {
                Self::OpenSpecArchiveStatusRecorded(serde_json::from_str(payload_json)?)
            }
            EventType::ReadyQueueSuperseded => {
                Self::ReadyQueueSuperseded(serde_json::from_str(payload_json)?)
            }
            EventType::ReservationRequested => {
                Self::ReservationRequested(serde_json::from_str(payload_json)?)
            }
            EventType::ReservationReleased => {
                Self::ReservationReleased(serde_json::from_str(payload_json)?)
            }
            EventType::ReservationRenewed => {
                Self::ReservationRenewed(serde_json::from_str(payload_json)?)
            }
            EventType::ConflictResolved => {
                Self::ConflictResolved(serde_json::from_str(payload_json)?)
            }
            EventType::SessionsReaped => Self::SessionsReaped(serde_json::from_str(payload_json)?),
            EventType::ClaimsExpired => Self::ClaimsExpired(serde_json::from_str(payload_json)?),
            EventType::ReservationsExpired => {
                Self::ReservationsExpired(serde_json::from_str(payload_json)?)
            }
            EventType::OpenSpecImported => {
                Self::OpenSpecImported(Box::new(serde_json::from_str(payload_json)?))
            }
        };
        Ok(payload)
    }
}
