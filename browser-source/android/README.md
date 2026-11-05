# Pip Android — bản phát triển đầu tiên

Ứng dụng Android native dùng WebView sẵn có trên thiết bị, không đóng gói Chromium.
Mã này dùng API Android có sẵn; không bổ sung framework UI hoặc thư viện chạy nền.
Hiện chưa có APK phát hành công khai, chữ ký release hay chứng nhận Google Play.

## Có trong source

- Ô địa chỉ tìm kiếm Google hoặc mở URL HTTPS; nút quay lại, tiến tới và tải lại.
- Tối đa 8 tab. Menu Tab cho phép chọn, mở và đóng tab.
- Chỉ khởi tạo WebView của tab đã chọn khi khôi phục Activity.
- Giữ URL/tab đang chọn qua recreation; back/forward hoạt động trong WebView còn sống.
- Từ chối popup tự động; cửa sổ mới xuất phát từ thao tác người dùng có thể mở thành tab.
- Safe Browsing, chặn mixed content, không vượt lỗi chứng chỉ, tắt file/content access.
- Không tạo JavaScript interface cho website; camera/mic/quyền website bị từ chối.
- Chỉ xin quyền INTERNET. Debug WebView chỉ bật ở debug build.

## Build và kiểm tra

Cần JDK 17, Gradle **8.13**, Android SDK **36**, Build Tools **35.0.0**.
Android Gradle Plugin được pin **8.11.1** trong build.gradle.kts.
Đường dẫn SDK đặt bằng ANDROID_HOME hoặc local.properties (không commit).
Dùng Android Studio import thư mục này hoặc Gradle đã cài; repo chưa chứa wrapper JAR.

```sh
cd browser-source/android
gradle --no-daemon --max-workers=2 testDebugUnitTest lintDebug assembleDebug
```

Artifact debug dự kiến: `app/build/outputs/apk/debug/app-debug.apk`.
Chỉ coi artifact tồn tại sau khi job build thành công và tải được artifact tương ứng.
APK debug ký bằng debug key của môi trường build, không dùng để cập nhật bản public.
Chưa cấu hình release keystore; không thêm secret hoặc keystore vào repo.

## Giới hạn cần hoàn tất trước khi phát hành

Đây là nền Android ban đầu, chưa ngang chức năng Pip Windows:

- Chưa tích hợp Brave Adblock Rust, Google DNS, Pip Sync, tải tệp, bookmarks/history UI,
  dịch trang, quản lý mật khẩu, file chooser, camera/mic hoặc quy trình default browser.
- Không có chế độ ẩn danh hoặc nhiều hồ sơ: các tab dùng chung kho cookie WebView của app.
  Không thể gọi một tab không ghi lịch sử là hồ sơ riêng tư.
- Chỉ mở HTTPS; HTTP và localhost HTTP chưa hỗ trợ. Không có nút bỏ qua lỗi TLS.
- Recreation khôi phục URL, không khôi phục biểu mẫu, trạng thái JS hay lịch sử back/forward.
  Tab background được pause nhưng WebView vẫn có thể giữ bộ nhớ; giới hạn tab không chứng minh
  mức RAM hoặc pin tốt hơn trình duyệt khác.
- Chưa có engine Android của Pip Dev, MCP native lock, hay chức năng điều khiển agent.
- Icon dùng lại mascot Pip đang có trong repo; chưa có bộ adaptive icon riêng cho Android.
- Cần emulator/thiết bị thật để xác minh layout nhỏ, rotation, bàn phím, HTTPS/video, lỗi mạng,
  nhiều tab, renderer crash, accessibility và behavior background/foreground trước release.

Regression lifecycle cần chạy trên emulator/thiết bị: mở nhiều tab đang tải, chọn tab cuối
rồi đóng liên tục; chuyển tab khi nhận progress/title trễ; hủy renderer cũ rồi tạo WebView
thay thế. Callback của view đã đóng không được đổi URL/progress của tab khác, gây lỗi
index, hoặc xóa view thay thế. Khi Activity bị hủy, callback trễ phải bỏ qua UI.

Tests JVM hiện kiểm tra route URL/search và từ chối scheme/credential không an toàn.
Build/lint/unit pass không thay thế kiểm thử WebView chạy thật hoặc chứng minh chặn quảng cáo.

## Nguồn nền tảng

- [AGP 8.11: Gradle 8.13, JDK 17, API 36](https://developer.android.com/build/releases/agp-8-11-0-release-notes)
- [WebSettings: file access, mixed content, Safe Browsing](https://developer.android.com/reference/android/webkit/WebSettings)
- [WebView](https://developer.android.com/reference/android/webkit/WebView)

iOS nằm ngoài đợt triển khai này.
