diff --git a/content/browser/web_contents/web_contents_impl.cc b/content/browser/web_contents/web_contents_impl.cc
index 6d6d0947fa..1cfc7a74ed 100644
--- a/content/browser/web_contents/web_contents_impl.cc
+++ b/content/browser/web_contents/web_contents_impl.cc
@@ -3187,6 +3187,23 @@ WebContents::ScopedIgnoreInputEvents WebContentsImpl::IgnoreInputEvents(
                                  : std::nullopt));
 }
 
+base::ScopedClosureRunner WebContentsImpl::AcquireNativeInputLock() {
+  ++native_input_lock_count_;
+  return base::ScopedClosureRunner(base::BindOnce(
+      [](base::WeakPtr<WebContentsImpl> web_contents) {
+        if (!web_contents) {
+          return;
+        }
+        CHECK_GT(web_contents->native_input_lock_count_, 0);
+        --web_contents->native_input_lock_count_;
+      },
+      weak_factory_.GetWeakPtr()));
+}
+
+bool WebContentsImpl::IsNativeInputLocked() const {
+  return native_input_lock_count_ > 0;
+}
+
 bool WebContentsImpl::ShouldIgnoreInputEventsForTesting() {
   return ShouldIgnoreInputEvents();
 }
