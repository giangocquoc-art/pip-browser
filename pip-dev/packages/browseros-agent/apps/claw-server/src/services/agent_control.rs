//! Authenticated per-tab native input leases shared by MCP and the extension.
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::sync::{Mutex, Notify};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use ulid::Ulid;

const LEASE_TTL: Duration = Duration::from_secs(90);
const ACK_TIMEOUT: Duration = Duration::from_secs(8);

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
enum Phase { Locking, Locked, Unlocking, Failed(&'static str) }

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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseView {
    pub lease_id: String,
    pub page_id: u32,
    pub tab_id: i64,
    pub target_id: String,
    pub operation: &'static str,
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
        let Some(expected) = self.token.as_deref() else { return false };
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
            if !matches!(existing.phase, Phase::Locking | Phase::Locked) {
                return Err("the previous tab lease is unresolved; wait for native unlock acknowledgement");
            }
            if existing.owner != owner {
                return Err("this tab is already controlled by another agent session");
            }
            if existing.page_id != page_id || existing.target_id != target_id {
                return Err("the leased page changed; reacquire from a fresh tab snapshot");
            }
            if matches!(existing.phase, Phase::Unlocking) {
                return Err("the previous tab lease is still being released; retry after it settles");
            }
            existing.expires_at = Instant::now() + LEASE_TTL;
            existing.persistent |= persistent;
            let action_ref = !persistent;
            if action_ref { existing.action_refs = existing.action_refs.saturating_add(1); }
            return Ok(LeaseTicket { lease_id: existing.id.clone(), action_ref });
        }
        let id = Ulid::new().to_string();
        inner.leases.insert(id.clone(), Lease {
            id: id.clone(),
            owner: owner.to_string(),
            page_id,
            tab_id,
            target_id: target_id.to_string(),
            phase: Phase::Locking,
            expires_at: Instant::now() + LEASE_TTL,
            persistent,
            action_refs: u32::from(!persistent),
        });
        self.changed.notify_waiters();
        Ok(LeaseTicket { lease_id: id, action_ref: !persistent })
    }

