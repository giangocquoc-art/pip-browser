//! Authenticated per-tab native input leases shared by MCP and the extension.
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
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
    required: bool,
    inner: Arc<Mutex<Inner>>,
    changed: Arc<Notify>,
}

#[derive(Default)]
struct Inner {
    leases: BTreeMap<String, Lease>,
    snapshot_revisions: BTreeMap<i64, u64>,
    stopped_tabs: BTreeSet<(i64, String)>,
    draining_tabs: BTreeSet<i64>,
    active_dispatches: BTreeMap<String, BTreeMap<String, CancellationToken>>,
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
    generation: u64,
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
    pub generation: u64,
    pub snapshot_revision: u64,
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
    pub generation: u64,
    pub action_ref: bool,
}

impl AgentControlService {
    #[must_use]
    pub fn new(token: Option<String>) -> Self {
        Self::new_with_policy(token, false)
    }

    #[must_use]
    pub fn new_with_policy(token: Option<String>, required: bool) -> Self {
        Self {
            token: token
                .filter(|token| !token.trim().is_empty())
                .map(Arc::from),
            required,
            inner: Arc::default(),
            changed: Arc::new(Notify::new()),
        }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.token.is_some()
    }

    #[must_use]
    pub fn is_required(&self) -> bool {
        self.required
    }

