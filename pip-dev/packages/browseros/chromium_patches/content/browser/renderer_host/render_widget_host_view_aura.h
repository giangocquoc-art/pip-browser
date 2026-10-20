diff --git a/content/browser/renderer_host/render_widget_host_view_aura.h b/content/browser/renderer_host/render_widget_host_view_aura.h
index e63a9870c1..0e4212b519 100644
--- a/content/browser/renderer_host/render_widget_host_view_aura.h
+++ b/content/browser/renderer_host/render_widget_host_view_aura.h
@@ -514,6 +514,8 @@ class CONTENT_EXPORT RenderWidgetHostViewAura
   void CleanUpHostObservers() override;
 
  private:
+  bool IsNativeInputLocked() const;
+
   friend class DelegatedFrameHostClientAura;
   friend class FakeRenderWidgetHostViewAura;
   friend class InputMethodAuraTestBase;