    /// Waits until the extension has locked the exact tab/target tuple. No caller
    /// may dispatch page input while this is pending, cancelled, stale, or unavailable.
    pub async fn wait_locked(
        &self,
        owner: &str,
        lease_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        let deadline = Instant::now() + ACK_TIMEOUT;
        loop {
            let notified = self.changed.notified();
            {
                let mut inner = self.inner.lock().await;
                self.sweep_locked(&mut inner);
                let Some(lease) = inner.leases.get(lease_id) else {
                    return Err("agent tab lease expired or was released");
                };
                if lease.owner != owner { return Err("agent tab lease owner mismatch"); }
                match lease.phase {
                    Phase::Locked => return Ok(()),
                    Phase::Failed(reason) => return Err(reason),
                    Phase::Unlocking => return Err("agent tab lease is being released"),
                    Phase::Locking => {}
                }
            }
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err("agent tab lock wait was cancelled"),
                () = tokio::time::sleep_until(deadline) => return Err("timed out waiting for native tab lock acknowledgement"),
                () = notified => {}
            }
        }
    }

    pub async fn finish_action(&self, owner: &str, ticket: &LeaseTicket) {
        if !ticket.action_ref { return; }
        let mut inner = self.inner.lock().await;
        if let Some(lease) = inner.leases.get_mut(&ticket.lease_id)
            && lease.owner == owner
        {
            lease.action_refs = lease.action_refs.saturating_sub(1);
            if lease.action_refs == 0 && !lease.persistent {
                lease.phase = Phase::Unlocking;
                lease.expires_at = Instant::now() + LEASE_TTL;
                self.changed.notify_waiters();
            }
        }
    }

    pub async fn abort(&self, owner: &str, lease_id: &str) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get_mut(lease_id) else { return Ok(()) };
        if lease.owner != owner { return Err("agent tab lease owner mismatch"); }
        lease.persistent = false;
        lease.action_refs = 0;
        lease.phase = Phase::Unlocking;
        lease.expires_at = Instant::now() + LEASE_TTL;
        self.changed.notify_waiters();
        Ok(())
    }

    pub async fn end(
        &self,
        owner: &str,
        lease_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        {
            let mut inner = self.inner.lock().await;
            let Some(lease) = inner.leases.get_mut(lease_id) else {
                return Err("agent tab lease was not found");
            };
            if lease.owner != owner { return Err("agent tab lease owner mismatch"); }
            lease.persistent = false;
            lease.action_refs = 0;
            lease.phase = Phase::Unlocking;
            lease.expires_at = Instant::now() + LEASE_TTL;
            self.changed.notify_waiters();
        }
        self.wait_unlocked(owner, lease_id, cancel).await
    }

    async fn wait_unlocked(
        &self,
        owner: &str,
        lease_id: &str,
        cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        let deadline = Instant::now() + ACK_TIMEOUT;
        loop {
            let notified = self.changed.notified();
            {
                let inner = self.inner.lock().await;
                let Some(lease) = inner.leases.get(lease_id) else { return Ok(()) };
                if lease.owner != owner { return Err("agent tab lease owner mismatch"); }
                match lease.phase {
                    Phase::Failed(reason) => return Err(reason),
                    Phase::Locking | Phase::Locked => return Err("agent tab lease is not releasing"),
                    Phase::Unlocking => {}
                }
            }
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err("agent tab unlock wait was cancelled"),
                () = tokio::time::sleep_until(deadline) => return Err("timed out waiting for native tab unlock acknowledgement"),
                () = notified => {}
            }
        }
    }

    pub async fn status(&self, owner: &str, lease_id: &str) -> Result<LeaseView, &'static str> {
        self.sweep().await;
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get_mut(lease_id) else { return Err("agent tab lease was not found") };
        if lease.owner != owner { return Err("agent tab lease owner mismatch"); }
        if matches!(lease.phase, Phase::Locked | Phase::Locking) {
            lease.expires_at = Instant::now() + LEASE_TTL;
        }
        Ok(view(lease))
    }

    /// Only pending locks and pending unlocks are handed to the extension. It
    /// never walks or polls arbitrary hidden browser tabs.
    pub async fn pending_for_extension(&self) -> Vec<LeaseView> {
        self.sweep().await;
        let inner = self.inner.lock().await;
        inner.leases.values()
            .filter(|lease| matches!(lease.phase, Phase::Locking | Phase::Unlocking | Phase::Failed(_)))
            .map(view)
            .collect()
    }

    pub async fn acknowledge(
        &self,
        lease_id: &str,
        tab_id: i64,
        target_id: &str,
        ack: LeaseAck,
    ) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get_mut(lease_id) else { return Err("unknown or expired lease") };
        if lease.tab_id != tab_id || lease.target_id != target_id {
            return Err("lease acknowledgement does not match the requested tab incarnation");
        }
        match (&lease.phase, ack) {
            (Phase::Locking, LeaseAck::Locked) => lease.phase = Phase::Locked,
            (Phase::Locking, LeaseAck::Unavailable) => lease.phase = Phase::Failed("native tab lock is unavailable"),
            (Phase::Locking, LeaseAck::Stale) => lease.phase = Phase::Failed("tab changed before the native lock was acknowledged"),
            (Phase::Unlocking, LeaseAck::Unlocked) | (Phase::Failed(_), LeaseAck::Unlocked) => {
                inner.leases.remove(lease_id);
                self.changed.notify_waiters();
                return Ok(());
            }
            // A lock may finish after expiry/cancel already requested unlock. Accept
            // only the exact tuple, keep the desired phase as Unlocking, and make the
            // extension's next lease-driven pass detach the just-acquired lock.
            (Phase::Unlocking, LeaseAck::Locked) | (Phase::Failed(_), LeaseAck::Locked) => {
                lease.phase = Phase::Unlocking;
            }
            (Phase::Unlocking, LeaseAck::Unavailable) => return Ok(()),
            (Phase::Failed(_), LeaseAck::Unavailable) | (Phase::Failed(_), LeaseAck::Stale) => return Ok(()),
            (Phase::Locked, LeaseAck::Locked) => return Ok(()),
            (Phase::Locking, LeaseAck::Unlocked) => return Err("lease acknowledgement does not match its current operation"),
            _ => return Err("lease acknowledgement does not match its current operation"),
        }
        self.changed.notify_waiters();
        Ok(())
    }

    /// Session stop/disconnect requests native unlock for only that owner's leases.
    pub async fn release_session(&self, owner: &str) {
        let mut inner = self.inner.lock().await;
        for lease in inner.leases.values_mut().filter(|lease| lease.owner == owner) {
            lease.persistent = false;
            lease.action_refs = 0;
            if matches!(lease.phase, Phase::Locking | Phase::Locked) {
                lease.phase = Phase::Unlocking;
                lease.expires_at = Instant::now() + LEASE_TTL;
            }
        }
        self.changed.notify_waiters();
    }

    async fn sweep(&self) {
        let mut inner = self.inner.lock().await;
        if self.sweep_locked(&mut inner) { self.changed.notify_waiters(); }
    }

    fn sweep_locked(&self, inner: &mut Inner) -> bool {
        let now = Instant::now();
        let mut changed = false;
        for lease in inner.leases.values_mut() {
            if lease.expires_at > now { continue; }
            match lease.phase {
                Phase::Locking => lease.phase = Phase::Unlocking,
                Phase::Locked => { lease.phase = Phase::Unlocking; lease.expires_at = now + LEASE_TTL; }
                Phase::Unlocking => lease.expires_at = now + LEASE_TTL,
                Phase::Failed(_) => {}
            }
            changed = true;
        }
        changed
    }
}