    /// Whether page-facing operations must pass through native control.
    #[must_use]
    pub fn requires_control(&self) -> bool {
        self.required || self.is_enabled()
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
        if inner
            .stopped_tabs
            .contains(&(tab_id, target_id.to_string()))
        {
            return Err(
                "agent control was stopped for this tab; restart Pip Dev before reacquiring control",
            );
        }
        if inner.draining_tabs.contains(&tab_id) {
            return Err(
                "the current page document changed; native unlock is still being confirmed",
            );
        }
        if let Some(existing_id) = inner
            .leases
            .values()
            .find(|lease| lease.tab_id == tab_id)
            .map(|lease| lease.id.clone())
        {
            let ticket = {
                let existing = inner
                    .leases
                    .get_mut(&existing_id)
                    .expect("lease id was collected from this map");
                match existing.phase {
                    Phase::Locking | Phase::Locked => {}
                    Phase::Unlocking => {
                        return Err(
                            "the previous tab lease is still being released; retry after native unlock",
                        );
                    }
                    Phase::Failed(_) => {
                        return Err(
                            "the previous tab lease has an unresolved native lock; stop or retry cleanup before reacquiring",
                        );
                    }
                }
                if existing.owner != owner {
                    return Err("this tab is already controlled by another agent session");
                }
                if existing.page_id != page_id || existing.target_id != target_id {
                    return Err("the leased page changed; reacquire from a fresh tab snapshot");
                }
                let action_ref = !persistent;
                let next_generation = if action_ref {
                    Some(
                        existing
                            .generation
                            .checked_add(1)
                            .ok_or("agent tab lock generation exhausted")?,
                    )
                } else {
                    None
                };
                existing.expires_at = Instant::now() + LEASE_TTL;
                existing.persistent |= persistent;
                if let Some(generation) = next_generation {
                    // Each action gets a distinct ACK generation. This invalidates
                    // delayed results from overlapping extension polls.
                    existing.generation = generation;
                    existing.action_refs = existing.action_refs.saturating_add(1);
                    existing.phase = Phase::Locking;
                    existing.error = None;
                }
                LeaseTicket {
                    lease_id: existing.id.clone(),
                    generation: existing.generation,
                    action_ref,
                }
            };
            bump_snapshot_revision(&mut inner, tab_id);
            if ticket.action_ref {
                self.changed.notify_waiters();
            }
            return Ok(ticket);
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
                generation: 1,
                phase: Phase::Locking,
                expires_at: Instant::now() + LEASE_TTL,
                persistent,
                action_refs: u32::from(!persistent),
                cleanup_failures: 0,
                error: None,
            },
        );
        bump_snapshot_revision(&mut inner, tab_id);
        self.changed.notify_waiters();
        Ok(LeaseTicket {
            lease_id: id,
            generation: 1,
            action_ref: !persistent,
        })
    }

    /// Waits until the extension has locked the exact tab/target tuple. No caller
    /// may dispatch page input while this is pending, cancelled, stale, or unavailable.
    pub async fn wait_locked(
        &self,
        owner: &str,
        lease_id: &str,
        generation: u64,
        cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        self.wait_locked_for(owner, lease_id, generation, cancel, ACK_TIMEOUT)
            .await
    }

    async fn wait_locked_for(
        &self,
        owner: &str,
        lease_id: &str,
        generation: u64,
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
                if lease.generation != generation {
                    return Err("agent tab lock generation changed; retry the action");
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
        let Some(tab_id) = inner
            .leases
            .get(&ticket.lease_id)
            .filter(|lease| lease.owner == owner)
            .map(|lease| lease.tab_id)
        else {
            return false;
        };
        bump_snapshot_revision(&mut inner, tab_id);
        let mut unlock_requested = false;
        if let Some(lease) = inner.leases.get_mut(&ticket.lease_id) {
            lease.action_refs = lease.action_refs.saturating_sub(1);
            if lease.action_refs == 0 && !lease.persistent {
                request_unlock(lease, None);
                unlock_requested = true;
                self.changed.notify_waiters();
            }
        }
        unlock_requested
    }

    /// Aborts an action only while it still owns the current lock generation.
    /// A stale action must never cancel a newer action's lease.
    pub async fn abort_action(
        &self,
        owner: &str,
        ticket: &LeaseTicket,
    ) -> Result<(), &'static str> {
        if !ticket.action_ref {
            return Ok(());
        }
        let mut inner = self.inner.lock().await;
        let Some(existing) = inner.leases.get(&ticket.lease_id) else {
            return Ok(());
        };
        if existing.owner != owner {
            return Err("agent tab lease owner mismatch");
        }
        let tab_id = existing.tab_id;
        bump_snapshot_revision(&mut inner, tab_id);
        let Some(lease) = inner.leases.get_mut(&ticket.lease_id) else {
            return Ok(());
        };
        lease.action_refs = lease.action_refs.saturating_sub(1);
        if lease.action_refs == 0
            && !lease.persistent
            && matches!(lease.phase, Phase::Locking | Phase::Locked)
        {
            request_unlock(lease, lease.error);
        }
        self.changed.notify_waiters();
        Ok(())
    }

    /// Requests cleanup for a cancelled action or an explicitly stopped lease.
    pub async fn abort(&self, owner: &str, lease_id: &str) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        let Some(tab_id) = inner
            .leases
            .get(lease_id)
            .filter(|lease| lease.owner == owner)
            .map(|lease| lease.tab_id)
        else {
            if inner.leases.contains_key(lease_id) {
                return Err("agent tab lease owner mismatch");
            }
            return Ok(());
        };
        bump_snapshot_revision(&mut inner, tab_id);
        let Some(lease) = inner.leases.get_mut(lease_id) else {
            return Ok(());
        };
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
        self.wait_unlocked(owner, lease_id, cancel, cancel, cancel)
            .await
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
        let tab_id = {
            let Some(lease) = inner.leases.get_mut(lease_id) else {
                return Err("agent tab lease was not found");
            };
            if lease.owner != owner {
                return Err("agent tab lease owner mismatch");
            }
            if matches!(lease.phase, Phase::Locked | Phase::Locking) {
                lease.expires_at = Instant::now() + LEASE_TTL;
            }
            lease.tab_id
        };
        let revision = snapshot_revision(&inner, tab_id);
        let lease = inner
            .leases
            .get(lease_id)
            .expect("lease was validated while holding the service lock");
        Ok(view(lease, revision))
    }

    /// Returns every lease to the authenticated bundled extension so it can
    /// surface terminal failures and preserve Stop/Retry cleanup affordances.
    pub async fn leases_for_extension(&self) -> Vec<LeaseView> {
        self.sweep().await;
        let inner = self.inner.lock().await;
        inner
            .leases
            .values()
            .map(|lease| view(lease, snapshot_revision(&inner, lease.tab_id)))
            .collect()
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
            .map(|lease| view(lease, snapshot_revision(&inner, lease.tab_id)))
            .collect()
    }

    /// User stop/extension recovery requests cleanup for this exact lease.
    /// The bearer token authenticates the bundled extension; tuple matching prevents
    /// a stale tab id from releasing a newer incarnation.
    pub async fn register_action_dispatch(
        &self,
        owner: &str,
        ticket: &LeaseTicket,
        dispatch_id: &str,
        cancel: CancellationToken,
    ) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get(&ticket.lease_id) else {
            return Err("agent tab lease expired before action registration");
        };
        if lease.owner != owner {
            return Err("agent tab lease owner mismatch");
        }
        if lease.generation != ticket.generation
            || !matches!(lease.phase, Phase::Locking | Phase::Locked)
        {
            return Err("agent tab lease changed before action registration");
        }
        if inner
            .stopped_tabs
            .contains(&(lease.tab_id, lease.target_id.clone()))
        {
            cancel.cancel();
            return Err("agent control was stopped for this tab; the action was not dispatched");
        }
        if inner.draining_tabs.contains(&lease.tab_id) {
            cancel.cancel();
            return Err(
                "the page changed and native unlock is still being confirmed; the action was not dispatched",
            );
        }
        inner
            .active_dispatches
            .entry(ticket.lease_id.clone())
            .or_default()
            .insert(dispatch_id.to_string(), cancel);
        self.changed.notify_waiters();
        Ok(())
    }

    pub async fn unregister_action_dispatch(&self, lease_id: &str, dispatch_id: &str) {
        let mut inner = self.inner.lock().await;
        let remove_entry = inner
            .active_dispatches
            .get_mut(lease_id)
            .is_some_and(|active| {
                active.remove(dispatch_id);
                active.is_empty()
            });
        if remove_entry {
            inner.active_dispatches.remove(lease_id);
        }
        self.changed.notify_waiters();
    }

    /// Atomically unregisters and finishes one uniquely identified action.
    /// Repeated cleanup after cancellation is harmless and cannot decrement a
    /// sibling action reference.
    pub async fn finish_action_dispatch(
        &self,
        owner: &str,
        ticket: &LeaseTicket,
        dispatch_id: &str,
    ) -> bool {
        let mut inner = self.inner.lock().await;
        if inner
            .leases
            .get(&ticket.lease_id)
            .is_some_and(|lease| lease.owner != owner)
        {
            self.changed.notify_waiters();
            return false;
        }
        let (removed, remove_entry) = match inner.active_dispatches.get_mut(&ticket.lease_id) {
            Some(active) => {
                let removed = active.remove(dispatch_id).is_some();
                (removed, removed && active.is_empty())
            }
            None => (false, false),
        };
        if remove_entry {
            inner.active_dispatches.remove(&ticket.lease_id);
        }
        if !removed {
            self.changed.notify_waiters();
            return false;
        }
        let tab_id = inner.leases.get(&ticket.lease_id).map(|lease| lease.tab_id);
        let Some(tab_id) = tab_id else {
            self.changed.notify_waiters();
            return false;
        };
        let mut unlock_requested = false;
        if ticket.action_ref {
            bump_snapshot_revision(&mut inner, tab_id);
            if let Some(lease) = inner.leases.get_mut(&ticket.lease_id) {
                lease.action_refs = lease.action_refs.saturating_sub(1);
                if lease.action_refs == 0 && !lease.persistent {
                    request_unlock(lease, None);
                    unlock_requested = true;
                }
            }
        }
        self.changed.notify_waiters();
        unlock_requested
    }

    pub async fn finish_action_dispatch_and_wait_with_cancellation(
        &self,
        owner: &str,
        ticket: &LeaseTicket,
        dispatch_id: &str,
        cancel: &CancellationToken,
        client_cancel: &CancellationToken,
        dispatch_cancel: &CancellationToken,
    ) -> Result<(), &'static str> {
        if self
            .finish_action_dispatch(owner, ticket, dispatch_id)
            .await
        {
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

    /// Records explicit user Stop before a lease exists. The HTTP bridge has
    /// verified the target against current browser inventory; keep that exact
    /// incarnation so a reused numeric tab ID does not inherit the tombstone.
    pub async fn request_tab_stop(&self, tab_id: i64, target_id: &str) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        if inner
            .leases
            .values()
            .any(|lease| lease.tab_id == tab_id && lease.target_id != target_id)
        {
            return Err("tab Stop target does not match its active lease incarnation");
        }
        inner.stopped_tabs.insert((tab_id, target_id.to_string()));
        for lease in inner.leases.values().filter(|lease| lease.tab_id == tab_id) {
            if let Some(active) = inner.active_dispatches.get(&lease.id) {
                for cancel in active.values() {
                    cancel.cancel();
                }
            }
        }
        self.changed.notify_waiters();
        Ok(())
    }

    pub async fn request_release(
        &self,
        lease_id: &str,
        generation: u64,
        tab_id: i64,
        target_id: &str,
    ) -> Result<(), &'static str> {
        {
            let mut inner = self.inner.lock().await;
            let Some(lease) = inner.leases.get(lease_id) else {
                return Err("unknown or expired lease");
            };
            if lease.tab_id != tab_id || lease.target_id != target_id {
                return Err("lease release does not match the requested tab incarnation");
            }
            if lease.generation != generation {
                return Err("lease release generation is stale");
            }
            inner.stopped_tabs.insert((tab_id, target_id.to_string()));
            if let Some(active) = inner.active_dispatches.get(lease_id) {
                for cancel in active.values() {
                    cancel.cancel();
                }
            }
            self.changed.notify_waiters();
        }

        // Stop must not report success or release the native lock while an action
        // registered to this lease is still executing. A delayed registration sees
        // stopped_tabs and fails closed.
        let deadline = Instant::now() + ACK_TIMEOUT;
        loop {
            let notified = self.changed.notified();
            let still_active = self
                .inner
                .lock()
                .await
                .active_dispatches
                .get(lease_id)
                .is_some_and(|active| !active.is_empty());
            if !still_active {
                break;
            }
            tokio::select! {
                () = notified => {}
                () = tokio::time::sleep_until(deadline) => {
                    return Err("active page action did not stop before the timeout; native unlock was not requested");
                }
            }
        }

        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get(lease_id) else {
            return Ok(());
        };
        if lease.tab_id != tab_id || lease.target_id != target_id {
            return Err("lease changed while stopping the active page action");
        }
        if lease.generation != generation {
            return Err("lease generation changed while stopping the active page action");
        }
        bump_snapshot_revision(&mut inner, tab_id);
        let Some(lease) = inner.leases.get_mut(lease_id) else {
            return Ok(());
        };
        lease.persistent = false;
        lease.action_refs = 0;
        request_unlock(lease, lease.error);
        self.changed.notify_waiters();
        Ok(())
    }

    /// Stops dispatches for a page whose document identity changed while a native
    /// input lock was being acknowledged, then confirms native unlock before returning.
    pub async fn release_after_document_change(
        &self,
        owner: &str,
        lease_id: &str,
        tab_id: i64,
        target_id: &str,
    ) -> Result<(), &'static str> {
        {
            let mut inner = self.inner.lock().await;
            let Some(lease) = inner.leases.get(lease_id) else {
                return Ok(());
            };
            if lease.owner != owner {
                return Err("agent tab lease owner mismatch");
            }
            if lease.tab_id != tab_id || lease.target_id != target_id {
                return Err("page document changed on a different tab incarnation");
            }
            inner.draining_tabs.insert(tab_id);
            if let Some(active) = inner.active_dispatches.get(lease_id) {
                for cancel in active.values() {
                    cancel.cancel();
                }
            }
            self.changed.notify_waiters();
        }

        let deadline = Instant::now() + ACK_TIMEOUT;
        loop {
            let notified = self.changed.notified();
            let still_active = self
                .inner
                .lock()
                .await
                .active_dispatches
                .get(lease_id)
                .is_some_and(|active| !active.is_empty());
            if !still_active {
                break;
            }
            tokio::select! {
                () = notified => {}
                () = tokio::time::sleep_until(deadline) => {
                    return Err("concurrent page action did not stop; native unlock is not confirmed");
                }
            }
        }

        {
            let mut inner = self.inner.lock().await;
            let Some(lease) = inner.leases.get(lease_id) else {
                inner.draining_tabs.remove(&tab_id);
                return Ok(());
            };
            if lease.owner != owner || lease.tab_id != tab_id || lease.target_id != target_id {
                return Err("lease changed while stopping actions for the changed page");
            }
            bump_snapshot_revision(&mut inner, tab_id);
            let Some(lease) = inner.leases.get_mut(lease_id) else {
                inner.draining_tabs.remove(&tab_id);
                return Ok(());
            };
            lease.persistent = false;
            lease.action_refs = 0;
            request_unlock(
                lease,
                Some("page document changed while the action was waiting for its native lock"),
            );
            self.changed.notify_waiters();
        }

        let cancel = CancellationToken::new();
        self.wait_unlocked(owner, lease_id, &cancel, &cancel, &cancel)
            .await
    }

    pub async fn acknowledge(
        &self,
        lease_id: &str,
        generation: u64,
        tab_id: i64,
        target_id: &str,
        ack: LeaseAck,
    ) -> Result<(), &'static str> {
        let mut inner = self.inner.lock().await;
        let Some(lease) = inner.leases.get(lease_id) else {
            return Err("unknown or expired lease");
        };
        if lease.tab_id != tab_id || lease.target_id != target_id {
            return Err("lease acknowledgement does not match the requested tab incarnation");
        }
        if lease.generation != generation {
            return Err("lease acknowledgement generation is stale");
        }
        let removes_lease =
            matches!(lease.phase, Phase::Unlocking | Phase::Failed(_)) && ack == LeaseAck::Unlocked;
        if removes_lease {
            bump_snapshot_revision(&mut inner, tab_id);
            inner.leases.remove(lease_id);
            inner.draining_tabs.remove(&tab_id);
            self.changed.notify_waiters();
            return Ok(());
        }
        {
            let Some(lease) = inner.leases.get_mut(lease_id) else {
                return Err("unknown or expired lease");
            };
            match (&lease.phase, ack) {
                (Phase::Locking, LeaseAck::Locked) => {
                    lease.phase = Phase::Locked;
                    lease.error = None;
                    lease.cleanup_failures = 0;
                    lease.expires_at = Instant::now() + LEASE_TTL;
                }
                (Phase::Locking, LeaseAck::Unavailable) => {
                    request_unlock(
                        lease,
                        Some("native tab lock is unavailable; cleanup is required"),
                    );
                }
                (Phase::Locking, LeaseAck::Stale) => {
                    request_unlock(
                        lease,
                        Some("tab changed before native lock acknowledgement"),
                    );
                }
                // A lock may finish after expiry/cancel already requested unlock. Accept
                // only the exact tuple, keep the desired phase as Unlocking, and make the
                // extension's next lease-driven pass detach the just-acquired lock.
                (Phase::Unlocking, LeaseAck::Locked) | (Phase::Failed(_), LeaseAck::Locked) => {
                    lease.phase = Phase::Unlocking;
                    lease.error =
                        Some("late native lock acknowledgement; immediate cleanup is pending");
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
        }
        bump_snapshot_revision(&mut inner, tab_id);
        self.changed.notify_waiters();
        Ok(())
    }

    /// Session stop/disconnect requests native unlock for only that owner's leases,
    /// then waits a bounded interval for extension acknowledgement.
    pub async fn release_session(&self, owner: &str) {
        let ids = {
            let mut inner = self.inner.lock().await;
            let mut ids = Vec::new();
            let mut tab_ids = Vec::new();
            for lease in inner
                .leases
                .values_mut()
                .filter(|lease| lease.owner == owner)
            {
                lease.persistent = false;
                lease.action_refs = 0;
                request_unlock(lease, lease.error);
                ids.push(lease.id.clone());
                tab_ids.push(lease.tab_id);
            }
            tab_ids.sort_unstable();
            tab_ids.dedup();
            for tab_id in tab_ids {
                bump_snapshot_revision(&mut inner, tab_id);
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
        let mut tab_ids = Vec::new();
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
                Phase::Failed(_) => continue,
            }
            tab_ids.push(lease.tab_id);
            changed = true;
        }
        tab_ids.sort_unstable();
        tab_ids.dedup();
        for tab_id in tab_ids {
            bump_snapshot_revision(inner, tab_id);
        }
        changed
    }
}

