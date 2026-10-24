//! Authenticated per-tab native input leases shared by MCP and the extension.
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, Notify};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use ulid::Ulid;

const LEASE_TTL: Duration = Duration::from_secs(90);
const CLEANUP_RETRY: Duration = Duration::from_secs(5);
const ACK_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CLEANUP_FAILURES: u8 = 3;

#[derive(Clone)]
pub struct AgentControlService {
    token: Option<Arc<str>>,
    inner: Arc<Mutex<Inner>>,
    changed: Arc<Notify>,
}

#[derive(Default)]
struct Inner {
    leases: BTreeMap<String, Lease>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Locking,
    Locked,
    Unlocking,
    Failed(&'static str),
}

#[derive(Clone, Debug)]
struct Lease {
    id: String,
    owner: String,
    page_id: u32,
    tab_id: i64,
    target_id: String,
    phase: Phase,
    expires_at: Instant,
    persistent: bool,
    action_refs: u32,
    cleanup_failures: u8,
    error: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseView {
    pub lease_id: String,
    pub page_id: u32,
    pub tab_id: i64,
    pub target_id: String,
    pub operation: &'static str,
    pub status: &'static str,
    pub error: Option<&'static str>,
    pub expires_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseAck {
    Locked,
    Unlocked,
    Unavailable,
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseTicket {
    pub lease_id: String,
    pub action_ref: bool,
}

impl AgentControlService {
    #[must_use]
    pub fn new(token: Option<String>) -> Self {
        Self {
            token: token.filter(|token| !token.trim().is_empty()).map(Arc::from),
            inner: Arc::default(),
            changed: Arc::new(Notify::new()),
        }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.token.is_some()
    }

    /// Bearer auth is the authority. Origin checks are only defense in depth.
    #[must_use]
    pub fn authorized(&self, authorization: Option<&str>) -> bool {
        let Some(expected) = self.token.as_deref() else {
            return false;
        };
        let Some(candidate) = authorization.and_then(|value| value.strip_prefix("Bearer ")) else {
            return false;
        };
        constant_time_eq(expected.as_bytes(), candidate.as_bytes())
    }

    /// Creates or reuses an owner-scoped lease for this exact page incarnation.
    pub async fn begin(
        &self,
        owner: &str,
        page_id: u32,
        tab_id: i64,
        target_id: &str,
        persistent: bool,
    ) -> Result<LeaseTicket, &'static str> {
        self.sweep().await;
        let mut inner = self.inner.lock().await;
        if let Some(existing) = inner.leases.values_mut().find(|lease| lease.tab_id == tab_id) {
            match existing.phase {
                Phase::Locking | Phase::Locked => {}
                Phase::Unlocking => {
                    return Err("the previous tab lease is still being released; retry after native unlock");
                }
                Phase::Failed(_) => {
                    return Err("the previous tab lease has an unresolved native lock; stop or retry cleanup before reacquiring");
                }
            }
            if existing.owner != owner {
                return Err("this tab is already controlled by another agent session");
            }
            if existing.page_id != page_id || existing.target_id != target_id {
                return Err("the leased page changed; reacquire from a fresh tab snapshot");
            }
            existing.expires_at = Instant::now() + LEASE_TTL;
            existing.persistent |= persistent;
            let action_ref = !persistent;
            if action_ref {
                existing.action_refs = existing.action_refs.saturating_add(1);
            }
            return Ok(LeaseTicket {
                lease_id: existing.id.clone(),
                action_ref,
            });
        }

        let id = Ulid::new().to_string();
        inner.leases.insert(
            id.clone(),
            Lease {
                id: id.clone(),
                owner: owner.to_string(),
                page_id,
                tab_id,
                target_id: target_id.to_string(),
                phase: Phase::Locking,
                expires_at: Instant::now() + LEASE_TTL,
                persistent,
                action_refs: u32::from(!persistent),
                cleanup_failures: 0,
                error: None,
            },
        );
        self.changed.notify_waiters();
        Ok(LeaseTicket {
            lease_id: id,
            action_ref: !persistent,
        })
    }

    /// Waits until the extension has locked the exact tab/target tuple. No caller
    /// may dispatch page input while this is pending, cancelled, stale, or unavailable.
    pub async fn wait_locked(
        &self,
        owner: &str,
        lease_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        self.wait_locked_for(owner, lease_id, cancel, ACK_TIMEOUT).await
    }

    async fn wait_locked_for(
        &self,
        owner: &str,
        lease_id: &str,
        cancel: &CancellationToken,
        timeout: Duration,
    ) -> Result<(), &'static str> {
        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.changed.notified();
            {
                let mut inner = self.inner.lock().await;
                if self.sweep_locked(&mut inner) {
                    self.changed.notify_waiters();
                }
                let Some(lease) = inner.leases.get(lease_id) else {
                    return Err("agent tab lease expired or was released");
                };
                if lease.owner != owner {
                    return Err("agent tab lease owner mismatch");
                }
                match lease.phase {
                    Phase::Locked => return Ok(()),
                    Phase::Failed(reason) => return Err(reason),
                    Phase::Unlocking => {
                        return Err(lease.error.unwrap_or("agent tab lease is being released"));
                    }
                    Phase::Locking => {}
                }
            }
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err("agent tab lock wait was cancelled"),
                () = tokio::time::sleep_until(deadline) => return Err("Pip Dev could not wake the extension to lock this tab within 8 seconds, so no page input was sent. Keep a visible Pip Dev page or popup open, then retry."),
                () = notified => {}
            }
        }
    }