fn view(lease: &Lease) -> LeaseView {
    let operation = if matches!(lease.phase, Phase::Unlocking | Phase::Failed(_)) { "unlock" } else { "lock" };
    let remaining = lease.expires_at.saturating_duration_since(Instant::now());
    let expires_at_ms = SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().saturating_add(remaining).as_millis() as u64;
    LeaseView {
        lease_id: lease.id.clone(),
        page_id: lease.page_id,
        tab_id: lease.tab_id,
        target_id: lease.target_id.clone(),
        operation,
        expires_at_ms,
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let max = left.len().max(right.len());
    for index in 0..max {
        difference |= usize::from(left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0));
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert!(service.acknowledge(&ticket.lease_id, 43, "target-a", LeaseAck::Locked).await.is_err());
        assert!(service.acknowledge(&ticket.lease_id, 42, "target-b", LeaseAck::Locked).await.is_err());
        assert_eq!(service.pending_for_extension().await.len(), 1);
        service.acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked).await?;
        service.wait_locked("session-a", &ticket.lease_id, &CancellationToken::new()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn another_session_cannot_acquire_or_release_an_owned_tab() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        assert!(service.begin("other", 1, 42, "target-a", true).await.is_err());
        assert!(service.end("other", &ticket.lease_id, &CancellationToken::new()).await.is_err());
        service.acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked).await?;
        service.release_session("other").await;
        assert_eq!(service.pending_for_extension().await.len(), 0);
        assert_eq!(service.status("owner", &ticket.lease_id).await?.operation, "lock");
        Ok(())
    }

    #[tokio::test]
    async fn cancellation_and_missing_ack_fail_closed() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", false).await?;
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(service.wait_locked("owner", &ticket.lease_id, &cancel).await.is_err());
        service.abort("owner", &ticket.lease_id).await?;
        assert_eq!(service.pending_for_extension().await[0].operation, "unlock");
        Ok(())
    }

    #[tokio::test]
    async fn late_lock_ack_after_cancel_never_satisfies_lock_wait_and_requires_unlock() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        service.abort("owner", &ticket.lease_id).await?;
        service.acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked).await?;
        assert!(service.wait_locked("owner", &ticket.lease_id, &CancellationToken::new()).await.is_err());
        let cleanup = service.pending_for_extension().await;
        assert_eq!(cleanup.len(), 1);
        assert_eq!(cleanup[0].operation, "unlock");
        Ok(())
    }

    #[tokio::test]
    async fn every_unresolved_phase_blocks_same_tab_reacquisition() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        service.acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked).await?;
        service.abort("owner", &ticket.lease_id).await?;
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_err());
        service.acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Unlocked).await?;
        assert!(service.begin("owner", 1, 42, "target-a", true).await.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn explicit_end_is_pending_until_exact_unlock_ack() -> anyhow::Result<()> {
        let service = service();
        let ticket = service.begin("owner", 1, 42, "target-a", true).await?;
        service.acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Locked).await?;
        let ending = {
            let service = service.clone();
            let id = ticket.lease_id.clone();
            tokio::spawn(async move { service.end("owner", &id, &CancellationToken::new()).await })
        };
        tokio::task::yield_now().await;
        let pending = service.pending_for_extension().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].operation, "unlock");
        service.acknowledge(&ticket.lease_id, 42, "target-a", LeaseAck::Unlocked).await?;
        ending.await??;
        Ok(())
    }
}