fn bump_snapshot_revision(inner: &mut Inner, tab_id: i64) {
    let revision = inner.snapshot_revisions.entry(tab_id).or_default();
    *revision = revision.saturating_add(1);
}

fn snapshot_revision(inner: &Inner, tab_id: i64) -> u64 {
    inner.snapshot_revisions.get(&tab_id).copied().unwrap_or(0)
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

fn view(lease: &Lease, snapshot_revision: u64) -> LeaseView {
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
        generation: lease.generation,
        snapshot_revision,
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

    #[test]
    fn required_policy_does_not_degrade_to_feature_off_without_a_token() {
        let required = AgentControlService::new_with_policy(None, true);
        assert!(required.is_required());
        assert!(required.requires_control());
        assert!(!required.is_enabled());
        assert!(!required.authorized(Some("Bearer anything")));

        let disabled = AgentControlService::new_with_policy(None, false);
        assert!(!disabled.is_required());
        assert!(!disabled.requires_control());

        let enabled = AgentControlService::new_with_policy(Some("token".to_string()), false);
        assert!(!enabled.is_required());
        assert!(enabled.requires_control());
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
    async fn acknowledgement_must_match_exact_tab_and_target_before_locking() -> anyhow::Result<()>
    {
        let service = service();
        let ticket = service
            .begin("session-a", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .acknowledge(
                    &ticket.lease_id,
                    ticket.generation,
                    43,
                    "target-a",
                    LeaseAck::Locked,
                )
                .await
                .is_err()
        );
        assert!(
            service
                .acknowledge(
                    &ticket.lease_id,
                    ticket.generation,
                    42,
                    "target-b",
                    LeaseAck::Locked,
                )
                .await
                .is_err()
        );
        assert_eq!(service.pending_for_extension().await.len(), 1);
        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .wait_locked(
                "session-a",
                &ticket.lease_id,
                ticket.generation,
                &CancellationToken::new(),
            )
            .await
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    #[tokio::test]
    async fn another_session_cannot_acquire_or_release_an_owned_tab() -> anyhow::Result<()> {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("other", 1, 42, "target-a", true)
                .await
                .is_err()
        );
        assert!(
            service
                .end("other", &ticket.lease_id, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(service.pending_for_extension().await.len(), 1);
        assert_eq!(
            service
                .status("owner", &ticket.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locking"
        );
        Ok(())
    }

    #[tokio::test]
    async fn every_ephemeral_action_requires_a_fresh_lock_ack() -> anyhow::Result<()> {
        let service = service();
        let persistent = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &persistent.lease_id,
                persistent.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;

        let previous_action = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        let action = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(action.lease_id, persistent.lease_id);
        assert!(action.generation > previous_action.generation);
        assert_eq!(
            service
                .status("owner", &action.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locking"
        );
        assert_eq!(
            service.pending_for_extension().await[0].generation,
            action.generation
        );

        // A native poll may lock before a detach but deliver its ACK after a
        // later action has already requested a fresh generation.
        assert!(
            service
                .acknowledge(
                    &previous_action.lease_id,
                    previous_action.generation,
                    42,
                    "target-a",
                    LeaseAck::Locked,
                )
                .await
                .is_err()
        );
        assert!(
            service
                .wait_locked(
                    "owner",
                    &previous_action.lease_id,
                    previous_action.generation,
                    &CancellationToken::new(),
                )
                .await
                .is_err()
        );
        service
            .abort_action("owner", &previous_action)
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(
            service
                .status("owner", &action.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locking"
        );

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(
            service
                .wait_locked("owner", &action.lease_id, action.generation, &cancelled,)
                .await
                .is_err()
        );

        service
            .acknowledge(
                &action.lease_id,
                action.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .wait_locked(
                "owner",
                &action.lease_id,
                action.generation,
                &CancellationToken::new(),
            )
            .await
            .map_err(anyhow::Error::msg)?;
        service.finish_action("owner", &action).await;
        assert_eq!(
            service
                .status("owner", &action.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locked",
            "a persistent lease remains held after one acknowledged action"
        );
        Ok(())
    }
    #[tokio::test]
    async fn aborting_one_overlapping_action_preserves_the_other_reference() -> anyhow::Result<()> {
        let service = service();
        let action_a = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &action_a.lease_id,
                action_a.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;

        let action_b = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &action_b.lease_id,
                action_b.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .abort_action("owner", &action_b)
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(
            service
                .status("owner", &action_b.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locked",
            "aborting B must not drop A's live reference"
        );

        service.finish_action("owner", &action_a).await;
        assert_eq!(
            service
                .status("owner", &action_b.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "unlocking",
            "the last completed action releases the temporary lease"
        );
        service
            .acknowledge(
                &action_b.lease_id,
                action_b.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(service.status("owner", &action_b.lease_id).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn aborting_an_action_does_not_release_a_persistent_tab_lease() -> anyhow::Result<()> {
        let service = service();
        let persistent = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &persistent.lease_id,
                persistent.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;

        let action = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(!action.action_ref);
        service
            .abort_action("owner", &action)
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(
            service.pending_for_extension().await[0].status,
            "locking",
            "failed persistent actions leave the lock held while awaiting their new ACK"
        );
        service
            .acknowledge(
                &action.lease_id,
                action.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(
            service
                .status("owner", &action.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locked",
            "the user's persistent control remains active"
        );
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_revision_advances_across_lease_ids_on_one_tab() -> anyhow::Result<()> {
        let service = service();
        let old = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        let old_revision = service.leases_for_extension().await[0].snapshot_revision;
        service
            .request_release(&old.lease_id, old.generation, 42, "target-a")
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &old.lease_id,
                old.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;

        let new = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        let new_view = service.leases_for_extension().await.remove(0);
        assert_ne!(old.lease_id, new.lease_id);
        assert!(new_view.snapshot_revision > old_revision);
        assert_eq!(new_view.lease_id, new.lease_id);
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_revisions_are_isolated_per_tab() -> anyhow::Result<()> {
        let service = service();
        let tab_a = service
            .begin("owner-a", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        let revision_a_before = service
            .leases_for_extension()
            .await
            .into_iter()
            .find(|lease| lease.tab_id == 42)
            .ok_or_else(|| anyhow::anyhow!("tab A lease missing"))?
            .snapshot_revision;

        let tab_b = service
            .begin("owner-b", 2, 84, "target-b", true)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &tab_b.lease_id,
                tab_b.generation,
                84,
                "target-b",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        let views = service.leases_for_extension().await;
        let revision_a_after_tab_b_change = views
            .iter()
            .find(|lease| lease.tab_id == 42)
            .ok_or_else(|| anyhow::anyhow!("tab A lease missing after tab B change"))?
            .snapshot_revision;
        let revision_b = views
            .iter()
            .find(|lease| lease.tab_id == 84)
            .ok_or_else(|| anyhow::anyhow!("tab B lease missing"))?
            .snapshot_revision;
        assert_eq!(revision_a_after_tab_b_change, revision_a_before);
        assert!(revision_b > revision_a_before);

        service
            .acknowledge(
                &tab_a.lease_id,
                tab_a.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        let revision_a_after_same_tab_change = service
            .leases_for_extension()
            .await
            .into_iter()
            .find(|lease| lease.tab_id == 42)
            .ok_or_else(|| anyhow::anyhow!("tab A lease missing after its own change"))?
            .snapshot_revision;
        assert!(revision_a_after_same_tab_change > revision_a_before);
        Ok(())
    }

    #[tokio::test]
    async fn duplicate_locked_ack_does_not_advance_revision_without_mutation() -> anyhow::Result<()>
    {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        let revision_before = service
            .status("owner", &ticket.lease_id)
            .await
            .map_err(anyhow::Error::msg)?
            .snapshot_revision;

        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        let revision_after = service
            .status("owner", &ticket.lease_id)
            .await
            .map_err(anyhow::Error::msg)?
            .snapshot_revision;
        assert_eq!(revision_after, revision_before);
        Ok(())
    }

    #[tokio::test]
    async fn expired_failed_lease_does_not_advance_revision_without_mutation() {
        let service = service();
        let mut inner = service.inner.lock().await;
        let tab = 42;
        let lease = Lease {
            id: "failed".to_string(),
            owner: "owner".to_string(),
            page_id: 1,
            tab_id: tab,
            target_id: "target-a".to_string(),
            generation: 1,
            phase: Phase::Failed("cleanup failed"),
            expires_at: Instant::now() - Duration::from_secs(1),
            persistent: false,
            action_refs: 0,
            cleanup_failures: MAX_CLEANUP_FAILURES,
            error: Some("native unlock acknowledgement failed"),
        };
        inner.leases.insert(lease.id.clone(), lease);
        let revision_before = snapshot_revision(&inner, tab);
        assert!(!service.sweep_locked(&mut inner));
        assert_eq!(snapshot_revision(&inner, tab), revision_before);
    }

    #[tokio::test]
    async fn every_unresolved_phase_blocks_same_tab_reacquisition() -> anyhow::Result<()> {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_ok()
        );
        assert!(
            service
                .begin("other", 1, 42, "target-a", true)
                .await
                .is_err()
        );
        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .abort("owner", &ticket.lease_id)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_err()
        );
        for _ in 0..MAX_CLEANUP_FAILURES {
            service
                .acknowledge(
                    &ticket.lease_id,
                    ticket.generation,
                    42,
                    "target-a",
                    LeaseAck::Unavailable,
                )
                .await
                .unwrap_or_else(|_| panic!("unlock ACK should be accepted during cleanup"));
            // A failed cleanup can be retried only explicitly; this test advances it
            // through expiry via the private sweep path without sleeping.
            let status = service
                .status("owner", &ticket.lease_id)
                .await
                .map_err(anyhow::Error::msg)?;
            if status.status == "failed" {
                break;
            }
            {
                let mut inner = service.inner.lock().await;
                inner.leases.get_mut(&ticket.lease_id).unwrap().expires_at = Instant::now();
            }
            service.sweep().await;
        }
        assert_eq!(
            service
                .status("owner", &ticket.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "failed"
        );
        assert!(
            service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_err()
        );
        service
            .abort("owner", &ticket.lease_id)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_err()
        );
        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_ok()
        );
        Ok(())
    }

    #[tokio::test]
    async fn missing_extension_wakeup_times_out_before_any_lock_ack() -> anyhow::Result<()> {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;

        let error = service
            .wait_locked_for(
                "owner",
                &ticket.lease_id,
                ticket.generation,
                &CancellationToken::new(),
                Duration::from_millis(1),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            "Pip Dev could not wake the extension to lock this tab within 8 seconds, so no page input was sent. Keep a visible Pip Dev page or popup open, then retry."
        );
        assert_eq!(
            service
                .status("owner", &ticket.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locking"
        );
        service
            .abort("owner", &ticket.lease_id)
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(service.pending_for_extension().await[0].operation, "unlock");
        Ok(())
    }

    #[tokio::test]
    async fn cancellation_requests_exact_unlock_and_late_lock_never_wakes_waiter()
    -> anyhow::Result<()> {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .abort("owner", &ticket.lease_id)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .wait_locked(
                    "owner",
                    &ticket.lease_id,
                    ticket.generation,
                    &CancellationToken::new(),
                )
                .await
                .is_err()
        );
        let cleanup = service.pending_for_extension().await;
        assert_eq!(cleanup.len(), 1);
        assert_eq!(cleanup[0].operation, "unlock");
        assert_eq!(cleanup[0].tab_id, 42);
        assert_eq!(cleanup[0].target_id, "target-a");
        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(service.pending_for_extension().await.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn stale_and_unavailable_lock_results_transition_to_bounded_cleanup() -> anyhow::Result<()>
    {
        for result in [LeaseAck::Unavailable, LeaseAck::Stale] {
            let service = service();
            let ticket = service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .map_err(anyhow::Error::msg)?;
            service
                .acknowledge(&ticket.lease_id, ticket.generation, 42, "target-a", result)
                .await
                .map_err(anyhow::Error::msg)?;
            assert!(
                service
                    .wait_locked(
                        "owner",
                        &ticket.lease_id,
                        ticket.generation,
                        &CancellationToken::new(),
                    )
                    .await
                    .is_err()
            );
            assert_eq!(service.pending_for_extension().await[0].operation, "unlock");
            for retry in 0..MAX_CLEANUP_FAILURES {
                let _ = service
                    .acknowledge(
                        &ticket.lease_id,
                        ticket.generation,
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
            assert_eq!(
                service
                    .status("owner", &ticket.lease_id)
                    .await
                    .map_err(anyhow::Error::msg)?
                    .status,
                "failed"
            );
            assert!(service.pending_for_extension().await.is_empty());
        }
        Ok(())
    }

    #[tokio::test]
    async fn expired_lock_lease_requires_unlock_before_reacquisition() -> anyhow::Result<()> {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        {
            let mut inner = service.inner.lock().await;
            inner.leases.get_mut(&ticket.lease_id).unwrap().expires_at = Instant::now();
        }
        let pending = service.pending_for_extension().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].operation, "unlock");
        assert!(
            service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_err()
        );
        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_ok()
        );
        Ok(())
    }

    #[tokio::test]
    async fn session_stop_releases_only_its_leases_and_waits_for_ack() -> anyhow::Result<()> {
        let service = service();
        let owned = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        let other = service
            .begin("other", 2, 43, "target-b", true)
            .await
            .map_err(anyhow::Error::msg)?;
        let stopping = {
            let service = service.clone();
            tokio::spawn(async move { service.release_session("owner").await })
        };
        tokio::task::yield_now().await;
        let pending = service.pending_for_extension().await;
        assert_eq!(pending.len(), 2);
        assert!(
            pending
                .iter()
                .any(|lease| lease.lease_id == owned.lease_id && lease.operation == "unlock")
        );
        assert!(
            pending
                .iter()
                .any(|lease| lease.lease_id == other.lease_id && lease.operation == "lock")
        );
        service
            .acknowledge(
                &owned.lease_id,
                owned.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        stopping.await.map_err(anyhow::Error::msg)?;
        assert_eq!(
            service
                .status("other", &other.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locking"
        );
        Ok(())
    }

    #[tokio::test]
    async fn external_stop_release_requires_exact_tab_incarnation() -> anyhow::Result<()> {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .request_release(&ticket.lease_id, ticket.generation, 43, "target-a",)
                .await
                .is_err()
        );
        assert!(
            service
                .request_release(&ticket.lease_id, ticket.generation, 42, "target-b",)
                .await
                .is_err()
        );
        assert!(
            service
                .request_release(&ticket.lease_id, ticket.generation + 1, 42, "target-a",)
                .await
                .is_err()
        );
        assert_eq!(
            service
                .status("owner", &ticket.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locking"
        );
        service
            .request_release(&ticket.lease_id, ticket.generation, 42, "target-a")
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(
            service
                .status("owner", &ticket.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .operation,
            "unlock"
        );
        Ok(())
    }

    #[tokio::test]
    async fn finishing_registered_dispatch_is_idempotent_for_sibling_actions() -> anyhow::Result<()>
    {
        let service = service();
        let action_a = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &action_a.lease_id,
                action_a.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .register_action_dispatch("owner", &action_a, "dispatch-a", CancellationToken::new())
            .await
            .map_err(anyhow::Error::msg)?;

        // A second action advances the shared lease generation while A is still
        // registered. A must still release only its own action reference.
        let action_b = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(action_a.lease_id, action_b.lease_id);
        service
            .register_action_dispatch("owner", &action_b, "dispatch-b", CancellationToken::new())
            .await
            .map_err(anyhow::Error::msg)?;

        assert!(
            !service
                .finish_action_dispatch("owner", &action_a, "dispatch-a")
                .await
        );
        assert!(
            !service
                .finish_action_dispatch("owner", &action_a, "dispatch-a")
                .await,
            "retrying A cleanup must not decrement B's action reference"
        );
        assert_eq!(
            service
                .inner
                .lock()
                .await
                .leases
                .get(&action_a.lease_id)
                .map(|lease| lease.action_refs),
            Some(1)
        );

        service
            .acknowledge(
                &action_b.lease_id,
                action_b.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .finish_action_dispatch("owner", &action_b, "dispatch-b")
                .await
        );
        assert_eq!(
            service
                .status("owner", &action_b.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "unlocking"
        );
        service
            .acknowledge(
                &action_b.lease_id,
                action_b.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    #[tokio::test]
    async fn tab_stop_cancels_then_preserves_the_existing_drain_barrier() -> anyhow::Result<()> {
        use std::time::Duration;

        let service = service();
        let action = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        let cancel = CancellationToken::new();
        service
            .register_action_dispatch("owner", &action, "dispatch", cancel.clone())
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &action.lease_id,
                action.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .request_tab_stop(42, "target-a")
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(cancel.is_cancelled());
        assert_eq!(
            service
                .status("owner", &action.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locked"
        );
        let releasing = service.clone();
        let ticket = action.clone();
        let mut stop = tokio::spawn(async move {
            releasing
                .request_release(&ticket.lease_id, ticket.generation, 42, "target-a")
                .await
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut stop)
                .await
                .is_err()
        );
        service
            .finish_action_dispatch("owner", &action, "dispatch")
            .await;
        stop.await
            .map_err(anyhow::Error::msg)?
            .map_err(anyhow::Error::msg)?;
        assert_eq!(
            service
                .status("owner", &action.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "unlocking"
        );
        service
            .acknowledge(
                &action.lease_id,
                action.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(service.leases_for_extension().await.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn stop_before_first_lease_blocks_only_the_stopped_target() -> anyhow::Result<()> {
        let service = service();
        service
            .request_tab_stop(42, "target-a")
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("late-owner", 1, 42, "target-a", false)
                .await
                .is_err()
        );
        let replacement = service
            .begin("new-owner", 2, 42, "target-b", false)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(service.request_tab_stop(42, "target-a").await.is_err());
        assert_eq!(
            service
                .status("new-owner", &replacement.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .target_id,
            "target-b"
        );
        service
            .abort("new-owner", &replacement.lease_id)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &replacement.lease_id,
                replacement.generation,
                42,
                "target-b",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("old-owner", 1, 42, "target-a", false)
                .await
                .is_err(),
            "a different-target begin must never erase the old Stop intent"
        );
        Ok(())
    }

    #[tokio::test]
    async fn stopping_cancels_and_drains_in_flight_action_before_unlock() -> anyhow::Result<()> {
        let service = service();
        let persistent = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &persistent.lease_id,
                persistent.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        let action = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &action.lease_id,
                action.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        let dispatch_cancel = CancellationToken::new();
        service
            .register_action_dispatch("owner", &action, "dispatch-1", dispatch_cancel.clone())
            .await
            .map_err(anyhow::Error::msg)?;

        let release_service = service.clone();
        let release_ticket = action.clone();
        let mut release = tokio::spawn(async move {
            release_service
                .request_release(
                    &release_ticket.lease_id,
                    release_ticket.generation,
                    42,
                    "target-a",
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), dispatch_cancel.cancelled())
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut release)
                .await
                .is_err(),
            "Stop must wait for the registered action to leave execution"
        );
        assert!(
            service
                .begin("owner", 1, 42, "target-a", false)
                .await
                .is_err(),
            "new actions must be rejected while Stop is draining the active action"
        );
        assert_eq!(
            service
                .status("owner", &action.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locked",
            "the native lock must remain held until the action exits"
        );

        service
            .unregister_action_dispatch(&action.lease_id, "dispatch-1")
            .await;
        release
            .await
            .map_err(anyhow::Error::msg)?
            .map_err(anyhow::Error::msg)?;
        assert_eq!(
            service
                .status("owner", &action.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "unlocking"
        );
        service
            .acknowledge(
                &action.lease_id,
                action.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(
            service
                .begin("owner", 1, 42, "target-a", false)
                .await
                .is_err(),
            "Stop must leave an action tombstone after the lease is removed"
        );
        assert!(
            service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_err(),
            "MCP persistent begin must not clear the user Stop tombstone"
        );
        assert!(
            service
                .begin("another-owner", 1, 42, "target-a", true)
                .await
                .is_err(),
            "Stop must block reacquisition by every agent owner"
        );
        assert!(
            service
                .begin("another-owner", 2, 84, "target-b", false)
                .await
                .is_ok(),
            "Stop must not disable agent control on another tab"
        );
        let restarted_service = AgentControlService::new(Some("test-token".to_string()));
        assert!(
            restarted_service
                .begin("owner", 1, 42, "target-a", true)
                .await
                .is_ok(),
            "restarting Pip Dev is the documented recovery path"
        );
        Ok(())
    }

    #[tokio::test]
    async fn wrong_owner_tab_or_target_never_changes_lease() -> anyhow::Result<()> {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        assert!(service.status("other", &ticket.lease_id).await.is_err());
        assert!(service.abort("other", &ticket.lease_id).await.is_err());
        assert!(
            service
                .acknowledge(
                    &ticket.lease_id,
                    ticket.generation,
                    99,
                    "target-a",
                    LeaseAck::Locked,
                )
                .await
                .is_err()
        );
        assert!(
            service
                .acknowledge(
                    &ticket.lease_id,
                    ticket.generation,
                    42,
                    "target-z",
                    LeaseAck::Locked,
                )
                .await
                .is_err()
        );
        let status = service
            .status("owner", &ticket.lease_id)
            .await
            .map_err(anyhow::Error::msg)?;
        assert_eq!(status.status, "locking");
        assert_eq!(status.target_id, "target-a");
        Ok(())
    }

    #[tokio::test]
    async fn ephemeral_action_waits_for_native_unlock_ack() -> anyhow::Result<()> {
        let service = service();
        let ticket = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;

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
            .acknowledge(
                &ticket.lease_id,
                ticket.generation,
                42,
                "target-a",
                LeaseAck::Unlocked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        finishing
            .await
            .map_err(anyhow::Error::msg)?
            .map_err(anyhow::Error::msg)?;
        assert!(service.status("owner", &ticket.lease_id).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn unlock_wait_honors_client_and_operator_cancellation() -> anyhow::Result<()> {
        for cancel_client in [true, false] {
            let service = service();
            let ticket = service
                .begin("owner", 1, 42, "target-a", false)
                .await
                .map_err(anyhow::Error::msg)?;
            service
                .acknowledge(
                    &ticket.lease_id,
                    ticket.generation,
                    42,
                    "target-a",
                    LeaseAck::Locked,
                )
                .await
                .map_err(anyhow::Error::msg)?;
            service.finish_action("owner", &ticket).await;

            let session_cancel = CancellationToken::new();
            let client_cancel = CancellationToken::new();
            let dispatch_cancel = CancellationToken::new();
            let finishing = {
                let service = service.clone();
                let lease_id = ticket.lease_id.clone();
                let session_cancel = session_cancel.clone();
                let client_cancel = client_cancel.clone();
                let dispatch_cancel = dispatch_cancel.clone();
                tokio::spawn(async move {
                    service
                        .wait_unlocked(
                            "owner",
                            &lease_id,
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
            let error = finishing.await.map_err(anyhow::Error::msg)?.unwrap_err();
            assert!(
                error.contains("cancelled by the client")
                    || error.contains("cancelled by the operator")
            );
            assert_eq!(service.pending_for_extension().await[0].operation, "unlock");
        }
        Ok(())
    }

    #[tokio::test]
    async fn action_references_unlock_after_last_action_but_not_while_persistent()
    -> anyhow::Result<()> {
        let service = service();
        let persistent = service
            .begin("owner", 1, 42, "target-a", true)
            .await
            .map_err(anyhow::Error::msg)?;
        let action_a = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        let action_b = service
            .begin("owner", 1, 42, "target-a", false)
            .await
            .map_err(anyhow::Error::msg)?;
        service
            .acknowledge(
                &persistent.lease_id,
                action_b.generation,
                42,
                "target-a",
                LeaseAck::Locked,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        service.finish_action("owner", &action_a).await;
        service.finish_action("owner", &action_b).await;
        assert_eq!(
            service
                .status("owner", &persistent.lease_id)
                .await
                .map_err(anyhow::Error::msg)?
                .status,
            "locked"
        );
        Ok(())
    }
}
