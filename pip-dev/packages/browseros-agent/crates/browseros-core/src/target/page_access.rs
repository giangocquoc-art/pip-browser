//! Reusable page capabilities retain both the call's scope and the page's target incarnation.
//! Observer state remains shared; only acquisition and protocol dispatch carry these guards.
use super::{BrowserView, CapturedScope, NativeWindow, cdp_policy, contains_frame};
use crate::{
    CoreError, FrameId, PageAdmission, ProtocolSession, SessionId, TargetError, TargetId,
    connection::{EpochGuard, ProtocolAdmission},
    pages::{PageInfo, PageSession},
};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct PageAccess {
    view: BrowserView,
    page: crate::PageId,
    target: TargetId,
    epoch_guard: Option<EpochGuard>,
}

/// Narrow authority to lock input on one page incarnation. It is issued only after fresh
/// target and connection-epoch validation; it cannot be repointed at another page afterward.
#[derive(Clone)]
pub struct AgentInputLockCapability {
    pub(super) access: Arc<PageAccess>,
}

impl AgentInputLockCapability {
    #[must_use]
    pub fn page_id(&self) -> &crate::PageId {
        &self.access.page
    }

    #[must_use]
    pub fn target_id(&self) -> &TargetId {
        &self.access.target
    }

    #[must_use]
    pub fn connection_epoch(&self) -> u64 {
        self.access
            .epoch_guard
            .as_ref()
            .expect("lock capabilities always retain an epoch guard")
            .expected
    }

    /// Apply or release the native page input lock and wait for Chromium's CDP acknowledgment.
    pub async fn set_locked(&self, locked: bool) -> Result<(), CoreError> {
        self.access.set_agent_input_lock(locked).await
    }
}

impl PageAccess {
    pub(super) fn new(view: BrowserView, page: PageInfo, epoch_guard: Option<EpochGuard>) -> Self {
        Self {
            view,
            page: page.page_id,
            target: page.target_id,
            epoch_guard,
        }
    }
    pub(crate) async fn fresh(&self) -> Result<PageInfo, CoreError> {
        if let Some(guard) = &self.epoch_guard {
            guard.check(self.view.session.connection().as_ref())?;
        }
        let fresh = self.view.page(self.page.clone()).await?;
        if fresh.target_id != self.target {
            return Err(TargetError::PageTargetChanged(self.page.clone()).into());
        }
        if let Some(guard) = &self.epoch_guard {
            guard.check(self.view.session.connection().as_ref())?;
        }
        Ok(fresh)
    }
    pub(crate) async fn session(self: &Arc<Self>) -> Result<PageSession, CoreError> {
        let live = self.view.validate_live().await?;
        let admission = Admission {
            access: self,
            scope: live.as_ref().map(|live| &live.scope),
            windows: live.as_ref().map_or(&[], |live| live.windows.as_slice()),
        };
        let mut session = self
            .view
            .session
            .pages
            .get_session_admitted_with_epoch(
                self.page.clone(),
                &admission,
                self.epoch_guard.clone(),
            )
            .await?;
        self.fresh().await?;
        session.session = session.session.with_admission(Arc::new(PageProtocol {
            access: self.clone(),
            root_session: session.session_id.clone(),
            frame: None,
        }));
        Ok(session)
    }
    pub(crate) async fn frame_session(
        self: &Arc<Self>,
        session: ProtocolSession,
        frame: Option<FrameId>,
    ) -> Result<ProtocolSession, CoreError> {
        let root = self.session().await?;
        if session.same_session(&root.session) {
            return Ok(root.session);
        }
        let frame_id = frame.ok_or_else(|| cdp_policy::unsupported("frame_session"))?;
        let endpoint = session
            .session_id()
            .cloned()
            .ok_or_else(|| cdp_policy::unsupported("frame_session"))?;
        // OOPIFs are internal descendants, never independent page capabilities. Bind the child
        // endpoint to a frame still present in this page's tree and its current registry session.
        Ok(session
            .with_epoch_guard(self.epoch_guard.clone())
            .with_admission(Arc::new(PageProtocol {
                access: self.clone(),
                root_session: root.session_id,
                frame: Some((frame_id, endpoint)),
            })))
    }

    pub(crate) fn epoch_guard(&self) -> Result<Option<crate::connection::EpochGuard>, CoreError> {
        Ok(self.epoch_guard.clone())
    }