    pub async fn finish_action(&self, owner: &str, ticket: &LeaseTicket) {
        let _ = self.finish_action_request_unlock(owner, ticket).await;
    }

    /// Releases an ephemeral action reference and waits for native unlock ACK.
    /// A persistent user-started lease remains held and returns immediately.
    pub async fn finish_action_and_wait(
        &self,
        owner: &str,
        ticket: &LeaseTicket,
        cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        self.finish_action_and_wait_with_cancellation(owner, ticket, cancel, cancel, cancel)
            .await
    }

    pub async fn finish_action_and_wait_with_cancellation(
        &self,
        owner: &str,
        ticket: &LeaseTicket,
        cancel: &CancellationToken,
        client_cancel: &CancellationToken,
        dispatch_cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        if self.finish_action_request_unlock(owner, ticket).await {
            self.wait_unlocked(
                owner,
                &ticket.lease_id,
                cancel,
                client_cancel,
                dispatch_cancel,
            )
            .await
        } else {
            Ok(())
        }
    }

    async fn finish_action_request_unlock(&self, owner: &str, ticket: &LeaseTicket) -> bool {
        if !ticket.action_ref {
            return false;
        }
        let mut inner = self.inner.lock().await;
        let mut unlock_requested = false;
        if let Some(lease) = inner.leases.get_mut(&ticket.lease_id)
            && lease.owner == owner
        {
            lease.action_refs = lease.action_refs.saturating_sub(1);
            if lease.action_refs == 0 && !lease.persistent {
                request_unlock(lease, None);
                unlock_requested = true;
                self.changed.notify_waiters();
            }
        }
        unlock_requested
    }

