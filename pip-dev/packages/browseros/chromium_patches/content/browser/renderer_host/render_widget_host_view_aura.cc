diff --git a/content/browser/renderer_host/render_widget_host_view_aura.cc b/content/browser/renderer_host/render_widget_host_view_aura.cc
index 0bca4235e9..8c4fed2cb0 100644
--- a/content/browser/renderer_host/render_widget_host_view_aura.cc
+++ b/content/browser/renderer_host/render_widget_host_view_aura.cc
@@ -52,6 +52,7 @@
 #include "content/browser/renderer_host/render_widget_host_delegate.h"
 #include "content/browser/renderer_host/render_widget_host_impl.h"
 #include "content/browser/renderer_host/render_widget_host_view_event_handler.h"
+#include "content/browser/web_contents/web_contents_impl.h"
 #include "content/browser/renderer_host/unbounded_surface_window_aura.h"
 #include "content/browser/renderer_host/visible_time_request_trigger.h"
 #include "content/public/browser/content_browser_client.h"
@@ -2512,6 +2513,10 @@ void RenderWidgetHostViewAura::OnKeyEvent(ui::KeyEvent* event) {
     event->SetHandled();
     return;
   }
+  if (IsNativeInputLocked()) {
+    event->SetHandled();
+    return;
+  }
   ScopedInputDispatchPin pin(this);
   last_pointer_type_ = ui::EventPointerType::kUnknown;
 
@@ -2554,6 +2559,10 @@ void RenderWidgetHostViewAura::OnMouseEvent(ui::MouseEvent* event) {
     event->SetHandled();
     return;
   }
+  if (IsNativeInputLocked()) {
+    event->SetHandled();
+    return;
+  }
   ScopedInputDispatchPin pin(this);
 #if BUILDFLAG(IS_WIN)
   if (event->type() == ui::EventType::kMouseMoved) {
@@ -2794,6 +2803,10 @@ void RenderWidgetHostViewAura::OnScrollEvent(ui::ScrollEvent* event) {
     event->SetHandled();
     return;
   }
+  if (IsNativeInputLocked()) {
+    event->SetHandled();
+    return;
+  }
   ScopedInputDispatchPin pin(this);
   event_handler_->OnScrollEvent(event);
 }
@@ -2803,6 +2816,10 @@ void RenderWidgetHostViewAura::OnTouchEvent(ui::TouchEvent* event) {
     event->SetHandled();
     return;
   }
+  if (IsNativeInputLocked()) {
+    event->SetHandled();
+    return;
+  }
   ScopedInputDispatchPin pin(this);
   last_pointer_type_ = event->pointer_details().pointer_type;
 
@@ -2848,11 +2865,25 @@ void RenderWidgetHostViewAura::OnGestureEvent(ui::GestureEvent* event) {
     event->SetHandled();
     return;
   }
+  if (IsNativeInputLocked()) {
+    event->SetHandled();
+    return;
+  }
   ScopedInputDispatchPin pin(this);
   last_pointer_type_ = event->details().primary_pointer_type();
   event_handler_->OnGestureEvent(event);
 }
 
+bool RenderWidgetHostViewAura::IsNativeInputLocked() const {
+  if (!host()) {
+    return false;
+  }
+  auto* render_widget_host = static_cast<RenderWidgetHostImpl*>(host());
+  auto* web_contents =
+      WebContentsImpl::FromRenderWidgetHostImpl(render_widget_host);
+  return web_contents && web_contents->IsNativeInputLocked();
+}
+
 std::string_view RenderWidgetHostViewAura::GetLogContext() const {
   return "RenderWidgetHostViewAura";
 }
