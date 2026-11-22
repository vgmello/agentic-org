//! Browser multi-person account, invitation, password reset, access removal, and
//! ownership/administrator handover contracts (`TASK-38` / A6; admitted web connection only).

use std::net::IpAddr;

use org_proto::{Call, Empty};
use serde::{Deserialize, Serialize};

use crate::browser::{BrowserOrigin, BrowserSession, Digest32};
use crate::organization::BrowserAuth;
use crate::{LinkToken, OneTimeLinkUrl, Password};

/// Kind of one-time link inspected by `/api/v1/links/inspect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectLinkKind {
    Registration,
    SetPassword,
}

/// `link.inspect`: non-consuming preview of a one-time registration or set-password link so
/// the browser can render the right heading ("You're invited to Lumen Labs as Sam Ortiz" or
/// "Set a new password for alex") without leaking anything for dead/unknown tokens.
pub struct LinkInspect;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkInspectParams {
    pub link: LinkToken,
    pub kind: InspectLinkKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkInspectResult {
    AdministratorRegistration,
    OrganizationInvite {
        org_id: String,
        org_name: String,
        member_id: String,
    },
    SetPassword {
        login: String,
    },
}

impl Call for LinkInspect {
    const METHOD: &'static str = "link.inspect";
    type Params = LinkInspectParams;
    type Result = LinkInspectResult;
}

/// `link.register_attach` (SIGN-35): redeem an organization invitation link by signing in
/// with an existing account on this HQ, tying that account to the invited person record.
pub struct LinkRegisterAttach;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkRegisterAttachParams {
    pub link: LinkToken,
    pub login: String,
    pub password: Password,
    pub client_address: IpAddr,
    pub cookie_digest: Digest32,
    pub proof_digest: Digest32,
    pub origin: BrowserOrigin,
}

impl Call for LinkRegisterAttach {
    const METHOD: &'static str = "link.register_attach";
    type Params = LinkRegisterAttachParams;
    type Result = BrowserSession;
}

/// `link.set_password`: redeem a one-time set-password link (from `admin reset` or People ›
/// New sign-in link). Ends every session of the account, clears its sign-in throttle, and
/// returns its display login so the browser can pre-fill sign-in.
pub struct LinkSetPassword;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSetPasswordParams {
    pub link: LinkToken,
    pub password: Password,
    pub client_address: IpAddr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSetPasswordResult {
    pub login: String,
}

impl Call for LinkSetPassword {
    const METHOD: &'static str = "link.set_password";
    type Params = LinkSetPasswordParams;
    type Result = LinkSetPasswordResult;
}

/// `browser.change_password`: signed-in person changes their own password, ending every other
/// session of that account while keeping the current session live.
pub struct BrowserChangePassword;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserChangePasswordParams {
    pub auth: BrowserAuth,
    pub current_password: Password,
    pub new_password: Password,
}

impl Call for BrowserChangePassword {
    const METHOD: &'static str = "browser.change_password";
    type Params = BrowserChangePasswordParams;
    type Result = Empty;
}

/// `browser.account.preferences.set`: update the signed-in account's persisted IANA time zone
/// (`FC-008`) and/or last authorized organization (`OPEN-1`, `FC-015`).
pub struct BrowserAccountPreferencesSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAccountPreferencesSetParams {
    pub auth: BrowserAuth,
    #[serde(default)]
    pub time_zone: Option<String>,
    #[serde(default)]
    pub clear_time_zone: bool,
    #[serde(default)]
    pub last_org_id: Option<String>,
    #[serde(default)]
    pub clear_last_org_id: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAccountPreferencesSetResult {
    pub time_zone: Option<String>,
    pub last_org_id: Option<String>,
}

impl Call for BrowserAccountPreferencesSet {
    const METHOD: &'static str = "browser.account.preferences.set";
    type Params = BrowserAccountPreferencesSetParams;
    type Result = BrowserAccountPreferencesSetResult;
}

/// One person's sign-in access state inside a specific organization (never lists instance-wide
/// accounts outside this organization).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgAccessEntry {
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    pub admin: bool,
    pub owner: bool,
    pub pending_invite: bool,
    pub pending_password_reset: bool,
}

/// `browser.org_access_list`: list account links and pending invites/resets for people inside
/// one organization (organization owner or instance administrator).
pub struct BrowserOrgAccessList;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOrgAccessListParams {
    pub auth: BrowserAuth,
    pub org_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOrgAccessListResult {
    pub entries: Vec<OrgAccessEntry>,
}

impl Call for BrowserOrgAccessList {
    const METHOD: &'static str = "browser.org_access_list";
    type Params = BrowserOrgAccessListParams;
    type Result = BrowserOrgAccessListResult;
}

/// One-time link returned once to an owner or administrator (debug-redacted).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssuedLinkResult {
    pub url: OneTimeLinkUrl,
    pub expires_at_ms: i64,
}

/// `browser.org_invite`: issue a one-time registration link for a person record inside an
/// organization (owner or administrator). Cancels any prior pending invite for that person.
pub struct BrowserOrgInvite;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOrgInviteParams {
    pub auth: BrowserAuth,
    pub org_id: String,
    pub member_id: String,
}

impl Call for BrowserOrgInvite {
    const METHOD: &'static str = "browser.org_invite";
    type Params = BrowserOrgInviteParams;
    type Result = IssuedLinkResult;
}

/// `browser.org_password_reset_link`: issue a one-time set-password link for a person who has
/// an account linked to this organization (owner or administrator).
pub struct BrowserOrgPasswordResetLink;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOrgPasswordResetLinkParams {
    pub auth: BrowserAuth,
    pub org_id: String,
    pub member_id: String,
}

impl Call for BrowserOrgPasswordResetLink {
    const METHOD: &'static str = "browser.org_password_reset_link";
    type Params = BrowserOrgPasswordResetLinkParams;
    type Result = IssuedLinkResult;
}

/// `browser.org_remove_access`: unlink a person's account from an organization and sign that
/// account out everywhere (owner or administrator). Refuses with `conflict` for the org owner.
pub struct BrowserOrgRemoveAccess;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOrgRemoveAccessParams {
    pub auth: BrowserAuth,
    pub org_id: String,
    pub member_id: String,
}

impl Call for BrowserOrgRemoveAccess {
    const METHOD: &'static str = "browser.org_remove_access";
    type Params = BrowserOrgRemoveAccessParams;
    type Result = Empty;
}

/// `browser.org_owner_handover`: organization owner hands ownership to another person in the
/// same organization who already has an active account, confirmed with the owner's password.
pub struct BrowserOrgOwnerHandover;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOrgOwnerHandoverParams {
    pub auth: BrowserAuth,
    pub org_id: String,
    pub to_member_id: String,
    pub password: Password,
}

impl Call for BrowserOrgOwnerHandover {
    const METHOD: &'static str = "browser.org_owner_handover";
    type Params = BrowserOrgOwnerHandoverParams;
    type Result = Empty;
}

/// `browser.admin_handover`: instance administrator hands the administrator role to another
/// active account sharing at least one organization, confirmed with the administrator's password.
pub struct BrowserAdminHandover;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAdminHandoverParams {
    pub auth: BrowserAuth,
    pub to_account_id: String,
    pub password: Password,
}

impl Call for BrowserAdminHandover {
    const METHOD: &'static str = "browser.admin_handover";
    type Params = BrowserAdminHandoverParams;
    type Result = Empty;
}
