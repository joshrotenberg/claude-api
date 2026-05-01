//! Anthropic Admin API.
//!
//! Workspace-, user-, key-, and usage-management endpoints under
//! `/v1/organizations`. **Requires an admin API key** (distinct from
//! the regular API key used for Messages/Models/etc.) -- pass it via
//! [`Client::builder().api_key(...)`](crate::ClientBuilder::api_key)
//! exactly like a normal key. The wire-level `x-api-key` header is the
//! same; only the key's permissions differ.
//!
//! Gated on the `admin` feature.
//!
//! # Layout
//!
//! - [`organization`] -- the authenticated organization (`me`).
//! - [`invites`] -- create / retrieve / list / delete user invites.
//! - [`users`] -- retrieve / list / update / delete users.
//! - [`workspaces`] -- workspace CRUD plus archive.
//! - [`workspace_members`] -- per-workspace membership.

#![cfg(feature = "admin")]
#![cfg_attr(docsrs, doc(cfg(feature = "admin")))]

use serde::{Deserialize, Serialize};

use crate::client::Client;

pub mod api_keys;
pub mod cost_report;
pub mod invites;
pub mod organization;
pub mod rate_limits;
pub mod usage_report;
pub mod users;
pub mod workspace_members;
pub mod workspaces;

/// Top-level namespace handle for the Admin API.
///
/// Obtained via [`Client::admin`].
pub struct Admin<'a> {
    client: &'a Client,
}

impl<'a> Admin<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Sub-namespace for the authenticated organization
    /// (`/v1/organizations/me`).
    #[must_use]
    pub fn organization(&self) -> organization::Organization<'a> {
        organization::Organization::new(self.client)
    }

    /// Sub-namespace for invites.
    #[must_use]
    pub fn invites(&self) -> invites::Invites<'a> {
        invites::Invites::new(self.client)
    }

    /// Sub-namespace for organization users.
    #[must_use]
    pub fn users(&self) -> users::Users<'a> {
        users::Users::new(self.client)
    }

    /// Sub-namespace for workspaces.
    #[must_use]
    pub fn workspaces(&self) -> workspaces::Workspaces<'a> {
        workspaces::Workspaces::new(self.client)
    }

    /// Sub-namespace for workspace members. Scoped to a single
    /// workspace at call time.
    #[must_use]
    pub fn workspace_members(
        &self,
        workspace_id: impl Into<String>,
    ) -> workspace_members::WorkspaceMembers<'a> {
        workspace_members::WorkspaceMembers::new(self.client, workspace_id.into())
    }

    /// Sub-namespace for API keys.
    #[must_use]
    pub fn api_keys(&self) -> api_keys::ApiKeys<'a> {
        api_keys::ApiKeys::new(self.client)
    }

    /// Sub-namespace for the usage reports.
    #[must_use]
    pub fn usage_report(&self) -> usage_report::UsageReport<'a> {
        usage_report::UsageReport::new(self.client)
    }

    /// Sub-namespace for the cost report.
    #[must_use]
    pub fn cost(&self) -> cost_report::Cost<'a> {
        cost_report::Cost::new(self.client)
    }

    /// Sub-namespace for rate-limit listings (org + workspace).
    #[must_use]
    pub fn rate_limits(&self) -> rate_limits::RateLimits<'a> {
        rate_limits::RateLimits::new(self.client)
    }
}

// =====================================================================
// Shared role + status enums
// =====================================================================

/// Organization-level user role. Forward-compatible: unknown roles
/// fall through to [`Self::Other`].
///
/// Note that `admin` is read-only on responses; you cannot invite a
/// user as `admin` or update an existing user's role to `admin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrganizationRole {
    /// Standard end-user.
    User,
    /// Developer with API access.
    Developer,
    /// Billing-only access.
    Billing,
    /// Organization administrator (read-only on responses).
    Admin,
    /// Claude Code user role.
    ClaudeCodeUser,
    /// Unknown role; raw string preserved.
    Other(String),
}

impl Serialize for OrganizationRole {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            Self::User => "user",
            Self::Developer => "developer",
            Self::Billing => "billing",
            Self::Admin => "admin",
            Self::ClaudeCodeUser => "claude_code_user",
            Self::Other(v) => v,
        })
    }
}

impl<'de> Deserialize<'de> for OrganizationRole {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "user" => Self::User,
            "developer" => Self::Developer,
            "billing" => Self::Billing,
            "admin" => Self::Admin,
            "claude_code_user" => Self::ClaudeCodeUser,
            _ => Self::Other(s),
        })
    }
}

/// Subset of [`OrganizationRole`] valid as a write value (no `admin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WriteOrganizationRole {
    /// Standard end-user.
    User,
    /// Developer with API access.
    Developer,
    /// Billing-only access.
    Billing,
    /// Claude Code user role.
    ClaudeCodeUser,
}

/// Status of a pending invite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InviteStatus {
    /// User has accepted the invite.
    Accepted,
    /// Invite expired without being accepted.
    Expired,
    /// Invite was deleted by an admin.
    Deleted,
    /// Invite is outstanding.
    Pending,
}

/// Workspace-level role.
///
/// `workspace_billing` is read-only on responses; you cannot create or
/// update a member to that role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRole {
    /// Standard workspace user.
    User,
    /// Developer with API access in this workspace.
    Developer,
    /// Restricted developer (subset of developer).
    RestrictedDeveloper,
    /// Workspace administrator.
    Admin,
    /// Billing-only role (read-only on responses).
    Billing,
    /// Unknown role; raw string preserved.
    Other(String),
}

impl Serialize for WorkspaceRole {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            Self::User => "workspace_user",
            Self::Developer => "workspace_developer",
            Self::RestrictedDeveloper => "workspace_restricted_developer",
            Self::Admin => "workspace_admin",
            Self::Billing => "workspace_billing",
            Self::Other(v) => v,
        })
    }
}

impl<'de> Deserialize<'de> for WorkspaceRole {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "workspace_user" => Self::User,
            "workspace_developer" => Self::Developer,
            "workspace_restricted_developer" => Self::RestrictedDeveloper,
            "workspace_admin" => Self::Admin,
            "workspace_billing" => Self::Billing,
            _ => Self::Other(s),
        })
    }
}

/// Subset of [`WorkspaceRole`] valid as a write value (no
/// `workspace_billing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WriteWorkspaceRole {
    /// Standard workspace user.
    WorkspaceUser,
    /// Developer with API access in this workspace.
    WorkspaceDeveloper,
    /// Restricted developer.
    WorkspaceRestrictedDeveloper,
    /// Workspace administrator.
    WorkspaceAdmin,
}

/// Common pagination params for `after_id` / `before_id` / `limit`
/// list endpoints (invites / users / workspaces / members / api keys).
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ListParams {
    /// Cursor: results immediately after this ID.
    pub after_id: Option<String>,
    /// Cursor: results immediately before this ID.
    pub before_id: Option<String>,
    /// Page size. Default 20, max 1000.
    pub limit: Option<u32>,
}

impl ListParams {
    pub(crate) fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q = Vec::new();
        if let Some(a) = &self.after_id {
            q.push(("after_id", a.clone()));
        }
        if let Some(b) = &self.before_id {
            q.push(("before_id", b.clone()));
        }
        if let Some(l) = self.limit {
            q.push(("limit", l.to_string()));
        }
        q
    }
}