    /// Requests cleanup for a cancelled action or an explicitly stopped lease.
    pub async fn abort(&self, owner: &str, lease_id: &str) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get_mut(lease_id) else {
            return Ok(());
        };
        if lease.owner != owner {
            return Err("agent tab lease owner mismatch");
        }
        lease.persistent = false;
        lease.action_refs = 0;
        request_unlock(lease, lease.error);
        self.changed.notify_waiters();
        Ok(())
    }

    pub async fn end(
        &self,
        owner: &str,
        lease_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        self.abort(owner, lease_id).await?;
        self.wait_unlocked(owner, lease_id, cancel, cancel, cancel).await
    }

    async fn wait_unlocked(
        &self,
        owner: &str,
        lease_id: &str,
        cancel: &CancellationToken,
        client_cancel: &CancellationToken,
        dispatch_cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        let deadline = Instant::now() + ACK_TIMEOUT;
        loop {
            let notified = self.changed.notified();
            {
                let inner = self.inner.lock().await;
                let Some(lease) = inner.leases.get(lease_id) else {
                    return Ok(());
                };
                if lease.owner != owner {
                    return Err("agent tab lease owner mismatch");
                }
                match lease.phase {
                    Phase::Failed(reason) => return Err(reason),
                    Phase::Locking | Phase::Locked => {
                        return Err("agent tab lease is not releasing");
                    }
                    Phase::Unlocking => {}
                }
            }
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err("agent tab unlock wait was cancelled"),
                () = client_cancel.cancelled() => return Err("agent tab unlock wait was cancelled by the client"),
                () = dispatch_cancel.cancelled() => return Err("agent tab unlock wait was cancelled by the operator"),
                () = tokio::time::sleep_until(deadline) => return Err("timed out waiting for native tab unlock acknowledgement"),
                () = notified => {}
            }
        }
    }

    pub async fn status(&self, owner: &str, lease_id: &str) -> Result<LeaseView, &'static str> {
        self.sweep().await;
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get_mut(lease_id) else {
            return Err("agent tab lease was not found");
        };
        if lease.owner != owner {
            return Err("agent tab lease owner mismatch");
        }
        if matches!(lease.phase, Phase::Locked | Phase::Locking) {
            lease.expires_at = Instant::now() + LEASE_TTL;
        }
        Ok(view(lease))
    }

    /// Returns every lease to the authenticated bundled extension so it can
    /// surface terminal failures and preserve Stop/Retry cleanup affordances.
    pub async fn leases_for_extension(&self) -> Vec<LeaseView> {
        self.sweep().await;
        let inner = self.inner.lock().await;
        inner.leases.values().map(view).collect()
    }

    /// Only pending locks and pending unlocks are handed to the extension. A
    /// terminal failure stays visible to the owning MCP caller but is no longer
    /// polled forever; explicit end/abort can start a bounded cleanup retry.
    pub async fn pending_for_extension(&self) -> Vec<LeaseView> {
        self.sweep().await;
        let inner = self.inner.lock().await;
        inner
            .leases
            .values()
            .filter(|lease| matches!(lease.phase, Phase::Locking | Phase::Unlocking))
            .map(view)
            .collect()
    }

    /// User stop/extension recovery requests cleanup for this exact lease.
    /// The bearer token authenticates the bundled extension; tuple matching prevents
    /// a stale tab id from releasing a newer incarnation.
    pub async fn request_release(
        &self,
        lease_id: &str,
        tab_id: i64,
        target_id: &str,
    ) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get_mut(lease_id) else {
            return Err("unknown or expired lease");
        };
        if lease.tab_id != tab_id || lease.target_id != target_id {
            return Err("lease release does not match the requested tab incarnation");
        }
        lease.persistent = false;
        lease.action_refs = 0;
        request_unlock(lease, lease.error);
        self.changed.notify_waiters();
        Ok(())
    }

    pub async fn acknowledge(
        &self,
        lease_id: &str,
        tab_id: i64,
        target_id: &str,
        ack: LeaseAck,
    ) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get_mut(lease_id) else {
            return Err("unknown or expired lease");
        };
        if lease.tab_id != tab_id || lease.target_id != target_id {
            return Err("lease acknowledgement does not match the requested tab incarnation");
        }
        if matches!(lease.phase, Phase::Unlocking | Phase::Failed(_)) && ack == LeaseAck::Unlocked {
            inner.leases.remove(lease_id);
            self.changed.notify_waiters();
            return Ok(());
        }
        match (&lease.phase, ack) {
            (Phase::Locking, LeaseAck::Locked) => {
                lease.phase = Phase::Locked;
                lease.error = None;
                lease.cleanup_failures = 0;
                lease.expires_at = Instant::now() + LEASE_TTL;
            }
            (Phase::Locking, LeaseAck::Unavailable) => {
                request_unlock(lease, Some("native tab lock is unavailable; cleanup is required"));
            }
            (Phase::Locking, LeaseAck::Stale) => {
                request_unlock(lease, Some("tab changed before native lock acknowledgement"));
            }
            // A lock may finish after expiry/cancel already requested unlock. Accept
            // only the exact tuple, keep the desired phase as Unlocking, and make the
            // extension's next lease-driven pass detach the just-acquired lock.
            (Phase::Unlocking, LeaseAck::Locked) | (Phase::Failed(_), LeaseAck::Locked) => {
                lease.phase = Phase::Unlocking;
                lease.error = Some("late native lock acknowledgement; immediate cleanup is pending");
                lease.cleanup_failures = 0;
                lease.expires_at = Instant::now() + CLEANUP_RETRY;
            }
            (Phase::Unlocking, LeaseAck::Unavailable | LeaseAck::Stale) => {
                record_cleanup_failure(lease);
            }
            (Phase::Failed(_), LeaseAck::Unavailable | LeaseAck::Stale) => {
                return Err("terminal cleanup failure requires an explicit retry");
            }
            (Phase::Locked, LeaseAck::Locked) => return Ok(()),
            _ => return Err("lease acknowledgement does not match its current operation"),
        }
        self.changed.notify_waiters();
        Ok(())
    }

    /// Session stop/disconnect requests native unlock for only that owner's leases,
    /// then waits a bounded interval for extension acknowledgement.
    pub async fn release_session(&self, owner: &str) {
        let ids = {
            let mut inner = self.inner.lock().await;
            let mut ids = Vec::new();
            for lease in inner.leases.values_mut().filter(|lease| lease.owner == owner) {
                lease.persistent = false;
                lease.action_refs = 0;
                request_unlock(lease, lease.error);
                ids.push(lease.id.clone());
            }
            self.changed.notify_waiters();
            ids
        };
        let cancel = CancellationToken::new();
        for lease_id in ids {
            let _ = self
                .wait_unlocked(owner, &lease_id, &cancel, &cancel, &cancel)
                .await;
        }
    }

    async fn sweep(&self) {
        let mut inner = self.inner.lock().await;
        if self.sweep_locked(&mut inner) {
            self.changed.notify_waiters();
        }
    }

    fn sweep_locked(&self, inner: &mut Inner) -> bool {
        let now = Instant::now();
        let mut changed = false;
        for lease in inner.leases.values_mut() {
            if lease.expires_at > now {
                continue;
            }
            match lease.phase {
                Phase::Locking => {
                    request_unlock(
                        lease,
                        Some("native lock acknowledgement timed out; cleanup is pending"),
                    );
                }
                Phase::Locked => request_unlock(lease, None),
                Phase::Unlocking => record_cleanup_failure(lease),
                Phase::Failed(_) => {}
            }
            changed = true;
        }
        changed
    }
}

