diff --git a/content/browser/web_contents/web_contents_impl.h b/content/browser/web_contents/web_contents_impl.h
index c2c99afba1..af123fab1b 100644
--- a/content/browser/web_contents/web_contents_impl.h
+++ b/content/browser/web_contents/web_contents_impl.h
@@ -676,6 +676,8 @@ class CONTENT_EXPORT WebContentsImpl
   [[nodiscard]] ScopedIgnoreInputEvents IgnoreInputEvents(
       std::optional<WebInputEventAuditCallback> audit_callback) override;
   bool ShouldIgnoreInputEventsForTesting() override;
+  [[nodiscard]] base::ScopedClosureRunner AcquireNativeInputLock() override;
+  bool IsNativeInputLocked() const override;
   bool HasActiveEffectivelyFullscreenVideo() override;
   void WriteIntoTrace(perfetto::TracedValue context) override;
   const base::Location& GetCreatorLocation() override;
@@ -2540,6 +2542,7 @@ class CONTENT_EXPORT WebContentsImpl
   // Counts the number of outstanding requests to ignore input events. They will
   // not be sent when this is greater than zero.
   int ignore_input_events_count_ = 0;
+  int native_input_lock_count_ = 0;
   uint64_t next_web_input_event_audit_callback_id_ = 0;
   base::flat_map<uint64_t, WebInputEventAuditCallback>
       web_input_event_audit_callbacks_;
