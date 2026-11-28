use moli_webidl_callback::WebIdlCallbackFunction;

use crate::{
    context_bootstrap::{
        LegacyStorageQuotaCallbackOutcome, LegacyStorageQuotaCallbackTask,
        LegacyStorageQuotaCallbackTaskEffect,
    },
    page_task_queue::{
        PageMiscPlatformApiTargetEffect, RendererPageMiscPlatformApiTaskId,
        RendererPageMiscPlatformApiTaskKind,
    },
};

use super::window_document_tasks::{ExactWindowDocumentTaskLedger, PendingExactWindowDocumentTask};
use super::{OwnerDispatchScope, PageScriptHost, WindowDocumentTaskTarget};

pub(crate) enum MiscPlatformApiTaskPayload {
    LegacyStorageQuota(LegacyStorageQuotaCallbackTask),
    PendingPromiseRejectionFlush,
}

pub(super) type MiscPlatformApiTaskState = ExactWindowDocumentTaskLedger<
    RendererPageMiscPlatformApiTaskId,
    RendererPageMiscPlatformApiTaskKind,
    MiscPlatformApiTaskPayload,
>;

impl PageScriptHost {
    /// Publish one deprecated-quota callback to the exact calling
    /// Window/Document's miscellaneous-platform task source.
    pub(crate) fn queue_legacy_storage_quota_callback_task(
        &mut self,
        scope: &mut v8::PinScope<'_, '_>,
        callback: WebIdlCallbackFunction,
        outcome: LegacyStorageQuotaCallbackOutcome,
    ) -> bool {
        let Some(target) = self.current_window_document_task_target(scope) else {
            return false;
        };
        let task_id = self
            .misc_platform_api_tasks
            .allocate_task_id(RendererPageMiscPlatformApiTaskId::from_raw);
        let kind = outcome.kind();
        let task = LegacyStorageQuotaCallbackTask::new(scope, self, callback, outcome);
        self.misc_platform_api_tasks
            .push(PendingExactWindowDocumentTask::new(
                task_id,
                target,
                kind,
                MiscPlatformApiTaskPayload::LegacyStorageQuota(task),
            ));

        if self
            .page_misc_platform_api_sender()
            .send(target, task_id, kind)
            .is_ok()
        {
            return true;
        }

        let _ = self
            .misc_platform_api_tasks
            .remove_exact(task_id, target, kind);
        tracing::debug!(
            ?target,
            ?task_id,
            ?kind,
            "retired deprecated storage quota callback after MiscPlatformApi route closure"
        );
        false
    }

    /// Schedule one coalesced follow-up turn on the Page's root Document when
    /// `unhandledrejection` dispatch leaves newly rejected promises queued for
    /// reporting on an otherwise idle Page.
    pub(crate) fn queue_pending_promise_rejection_flush_task(&mut self) -> bool {
        if self.page_task_capabilities.get().is_none() {
            return false;
        }
        let Some(target) =
            self.current_window_document_task_target_for_dispatch_scope(OwnerDispatchScope::Top)
        else {
            return false;
        };
        let kind = RendererPageMiscPlatformApiTaskKind::PendingPromiseRejectionFlush;
        if self
            .misc_platform_api_tasks
            .find_slot_index(target, kind, |_| true)
            .is_some()
        {
            return true;
        }
        let Some(sender) = self
            .page_task_capabilities
            .get()
            .map(|capabilities| capabilities.misc_platform_api().clone())
        else {
            return false;
        };
        let task_id = self
            .misc_platform_api_tasks
            .allocate_task_id(RendererPageMiscPlatformApiTaskId::from_raw);
        self.misc_platform_api_tasks
            .push(PendingExactWindowDocumentTask::new(
                task_id,
                target,
                kind,
                MiscPlatformApiTaskPayload::PendingPromiseRejectionFlush,
            ));

        if sender.send(target, task_id, kind).is_ok() {
            return true;
        }

        let _ = self
            .misc_platform_api_tasks
            .remove_exact(task_id, target, kind);
        false
    }

    pub(crate) fn current_pending_misc_platform_api_task(
        &self,
        task_id: RendererPageMiscPlatformApiTaskId,
    ) -> Option<(
        WindowDocumentTaskTarget,
        RendererPageMiscPlatformApiTaskKind,
    )> {
        let pending = self.misc_platform_api_tasks.pending(task_id)?;
        let current_target = self.current_window_document_task_target_for_dispatch_scope(
            pending.target().dispatch_scope(),
        )?;
        Some((current_target, pending.kind()))
    }

    pub(crate) fn take_pending_misc_platform_api_task_for_exact_target(
        &mut self,
        task_id: RendererPageMiscPlatformApiTaskId,
        target: WindowDocumentTaskTarget,
        kind: RendererPageMiscPlatformApiTaskKind,
    ) -> Option<MiscPlatformApiTaskPayload> {
        self.misc_platform_api_tasks
            .remove_exact(task_id, target, kind)
            .map(PendingExactWindowDocumentTask::into_payload)
    }

    pub(crate) fn discard_pending_misc_platform_api_task(
        &mut self,
        task_id: RendererPageMiscPlatformApiTaskId,
    ) -> bool {
        self.misc_platform_api_tasks.remove(task_id).is_some()
    }

    pub(crate) fn dispatch_authorized_misc_platform_api_task(
        scope: &mut v8::PinScope<'_, '_>,
        host_ptr: *mut PageScriptHost,
        target: WindowDocumentTaskTarget,
        task: MiscPlatformApiTaskPayload,
    ) -> PageMiscPlatformApiTargetEffect {
        // SAFETY: `host_ptr` points to the live `PageScriptHost` for this
        // single-threaded V8 context, and the temporary mutable borrow ends
        // before any callback or microtask checkpoint executes.
        let Some(resolved) = unsafe { &mut *host_ptr }
            .resolve_authorized_window_document_task_context(scope, target)
        else {
            return PageMiscPlatformApiTargetEffect::CurrentOwnerCallbackRetired;
        };
        let scope = &mut v8::ContextScope::new(scope, resolved.context);
        match task {
            MiscPlatformApiTaskPayload::LegacyStorageQuota(task) => {
                let dispatch_scope = target.dispatch_scope();
                let previous_scope = dispatch_scope.enter(scope);
                let effect = match task.invoke(scope, host_ptr) {
                    LegacyStorageQuotaCallbackTaskEffect::CallbackInvoked => {
                        PageMiscPlatformApiTargetEffect::CallbackInvokedForCurrentOwner
                    }
                    LegacyStorageQuotaCallbackTaskEffect::CallbackNotInvoked => {
                        PageMiscPlatformApiTargetEffect::CurrentOwnerCallbackRetired
                    }
                };
                dispatch_scope.restore(scope, previous_scope);
                effect
            }
            MiscPlatformApiTaskPayload::PendingPromiseRejectionFlush => {
                // The selected-task dispatcher owns the ordinary task-end
                // microtask checkpoint, which flushes pending promise
                // rejections for the isolate. If an earlier turn already
                // drained them, retire quietly to a checkpoint-only completion.
                if crate::script_vm::has_pending_agent_promise_rejections(scope) {
                    PageMiscPlatformApiTargetEffect::CallbackInvokedForCurrentOwner
                } else {
                    PageMiscPlatformApiTargetEffect::CurrentOwnerCallbackRetired
                }
            }
        }
    }
}