    /// Set Chromium's native input lock through this page's attached CDP session.
    /// This intentionally bypasses the generic page-method allowlist: only the narrow
    /// capability created from a fresh page/epoch can issue this native operation.
    pub(crate) async fn set_agent_input_lock(
        self: &Arc<Self>,
        locked: bool,
    ) -> Result<(), CoreError> {
        let guard = self
            .epoch_guard
            .clone()
            .ok_or_else(|| TargetError::CdpUnsupported {
                method: "Input.setAgentInputLock".into(),
            })?;
        let page_session = if locked {
            self.session().await?
        } else {
            // Releasing a lock only reduces control over this exact page. Permit cleanup after
            // the page leaves its captured profile/window scope, while retaining incarnation
            // and epoch admission so this cannot affect a replacement target.
            self.cleanup_session().await?
        };
        let endpoint =
            ProtocolSession::for_session(self.view.session.connection(), page_session.session_id)
                .with_epoch_guard(Some(guard));
        // The command's response is its acknowledgment. Revalidate afterward so callers never
        // retain a successful result for a page or scope that changed during dispatch.
        endpoint
            .send_value(
                "Input.setAgentInputLock",
                serde_json::json!({ "locked": locked }),
            )
            .await?;
        if locked {
            self.fresh().await?;
        } else {
            self.fresh_incarnation().await?;
        }
        Ok(())
    }

    async fn cleanup_session(self: &Arc<Self>) -> Result<PageSession, CoreError> {
        self.fresh_incarnation().await?;
        let admission = IncarnationAdmission { access: self };
        self.view
            .session
            .pages
            .get_session_admitted_with_epoch(
                self.page.clone(),
                &admission,
                self.epoch_guard.clone(),
            )
            .await
    }

    async fn fresh_incarnation(&self) -> Result<PageInfo, CoreError> {
        let guard = self
            .epoch_guard
            .as_ref()
            .ok_or_else(|| TargetError::CdpUnsupported {
                method: "Input.setAgentInputLock".into(),
            })?;
        guard.check(self.view.session.connection().as_ref())?;
        let fresh = self
            .view
            .session
            .pages
            .refresh_with_epoch(self.page.clone(), Some(guard.clone()))
            .await?
            .ok_or_else(|| TargetError::PageTargetChanged(self.page.clone()))?;
        if fresh.target_id != self.target {
            return Err(TargetError::PageTargetChanged(self.page.clone()).into());
        }
        guard.check(self.view.session.connection().as_ref())?;
        Ok(fresh)
    }
}
struct Admission<'a> {
    access: &'a PageAccess,
    scope: Option<&'a CapturedScope>,
    windows: &'a [NativeWindow],
}
impl PageAdmission for Admission<'_> {
    fn admit(&self, fresh: &PageInfo) -> Result<(), CoreError> {
        if let Some(scope) = self.scope {
            self.access.view.check_epoch(scope)?;
            scope.admit_page(fresh, self.windows)?;
        }
        if let Some(guard) = &self.access.epoch_guard {
            guard.check(self.access.view.session.connection().as_ref())?;
        }
        if fresh.target_id != self.access.target {
            return Err(TargetError::PageTargetChanged(fresh.page_id.clone()).into());
        }
        Ok(())
    }
}

struct IncarnationAdmission<'a> {
    access: &'a PageAccess,
}

impl PageAdmission for IncarnationAdmission<'_> {
    fn admit(&self, fresh: &PageInfo) -> Result<(), CoreError> {
        if let Some(guard) = &self.access.epoch_guard {
            guard.check(self.access.view.session.connection().as_ref())?;
        }
        if fresh.page_id != self.access.page || fresh.target_id != self.access.target {
            return Err(TargetError::PageTargetChanged(self.access.page.clone()).into());
        }
        Ok(())
    }
}

struct PageProtocol {
    access: Arc<PageAccess>,
    root_session: SessionId,
    frame: Option<(FrameId, SessionId)>,
}
impl ProtocolAdmission for PageProtocol {
    fn check<'a>(
        &'a self,
        method: &'a str,
        params: &'a Value,
    ) -> BoxFuture<'a, Result<(), CoreError>> {
        Box::pin(async move {
            let fresh = self.access.fresh().await?;
            cdp_policy::page_method(method, params)?;
            let owner = self
                .access
                .view
                .session
                .pages
                .page_for_session(&self.root_session)
                .await;
            if owner.is_none_or(|owner| {
                owner.page_id != fresh.page_id || owner.target_id != self.access.target
            }) {
                return Err(TargetError::PageTargetChanged(fresh.page_id).into());
            }
            if let Some((frame, endpoint)) = &self.frame {
                let root = ProtocolSession::for_session(
                    self.access.view.session.connection(),
                    self.root_session.clone(),
                )
                .with_epoch_guard(self.access.epoch_guard.clone());
                let tree = root.send_value("Page.getFrameTree", json!({})).await?;
                if !contains_frame(&tree["frameTree"], frame.as_str())
                    || !self
                        .access
                        .view
                        .session
                        .frames()
                        .frame_session_matches(frame, endpoint)
                        .await
                {
                    return Err(TargetError::PageTargetChanged(fresh.page_id).into());
                }
                self.access.fresh().await?;
            }
            Ok(())
        })
    }
}
