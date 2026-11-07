# Pip Android — bản phát triển đầu tiên

Ứng dụng Android native dùng WebView sẵn có trên thiết bị, không đóng gói Chromium.
Giao diện dùng API Android có sẵn; bộ lọc mạng dùng Brave adblock-rust qua JNI.
Hiện chưa có APK phát hành công khai, chữ ký release hay chứng nhận Google Play.

## Có trong source

- Ô địa chỉ tìm kiếm Google hoặc mở URL HTTPS; nút quay lại, tiến tới và tải lại.
- Tối đa 8 tab. Menu Tab cho phép chọn, mở và đóng tab.
- Chỉ khởi tạo WebView của tab đã chọn khi khôi phục Activity.
- Khôi phục danh sách URL và tab đang chọn sau recreation hoặc khi mở lại app; không khôi phục back/forward stack, biểu mẫu hay trạng thái JavaScript.
- Lịch sử và dấu trang lưu cục bộ trong SQLite của ứng dụng; lịch sử giữ tối đa 1.000 lượt và giao diện hiển thị 500 lượt gần nhất. Xóa lịch sử không xóa dấu trang.
- Cookie và dữ liệu website không được đưa vào bảng lịch sử/dấu trang; các tab vẫn dùng chung cookie store của Android WebView.
- Từ chối popup tự động; cửa sổ mới xuất phát từ thao tác người dùng có thể mở thành tab.
- Safe Browsing, chặn mixed content, không vượt lỗi chứng chỉ, tắt file/content access.
- Không tạo JavaScript interface cho website; camera/mic/quyền website bị từ chối.
- Chỉ xin quyền INTERNET. Debug WebView chỉ bật ở debug build.
- Main-frame tự chuyển sang tên miền cá cược trong HaGeZi hoặc popup domain overrides bị
  chặn, gồm cả subdomain theo ranh giới nhãn tên miền. URL chính xác được tự nhập/khôi phục
  vẫn được phép; những đích chuyển tiếp khác tiếp tục qua policy. Danh sách được chuẩn hóa
  thành text và bảng offset u32 lúc build, không parse/sort 133k domain khi app mở.
  Policy cũng chạy trong request interception cho main-frame POST vì Android không gọi
  `shouldOverrideUrlLoading` cho POST. Ngoại lệ URL chỉ chuẩn hóa case scheme/host, cổng HTTPS
  mặc định và đường dẫn rỗng thành `/`; query/fragment/path khác không được mở rộng ngoại lệ.
- Engine Rust thật đọc snapshot EasyList, uBlock filters/quick-fixes/unbreak, popup-overrides
  và youtube từ cùng nguồn đã đóng gói trong repo; không tải/copy các danh sách sang Android.
- WebViewClient và ServiceWorkerClient gọi engine trên thread callback nền. Nạp native library
  và phân tích bộ lọc chạy một lần ngoài UI thread. Điều hướng đầu tiên và URL được nhập chờ
  engine sẵn sàng; mỗi tab chỉ giữ URL chờ mới nhất. Nếu nạp thất bại, trang tiếp tục chờ đến
  khi người dùng chạm thông báo và chọn rõ việc duyệt không bảo vệ.

## Build và kiểm tra

Cần JDK 17, Gradle **8.13**, Android SDK **36**, Build Tools **35.0.0**, NDK **27.2.12479018**,
Rust stable cùng targets `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`.
Android Gradle Plugin được pin **8.11.1** trong build.gradle.kts.
Đường dẫn SDK đặt bằng ANDROID_HOME hoặc local.properties (không commit).
Dùng Android Studio import thư mục này hoặc Gradle đã cài; repo chưa chứa wrapper JAR.

```sh
cd browser-source/android
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"
cargo test --manifest-path native/Cargo.toml --locked
bash native/build-android.sh
gradle --no-daemon --max-workers=2 testDebugUnitTest lintDebug assembleDebug
```

Artifact debug dự kiến: `app/build/outputs/apk/debug/app-debug.apk`.
Chỉ coi artifact tồn tại sau khi job build thành công và tải được artifact tương ứng.
APK debug ký bằng debug key của môi trường build, không dùng để cập nhật bản public.
Chưa cấu hình release keystore; không thêm secret hoặc keystore vào repo.

Script native hiện dành cho Linux/hosted CI, dùng NDK clang API 26, xuất `.so` cho
arm64-v8a, armeabi-v7a và x86_64. Không gọi Gradle trước khi build/download đủ native libraries.
Cargo workspace và lockfile của Android độc lập với desktop để tránh feature `single-thread`.
CI build JNI một lần, kiểm tra đủ ABI trong APK và chuyển cùng thư viện sang job emulator.
Native link dùng max/common page size 16 KB; CI kiểm ELF LOAD alignment và APK zipalign.
Các check này không thay thế việc chạy thật trên thiết bị/emulator có page size 16 KB.
Không cần cài NDK hoặc build trên ổ HDD của máy làm việc để dùng CI.

## Giới hạn cần hoàn tất trước khi phát hành

