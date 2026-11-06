diff --git a/content/browser/devtools/protocol/input_handler.cc b/content/browser/devtools/protocol/input_handler.cc
index 411d7835ac..31f3da426c 100644
--- a/content/browser/devtools/protocol/input_handler.cc
+++ b/content/browser/devtools/protocol/input_handler.cc
@@ -1167,6 +1167,12 @@ void InputHandler::SetRenderer(int process_host_id,
       scoped_ignore_input_events_.reset();
     }
   }
+  if (agent_input_lock_requested_ && old_web_contents != web_contents_) {
+    agent_input_lock_ = base::ScopedClosureRunner();
+    if (web_contents_) {
+      agent_input_lock_ = web_contents_->AcquireNativeInputLock();
+    }
+  }
 }
 
 void InputHandler::Wire(UberDispatcher* dispatcher) {
@@ -1178,6 +1184,8 @@ Response InputHandler::Disable() {
   ClearInputState();
   scoped_ignore_input_events_.reset();
   ignore_input_events_ = false;
+  agent_input_lock_requested_ = false;
+  agent_input_lock_ = base::ScopedClosureRunner();
   pointer_ids_.clear();
   touch_points_.clear();
   return Response::Success();
@@ -2090,6 +2098,23 @@ Response InputHandler::SetIgnoreInputEvents(bool ignore) {
   return Response::Success();
 }
 
+Response InputHandler::SetAgentInputLock(bool locked) {
+  if (locked == agent_input_lock_requested_) {
+    return Response::Success();
+  }
+  if (locked) {
+    if (!web_contents_) {
+      return Response::InternalError();
+    }
+    agent_input_lock_ = web_contents_->AcquireNativeInputLock();
+    agent_input_lock_requested_ = true;
+  } else {
+    agent_input_lock_requested_ = false;
+    agent_input_lock_ = base::ScopedClosureRunner();
+  }
+  return Response::Success();
+}
+
 void InputHandler::SynthesizePinchGesture(
     double x,
     double y,
