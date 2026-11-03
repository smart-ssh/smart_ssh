//! Leaf module (Issue #868): identifiers shared by `dto`, `state` and
//! `events` without making them depend on each other. `state` re-exports
//! [`SessionId`], so the old path `crate::state::SessionId` keeps working.

use uuid::Uuid;

pub type SessionId = Uuid;