### GeckoView pilot (debug only)

APK `debug` có thêm launcher “Pip GeckoView pilot” dùng GeckoView 153 để kiểm tra tách
cookie theo `contextId`, private mode và cấu hình Google DoH strict (`TRR_MODE_ONLY`).
Pilot chạy song song; app Pip chính và APK release vẫn dùng Android System WebView.
Kiểm thử emulator kiểm tra cookie giữa hai context và private session. Đây chưa phải bằng
chứng request Internet của engine đi qua Google DoH: cần capture/trace mạng riêng trước khi
đánh dấu DNS hoạt động. Không phát hành APK pilot như bản Pip ổn định.

Đây là nền Android ban đầu, chưa ngang chức năng Pip Windows:

- Chưa tích hợp Google DNS, Pip Sync, camera/mic permissions, dịch trang, quản lý mật khẩu
  hoặc quy trình default browser.
- File input mở Android document picker sau khi website yêu cầu; người dùng chọn tệp tường minh.
  Chế độ chọn nhiều tệp được hỗ trợ. Picker không bật camera capture theo yêu cầu của trang;
  trang chỉ nhận URI người dùng chọn, không được Pip tải/chia sẻ tệp tự động. Bộ lọc loại tệp
  hiện để `*/*`, nên một số trang có thể hiện nhiều loại tệp hơn trường HTML gợi ý.
- Tải tệp cơ bản được gửi qua Android DownloadManager vào thư mục Downloads sau khi người dùng
  xác nhận. URL phải là HTTPS; cookie/credential từ Pip không được chuyển cho trình tải hệ thống,
  nên nội dung yêu cầu đăng nhập có thể không tải được. DownloadManager dùng mạng hệ thống,
  không đi qua WebView hoặc bộ lọc request trong Pip. Không hỗ trợ resume trong giao diện Pip,
  upload hay chọn vị trí lưu tùy ý.
- Không có chế độ ẩn danh hoặc nhiều hồ sơ: các tab dùng chung kho cookie WebView của app.
  Không thể gọi một tab không ghi lịch sử là hồ sơ riêng tư.
- Chỉ mở HTTPS; HTTP và localhost HTTP chưa hỗ trợ. Không có nút bỏ qua lỗi TLS.
- Recreation khôi phục URL, không khôi phục biểu mẫu, trạng thái JS hay lịch sử back/forward.
  Tab background được pause nhưng WebView vẫn có thể giữ bộ nhớ; giới hạn tab không chứng minh
  mức RAM hoặc pin tốt hơn trình duyệt khác.
- Chưa có engine Android của Pip Dev, MCP native lock, hay chức năng điều khiển agent.
- GeckoView chỉ có trong debug pilot để đánh giá engine; chưa có parity chặn nội dung, tải tệp,
  quyền website, updater hay kiểm chứng tuyến DNS, và chưa được chọn làm engine mặc định.
- Android lọc request mạng bằng `adblock-rust` và đăng ký scriptlet YouTube ở document-start qua
  AndroidX WebKit khi WebView báo hỗ trợ `DOCUMENT_START_SCRIPT`. Chỉ các origin HTTPS YouTube
  đã liệt kê tường minh được phép nhận scriptlet; nếu WebView cũ không hỗ trợ thì app giữ lọc mạng
  và hoạt động theo chế độ dự phòng. Bộ lọc player response có rule riêng cho `m.youtube.com`;
  regression test Android 15 xác nhận phản hồi player mobile bỏ marker quảng cáo nhưng giữ dữ liệu
  video. Đây là tái sử dụng snapshot YouTube và scriptlet resources
  đã ghim trong repo, không phải cam kết mọi quảng cáo YouTube sẽ biến mất: YouTube thường đổi
  response/player và có thể vẫn phát quảng cáo. Chưa có bài kiểm tra phát YouTube thật xác nhận
  quảng cáo đã hết trên thiết bị. Chặn rộng `googlevideo.com` sẽ chặn cả luồng video bình thường,
  nên không phải cách sửa an toàn. Con số trong trạng thái Android chỉ là request mạng bị chặn,
  không phải số quảng cáo/video ads đã loại. Danh sách cá cược chưa thể
  nhận diện mọi tên miền mới và không thay thế chính sách đầy đủ của bản Windows. URL main-frame do người
  dùng nhập/khôi phục được mở trực tiếp, subresource vẫn lọc. WebView không đưa đủ resource type;
  host suy ra từ request headers và path, nếu không biết dùng `other`.
- ServiceWorker không đưa tab/client ID. Có Referer thì dùng làm source; thiếu thì dùng chính
  URL request theo ngữ cảnh cùng origin, nên có thể bỏ sót rule chỉ dành cho third-party.
  WebView interception không bao phủ mọi redirect endpoint, WebSocket hoặc URL blob/javascript.
  Engine lỗi không làm proxy hoặc giả thành lỗi Internet: callback để mạng bình thường và UI
  báo bộ lọc lỗi khi cập nhật trạng thái trang. Không công bố bảo vệ tuyệt đối.
