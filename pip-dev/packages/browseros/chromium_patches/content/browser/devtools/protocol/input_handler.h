diff --git a/content/browser/devtools/protocol/input_handler.h b/content/browser/devtools/protocol/input_handler.h
index 4c25a8c1c2..359a667803 100644
--- a/content/browser/devtools/protocol/input_handler.h
+++ b/content/browser/devtools/protocol/input_handler.h
@@ -12,6 +12,7 @@
 
 #include "base/containers/flat_map.h"
 #include "base/containers/flat_set.h"
+#include "base/functional/callback_helpers.h"
 #include "base/containers/unique_ptr_adapters.h"
 #include "base/memory/raw_ptr.h"
 #include "base/memory/raw_ref.h"
@@ -154,6 +155,7 @@ class InputHandler : public DevToolsDomainHandler, public Input::Backend {
                                       std::optional<int> click_count) override;
 
   Response SetIgnoreInputEvents(bool ignore) override;
+  Response SetAgentInputLock(bool locked) override;
   Response SetInterceptDrags(bool enabled) override;
 
   void SynthesizePinchGesture(
@@ -356,6 +358,8 @@ class InputHandler : public DevToolsDomainHandler, public Input::Backend {
       injectors_;
   int last_id_ = 0;
   bool ignore_input_events_ = false;
+  bool agent_input_lock_requested_ = false;
+  base::ScopedClosureRunner agent_input_lock_;
   std::optional<content::WebContents::ScopedIgnoreInputEvents>
       scoped_ignore_input_events_;
   bool intercept_drags_ = false;