fn request_unlock(lease: &mut Lease, error: Option<&'static str>) {
    lease.phase = Phase::Unlocking;
    lease.error = error;
    lease.cleanup_failures = 0;
    lease.expires_at = Instant::now() + CLEANUP_RETRY;
}

fn record_cleanup_failure(lease: &mut Lease) {
    lease.cleanup_failures = lease.cleanup_failures.saturating_add(1);
    if lease.cleanup_failures >= MAX_CLEANUP_FAILURES {
        lease.phase = Phase::Failed(
            "native unlock could not be confirmed after bounded retries; stop Pip Dev or retry lease cleanup",
        );
        lease.error = Some("native unlock acknowledgement failed");
        lease.expires_at = Instant::now() + LEASE_TTL;
    } else {
        lease.phase = Phase::Unlocking;
        lease.error = Some("native unlock acknowledgement failed; retrying cleanup");
        lease.expires_at = Instant::now() + CLEANUP_RETRY;
    }
}

fn view(lease: &Lease) -> LeaseView {
    let (operation, status) = match lease.phase {
        Phase::Locking => ("lock", "locking"),
        Phase::Locked => ("lock", "locked"),
        Phase::Unlocking => ("unlock", "unlocking"),
        Phase::Failed(_) => ("unlock", "failed"),
    };
    let remaining = lease.expires_at.saturating_duration_since(Instant::now());
    let expires_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .saturating_add(remaining)
        .as_millis() as u64;
    LeaseView {
        lease_id: lease.id.clone(),
        page_id: lease.page_id,
        tab_id: lease.tab_id,
        target_id: lease.target_id.clone(),
        operation,
        status,
        error: lease.error,
        expires_at_ms,
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let max = left.len().max(right.len());
    for index in 0..max {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> AgentControlService {
        AgentControlService::new(Some("test-only-secret".to_string()))
    }

    #[tokio::test]
    async fn bearer_is_required_and_origin_is_not_authority() {
        let service = service();
        assert!(!service.authorized(None));
        assert!(!service.authorized(Some("Bearer wrong")));
        assert!(!service.authorized(Some("Bearer test-only-secreT")));
        assert!(service.authorized(Some("Bearer test-only-secret")));
    }

    #[tokio::test]
    async fn acknowledgement_must_match_exact_tab_and_target_before_locking() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("session-a", 1, 42, "target-a", true).await?;
        assert!(service
            .acknowledge(&ticket.lease_id, 43, "target-a", LeaseAck::Locked)
            .await
            .is_err());
        assert!(service
            .acknowledge(&ticket.lease_id, 42, "target-b", LeaseAck::Locked)
            .await
            .is_err());
        assert_eq!(service.pending_for_extension().await.len(), 1);
        service
            .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked)
            .await?;
        service
            .wait_locked("session-a", &ticket.lease_id, &CancellationToken::new())
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn another_session_cannot_acquire_or_release_an_owned_tab() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        assert!(service.begin("other", 1, 42, "target-a", true).await.is_err());
        assert!(service
            .end("other", &ticket.lease_id, &CancellationToken::new())
            .await
            .is_err());
        assert_eq!(service.pending_for_extension().await.len(), 1);
        assert_eq!(service.status("owner", &ticket.lease_id).await?.status, "locking");
        Ok(())
    }

    #[tokio::test]
    async fn every_unresolved_phase_blocks_same_tab_reacquisition() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_ok());
        assert!(service.begin("other", 1, 42, "target-a", true).await.is_err());
        service
            .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked)
            .await?;
        service.abort("owner", &ticket.lease_id).await?;
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_err());
        for _ in 0..MAX_CLEANUP_FAILURES {
            service
                .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Unavailable)
                .await
                .unwrap_or_else(|_| panic!("unlock ACK should be accepted during cleanup"));
            // A failed cleanup can be retried only explicitly; this test advances it
            // through expiry via the private sweep path without sleeping.
            if service.status("owner", &ticket.lease_id).await?.status == "failed" {
                break;
            }
            {
                let mut inner = service.inner.lock().await;
                inner.leases.get_mut(&ticket.lease_id).unwrap().expires_at = Instant::now();
            }
            service.sweep().await;
        }
        assert_eq!(service.status("owner", &ticket.lease_id).await?.status, "failed");
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_err());
        service.abort("owner", &ticket.lease_id).await?;
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_err());
        service
            .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Unlocked)
            .await?;
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn missing_extension_wakeup_times_out_before_any_lock_ack() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", false).await?;

        let error = service
            .wait_locked_for(
                "owner",
                &ticket.lease_id,
                &CancellationToken::new(),
                Duration::from_millis(1),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            "Pip Dev could not wake the extension to lock this tab within 8 seconds, so no page input was sent. Keep a visible Pip Dev page or popup open, then retry."
        );
        assert_eq!(service.status("owner", &ticket.lease_id).await?.status, "locking");
        service.abort("owner", &ticket.lease_id).await?;
        assert_eq!(service.pending_for_extension().await[0].operation, "unlock");
        Ok(())
    }

    #[tokio::test]
    async fn cancellation_requests_exact_unlock_and_late_lock_never_wakes_waiter() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        service.abort("owner", &ticket.lease_id).await?;
        service
            .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked)
            .await?;
        assert!(service
            .wait_locked("owner", &ticket.lease_id, &CancellationToken::new())
            .await
            .is_err());
        let cleanup = service.pending_for_extension().await;
        assert_eq!(cleanup.len(), 1);
        assert_eq!(cleanup[0].operation, "unlock");
        assert_eq!(cleanup[0].tab_id, 42);
        assert_eq!(cleanup[0].target_id, "target-a");
        service
            .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Unlocked)
            .await?;
        assert!(service.pending_for_extension().await.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn stale_and_unavailable_lock_results_transition_to_bounded_cleanup() -> anyhow::Result<()> {
        for result in [LeaseAck::Unavailable, LeaseAck::Stale] {
            let service = service();
            let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
            service
                .acknowledge(&ticket.lease_id, 42, "target-a", result)
                .await?;
            assert!(service
                .wait_locked("owner", &ticket.lease_id, &CancellationToken::new())
                .await
                .is_err());
            assert_eq!(service.pending_for_extension().await[0].operation, "unlock");
            for retry in 0..MAX_CLEANUP_FAILURES {
                let _ = service
                    .acknowledge(
                        &ticket.lease_id,
                        42,
                        "target-a",
                        LeaseAck::Unavailable,
                    )
                    .await;
                if retry + 1 < MAX_CLEANUP_FAILURES {
                    let mut inner = service.inner.lock().await;
                    inner.leases.get_mut(&ticket.lease_id).unwrap().expires_at = Instant::now();
                    drop(inner);
                    service.sweep().await;
                }
            }
            assert_eq!(service.status("owner", &ticket.lease_id).await?.status, "failed");
            assert!(service.pending_for_extension().await.is_empty());
        }
        Ok(())
    }

    #[tokio::test]
    async fn expired_lock_lease_requires_unlock_before_reacquisition() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        {
            let mut inner = service.inner.lock().await;
            inner.leases.get_mut(&ticket.lease_id).unwrap().expires_at = Instant::now();
        }
        let pending = service.pending_for_extension().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].operation, "unlock");
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_err());
        service
            .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Unlocked)
            .await?;
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn session_stop_releases_only_its_leases_and_waits_for_ack() -> anyhow::Result<()> {
        let service = service();
        let owned = service.begin("owner", 1, 42, "target-a", true).await?;
        let other = service.begin("other", 2, 43, "target-b", true).await?;
        let stopping = {
            let service = service.clone();
            tokio::spawn(async move { service.release_session("owner").await })
        };
        tokio::task::yield_now().await;
        let pending = service.pending_for_extension().await;
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().any(|lease| lease.lease_id == owned.lease_id && lease.operation == "unlock"));
        assert!(pending.iter().any(|lease| lease.lease_id == other.lease_id && lease.operation == "lock"));
        service
            .acknowledge(&owned.lease_id, 42, "target-a", LeaseAck::Unlocked)
            .await?;
        stopping.await?;
        assert_eq!(service.status("other", &other.lease_id).await?.status, "locking");
        Ok(())
    }

    #[tokio::test]
    async fn external_stop_release_requires_exact_tab_incarnation() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        assert!(service.request_release(&ticket.lease_id, 43, "target-a").await.is_err());
        assert!(service.request_release(&ticket.lease_id, 42, "target-b").await.is_err());
        assert_eq!(service.status("owner", &ticket.lease_id).await?.status, "locking");
        service.request_release(&ticket.lease_id, 42, "target-a").await?;
        assert_eq!(service.status("owner", &ticket.lease_id).await?.operation, "unlock");
        Ok(())
    }

    #[tokio::test]
    async fn wrong_owner_tab_or_target_never_changes_lease() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        assert!(service.status("other", &ticket.lease_id).await.is_err());
        assert!(service.abort("other", &ticket.lease_id).await.is_err());
        assert!(service
            .acknowledge(&ticket.lease_id, 99, "target-a", LeaseAck::Locked)
            .await
            .is_err());
        assert!(service
            .acknowledge(&ticket.lease_id, 42, "target-z", LeaseAck::Locked)
            .await
            .is_err());
        let status = service.status("owner", &ticket.lease_id).await?;
        assert_eq!(status.status, "locking");
        assert_eq!(status.target_id, "target-a");
        Ok(())
    }

    #[tokio::test]
    async fn ephemeral_action_waits_for_native_unlock_ack() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", false).await?;
        service
            .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked)
            .await?;

        let finishing = {
            let service = service.clone();
            let ticket = ticket.clone();
            tokio::spawn(async move {
                service
                    .finish_action_and_wait("owner", &ticket, &CancellationToken::new())
                    .await
            })
        };
        tokio::task::yield_now().await;
        let pending = service.pending_for_extension().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].operation, "unlock");
        assert!(!finishing.is_finished());

        service
            .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Unlocked)
            .await?;
        finishing.await??;
        assert!(service.status("owner", &ticket.lease_id).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn unlock_wait_honors_client_and_operator_cancellation() -> anyhow::Result<()> {
        for cancel_client in [true, false] {
            let service = service();
            let ticket = service.begin("owner", 1, 42, "target-a", false).await?;
            service
                .acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked)
                .await?;
            service.finish_action("owner", &ticket).await;

            let session_cancel = CancellationToken::new();
            let client_cancel = CancellationToken::new();
            let dispatch_cancel = CancellationToken::new();
            let finishing = {
                let service = service.clone();
                let ticket = ticket.clone();
                let session_cancel = session_cancel.clone();
                let client_cancel = client_cancel.clone();
                let dispatch_cancel = dispatch_cancel.clone();
                tokio::spawn(async move {
                    service
                        .finish_action_and_wait_with_cancellation(
                            "owner",
                            &ticket,
                            &session_cancel,
                            &client_cancel,
                            &dispatch_cancel,
                        )
                        .await
                })
            };
            tokio::task::yield_now().await;
            assert_eq!(service.pending_for_extension().await[0].operation, "unlock");

            if cancel_client {
                client_cancel.cancel();
            } else {
                dispatch_cancel.cancel();
            }
            let error = finishing.await?.unwrap_err();
            assert!(
                error.contains("cancelled by the client")
                    || error.contains("cancelled by the operator")
            );
            assert_eq!(service.pending_for_extension().await[0].operation, "unlock");
        }
        Ok(())
    }

    #[tokio::test]
    async fn action_references_unlock_after_last_action_but_not_while_persistent() -> anyhow::Result<()> {
        let service = service();
        let persistent = service.begin("owner", 1, 42, "target-a", true).await?;
        let action_a = service.begin("owner", 1, 42, "target-a", false).await?;
        let action_b = service.begin("owner", 1, 42, "target-a", false).await?;
        service
            .acknowledge(&persistent.lease_id, 42, "target-a", LeaseAck::Locked)
            .await?;
        service.finish_action("owner", &action_a).await;
        service.finish_action("owner", &action_b).await;
        assert_eq!(service.status("owner", &persistent.lease_id).await?.status, "locked");
        Ok(())
    }
}