- Snapshot filter chưa có updater Android. Trước phát hành công khai cần đóng gói notices/
  giấy phép tương ứng adblock-rust, JNI và từng nguồn filter; metadata nguồn hiện ở
  `../crates/app/filters/sources.json`. CI debug không phải bản public đã hoàn tất gate này.
- Icon dùng lại mascot Pip đang có trong repo; chưa có bộ adaptive icon riêng cho Android.
- Cần emulator/thiết bị thật để xác minh layout nhỏ, rotation, bàn phím, HTTPS/video, lỗi mạng,
  nhiều tab, renderer crash, accessibility và behavior background/foreground trước release.

Regression lifecycle cần chạy trên emulator/thiết bị: mở nhiều tab đang tải, chọn tab cuối
rồi đóng liên tục; chuyển tab khi nhận progress/title trễ; hủy renderer cũ rồi tạo WebView
thay thế. Callback của view đã đóng không được đổi URL/progress của tab khác, gây lỗi
index, hoặc xóa view thay thế. Khi Activity bị hủy, callback trễ phải bỏ qua UI.

Tests JVM hiện kiểm tra route URL/search và từ chối scheme/credential không an toàn.
Build/lint/unit pass không thay thế kiểm thử WebView chạy thật hoặc chứng minh chặn quảng cáo.

## Kiểm thử Activity và WebView trên emulator

Source instrumentation dùng AndroidX Test chỉ trong APK test, không thêm dependency chạy
vào ứng dụng chính. Cấu hình runner là `androidx.test.runner.AndroidJUnitRunner`.

```sh
gradle --no-daemon --max-workers=2 connectedDebugAndroidTest
```

`MainActivityTest` mở Activity/WebView thật và kiểm tra chọn tab, khôi phục URL cùng tab
đang chọn sau `ActivityScenario.recreate()`, đóng tab cuối và callback của view cũ sau khi
tạo view thay thế. Test không chờ Google hoặc website bên ngoài trả kết quả: URL `.invalid`
được nhập qua omnibox và WebView dừng tải ngay; có thể tắt Wi-Fi/data của emulator.
Nhánh renderer exit được kiểm bằng callback mô phỏng, không phải chủ động crash tiến trình
renderer. Vẫn cần kiểm tra renderer crash thật, Back gesture, video, bàn phím và thiết bị thật.

`BrowserDataStoreTest` dùng SQLite thật trên emulator để kiểm tra lịch sử/dấu trang sau khi mở lại kho,
xóa lịch sử độc lập với dấu trang và giới hạn 1.000 lượt. `MainActivityTest` còn đóng rồi mở lại Activity
để kiểm tra URL các tab và tab đang chọn được lưu bền. Các bài này không chứng minh dữ liệu được cách ly
theo hồ sơ; Android hiện vẫn dùng chung cookie store và chưa có ẩn danh.

Việc có source test chưa chứng minh test đã chạy; dùng kết quả CI tương ứng với đúng commit.

`AdblockRequestTest` bắt buộc load JNI thật, rồi cho WebView thật tải trang HTTPS fixture
và các script được trả cục bộ trong callback chỉ của test. Nó kiểm chứng callback production
chặn host quảng cáo bằng 204 nhưng cho control request qua; test thứ hai đăng ký ServiceWorker
thật, yêu cầu worker fetch cả hai URL và kiểm chứng hook ServiceWorker production riêng.
Không gọi live Google/DoubleClick, không bỏ qua lỗi TLS, không thêm JavaScript interface.
Unit Rust kiểm tra engine với toàn bộ bundled network inputs, exception và resource types.
Regression navigation bổ sung kiểm tra domain/subdomain, lookalike, Google và URL nhập rõ
qua omnibox bằng callback production với request giả lập. Test riêng tải HTML fixture thật
chạy `location.assign` sang domain cá cược, delegate callback production và kiểm tra đích bị
chặn trước request/commit. Chưa kiểm chuỗi HTTP redirect nhiều chặng hoặc mọi kiểu popup;
cần kiểm thêm trên thiết bị thật. Source test cần CI tương ứng chạy thành công trước khi tuyên bố pass.
Test form tự POST thật kiểm thêm policy interception; mọi response fixture chỉ trả cục bộ,
kể cả trường hợp regression, không gửi form đến tên miền ngoài. JVM test giữ phạm vi ngoại lệ URL.

## Nguồn nền tảng

- [AGP 8.11: Gradle 8.13, JDK 17, API 36](https://developer.android.com/build/releases/agp-8-11-0-release-notes)
- [WebSettings: file access, mixed content, Safe Browsing](https://developer.android.com/reference/android/webkit/WebSettings)
- [WebView](https://developer.android.com/reference/android/webkit/WebView)
- [AndroidX Test dependencies](https://developer.android.com/jetpack/androidx/releases/test)
- [ActivityScenario](https://developer.android.com/reference/androidx/test/core/app/ActivityScenario)

iOS nằm ngoài đợt triển khai này.
