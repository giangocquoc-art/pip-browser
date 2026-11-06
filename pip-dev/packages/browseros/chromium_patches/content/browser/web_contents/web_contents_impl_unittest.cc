diff --git a/content/browser/web_contents/web_contents_impl_unittest.cc b/content/browser/web_contents/web_contents_impl_unittest.cc
index 575658e354..2378616d4a 100644
--- a/content/browser/web_contents/web_contents_impl_unittest.cc
+++ b/content/browser/web_contents/web_contents_impl_unittest.cc
@@ -3803,6 +3803,20 @@ TEST_F(WebContentsImplTest, IgnoreInputEvents) {
   EXPECT_FALSE(contents()->ShouldIgnoreInputEvents());
 }
 
+TEST_F(WebContentsImplTest, NativeInputLocksAreReferenceCounted) {
+  EXPECT_FALSE(contents()->IsNativeInputLocked());
+
+  auto lock_1 = contents()->AcquireNativeInputLock();
+  auto lock_2 = contents()->AcquireNativeInputLock();
+  EXPECT_TRUE(contents()->IsNativeInputLocked());
+
+  lock_1.RunAndReset();
+  EXPECT_TRUE(contents()->IsNativeInputLocked());
+
+  lock_2.RunAndReset();
+  EXPECT_FALSE(contents()->IsNativeInputLocked());
+}
+
 TEST_F(WebContentsImplTest, OnColorProviderChangedTriggersPageBroadcast) {
   TestColorProviderSource color_provider_source;
   mojo::AssociatedRemote<blink::mojom::PageBroadcast> broadcast_remote;
