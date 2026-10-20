diff --git a/content/browser/renderer_host/render_widget_host_view_aura_unittest.cc b/content/browser/renderer_host/render_widget_host_view_aura_unittest.cc
index 42cda09cc0..762502df7c 100644
--- a/content/browser/renderer_host/render_widget_host_view_aura_unittest.cc
+++ b/content/browser/renderer_host/render_widget_host_view_aura_unittest.cc
@@ -1186,6 +1186,32 @@ TEST_F(RenderWidgetHostViewAuraTest, InitController) {
 }
 #endif  // BUILDFLAG(IS_WIN)
 
+TEST_F(RenderWidgetHostViewAuraTest,
+       NativeInputLockBlocksPhysicalEventsWithoutBlockingDevTools) {
+  InitViewForFrame(nullptr);
+  auto input_lock = web_contents_->AcquireNativeInputLock();
+
+  ui::MouseEvent mouse_event(ui::EventType::kMousePressed, gfx::Point(),
+                             gfx::Point(), ui::EventTimeForNow(),
+                             ui::EF_LEFT_MOUSE_BUTTON, 0);
+  view_->OnMouseEvent(&mouse_event);
+  EXPECT_TRUE(mouse_event.handled());
+
+  ui::KeyEvent key_event(ui::EventType::kKeyPressed, ui::VKEY_A,
+                         ui::DomCode::US_A, ui::EF_NONE);
+  view_->OnKeyEvent(&key_event);
+  EXPECT_TRUE(key_event.handled());
+
+  // DevTools dispatch does not use WebContents' global ignore-input flag; it
+  // enters RenderWidgetHostImpl directly and remains available to the agent.
+  EXPECT_FALSE(web_contents_->ShouldIgnoreInputEventsForTesting());
+  EXPECT_TRUE(web_contents_->IsNativeInputLocked());
+  EXPECT_TRUE(GetAndResetDispatchedMessages().empty());
+
+  input_lock.RunAndReset();
+  EXPECT_FALSE(web_contents_->IsNativeInputLocked());
+}
+
 // Checks that moving parent sends new screen bounds.
 TEST_F(RenderWidgetHostViewAuraTest, ParentMovementUpdatesScreenRect) {
   InitViewForFrame(nullptr);
