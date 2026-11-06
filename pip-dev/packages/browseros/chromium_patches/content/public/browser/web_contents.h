diff --git a/content/public/browser/web_contents.h b/content/public/browser/web_contents.h
index 286c61111d..e83861bcf1 100644
--- a/content/public/browser/web_contents.h
+++ b/content/public/browser/web_contents.h
@@ -1704,6 +1704,14 @@ class WebContents : public PageNavigator, public base::SupportsUserData {
       std::optional<WebInputEventAuditCallback> audit_callback) = 0;
   virtual bool ShouldIgnoreInputEventsForTesting() = 0;
 
+  // Acquires a scoped lock that suppresses native user input delivered through
+  // the browser UI event path while leaving DevTools input dispatch available.
+  // The returned runner releases the lock when destroyed.
+  [[nodiscard]] virtual base::ScopedClosureRunner AcquireNativeInputLock() {
+    return {};
+  }
+  virtual bool IsNativeInputLocked() const { return false; }
+
   // Returns the group id for all audio streams that correspond to a single
   // WebContents. This can be used to determine if a AudioOutputStream was
   // created from a renderer that originated from this WebContents.
