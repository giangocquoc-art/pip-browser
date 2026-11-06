# Pip trên Android, macOS và Windows

Phạm vi được bổ sung ngày 2026-11-06: phát triển Pip cho Android, macOS và Windows. iOS chưa thuộc đợt này. Đây là mã phát triển. APK debug và ứng dụng macOS hai kiến trúc đã qua kiểm thử CI cơ bản; chưa đạt các điều kiện để đưa lên trang tải công khai.

## Nền tảng và phần tái sử dụng

| Nền tảng | Engine | Trạng thái |
| --- | --- | --- |
| Windows 10/11 x64 | Microsoft Edge WebView2, Rust/Wry hiện có | Bản Pip hiện hữu; giữ luồng build và installer Windows |
| macOS, Apple Silicon và Intel | WKWebView của macOS qua Wry/winit đang dùng | Shell phát triển, build qua binary `pip-macos` |
| Android 8 trở lên | Android System WebView, giao diện native | Ứng dụng phát triển trong `android/`; GeckoView mới là pilot debug, chưa đổi engine |
| iOS | Chưa chọn | Để sau theo yêu cầu |

Pip dùng engine do hệ điều hành cung cấp để tránh đóng gói thêm một Chromium riêng. Pip Dev vẫn dùng nền Chromium/BrowserOS riêng: shell Android/macOS trong tài liệu này không phải bản port của MCP hay native input lock Pip Dev.

Mã Windows phụ thuộc WebView2 khá sâu. Shell macOS và Android phải có adapter cho tải trang, popup, lưu dữ liệu, tải xuống và vòng đời tab trước khi đạt đủ chức năng; không thể coi đổi tên gói hoặc thêm điều kiện compile là hoàn tất port.

## Mức chức năng cần đạt trước bản công khai

- Điều hướng HTTPS, Google mặc định, tab, quay lại/tiến tới/tải lại và khôi phục phiên an toàn.
- Quyền website theo nền tảng; TLS lỗi phải bị từ chối; không cho website gọi bridge điều khiển giao diện trình duyệt.
- Dữ liệu hồ sơ độc lập và chế độ riêng tư thật. Nhiều WebView dùng chung cookie store không được quảng bá là nhiều hồ sơ độc lập.
- Chặn quảng cáo/popup qua hook mạng đúng engine. macOS đã chạy thử bộ lọc Brave chuyển sang WKContentRuleList và policy điều hướng cờ bạc; chưa có scriptlet/YouTube parity. Android đã có adapter Rust/JNI cho WebView và service worker; phải kiểm chứng callback native, source context và các giới hạn riêng.
- Google DNS phải đi qua đường mạng thật của tab và có kiểm tra bật/tắt; không hiển thị nút báo thành công nếu chỉ cấu hình một HTTP client khác.
- Lịch sử/dấu trang/tải xuống, mật khẩu theo kho an toàn của hệ điều hành và Pip Sync có mã hóa; dữ liệu website không chuyển ngầm giữa các engine.
- Giao diện phù hợp chuột/phím trên desktop và cảm ứng trên Android, hỗ trợ cỡ chữ hệ thống, nhãn truy cập và không thêm hoạt ảnh nặng.
- Pip Dev: ưu tiên Windows/macOS desktop cho MCP + DevTools, chỉ công bố nền tảng sau runtime test riêng. Android không mặc nhiên hỗ trợ extension hoặc MCP/native lock của bản desktop.

Các shell phát triển không quảng bá những mục còn thiếu như thể đã hoạt động.

## Build và kiểm thử

Windows tiếp tục dùng hướng dẫn và workflow hiện tại trong [README](README.md), [verify-pip.yml](../.github/workflows/verify-pip.yml). Không đưa feature macOS vào bộ cài Windows.

macOS dùng Rust stable, Xcode command line tools và system WebKit:

```sh
cd browser-source
cargo test -p evergreen-core --locked
cargo test -p evergreen-browser --bin pip-macos --features macos-shell --locked
cargo build -p evergreen-browser --bin pip-macos --features macos-shell --release --locked --target aarch64-apple-darwin
bash scripts/package-macos.sh aarch64-apple-darwin
```

Máy Intel thay target bằng `x86_64-apple-darwin`. Chỉ `.app` đã build/chạy thử đúng kiến trúc mới chứng minh tương thích; chạy qua Rosetta không thay kiểm thử native ARM64.

Android xem [android/README.md](android/README.md). Toolchain được pin: JDK 17, Gradle 8.13, Android Gradle Plugin 8.11.1, SDK 36. Không cần cài toolchain hay chạy build nặng trên ổ D của máy người dùng; workflow hosted thực hiện build.

Pilot GeckoView chỉ nằm trong `debugImplementation` và `src/debug`, có launcher riêng trên
APK debug. APK release hiện tại vẫn dùng WebView. Pilot kiểm tra cách tạo context ID riêng,
private mode và một fixture cookie chạy trên emulator; UI ghi rõ Google DoH mới được cấu hình
`TRR_MODE_ONLY`, tuyến DNS thực vẫn cần trace mạng. Không dùng kết quả pilot để bật engine,
DNS hoặc tính năng hồ sơ trên bản tải công khai.

[Verify Pip platforms](../.github/workflows/verify-pip-platforms.yml) chạy test/lint/build APK debug và build/test/package macOS trên runner Apple Silicon + Intel. Windows vẫn được kiểm tra bởi workflow hiện hữu. Workflow mới chỉ upload artifact phát triển với thời hạn 7 ngày, không tự tạo release hoặc thay link tải.

## Bằng chứng kiểm thử nền hiện tại

Run PR [42162964010](https://github.com/giangocquoc-art/pip-browser/actions/runs/42162964010) và
[42162964065](https://github.com/giangocquoc-art/pip-browser/actions/runs/42162964065) trên PR #65
đều xanh tại head `7d97ba8`. Windows browser/installer checks, Android APK/JNI, emulator Android 15,
macOS Intel/ARM, Sync Worker và Download Worker đều qua. PR #65 đã gộp vào `main` tại `6352abb`.
Các artifact CI là development builds; điều này không xác nhận thiết bị Android thật, ký phát hành,
notarization macOS hoặc trạng thái website tải xuống.

[Run 42090197137](https://github.com/giangocquoc-art/pip-browser/actions/runs/42090197137), commit `d10142868cb11dd7430e296a7e0158f0f6513000`, đã hoàn tất thành công cả bốn job:

- Android: JVM tests, lint và build APK debug; emulator Android 15 chạy Activity/WebView thật để kiểm tra chọn/đóng tab, khôi phục URL qua recreation và bỏ callback của view cũ. Nhánh renderer exit được mô phỏng bằng callback; không chứng minh renderer crash thật hay truy cập Internet/video.
- macOS Apple Silicon và Intel: test shared core/shell, build, package và chạy binary lấy từ ZIP. `--verify-ui` kiểm tra DOM native, tạo/chọn/đóng tab, toolbar IPC và xác nhận website không có IPC bridge. Cả hai báo `ok: true`; fixture không dùng mạng hoặc screenshot.
- Artifact phát triển được giữ 7 ngày trên run trên; APK ký debug và macOS ký ad-hoc. Kích thước nhỏ của shell không bao gồm engine WebView hệ thống, không dùng để so sánh tổng dung lượng hay RAM với Chrome.

Bằng chứng này chỉ áp dụng đúng commit và phạm vi nêu trên; thay đổi tiếp theo cần CI riêng. Chặn quảng cáo, Google DNS, đồng bộ, hồ sơ riêng, mạng/video thật và trải nghiệm trên thiết bị vẫn cần hoàn thiện/kiểm chứng theo các gate bên dưới.

[Run 42092953077](https://github.com/giangocquoc-art/pip-browser/actions/runs/42092953077), commit `502868658f8f77dd1100874ffa0197cb656acbf5`, kiểm chứng thêm native adblock macOS trên cả hai kiến trúc. WebKit biên dịch 81.271 quy tắc, converter dùng 81.145 quy tắc nguồn trong 91.653 dòng không phải comment. Fixture loopback xác nhận script thường và ngoại lệ được tải, script bị chặn không đến server; `adblockRequests: true` và `ok: true`. Các con số này không chứng minh bao phủ đầy đủ syntax hay quảng cáo YouTube thực tế.

Mã tải tệp macOS tiếp theo dùng NSSavePanel và WKDownload, không tự mở tệp hoặc ghi đè tệp đã có. Cần CI kiểm tra attachment thật trước khi công bố chức năng đã hoạt động. Android JNI ở commit `7ecca94248137bd5f69f1882ed1d6c4163e313fd` đã qua host Rust tests, build ba ABI, lint/APK và kiểm tra alignment; kết quả WebView/service-worker emulator phải xem đúng run [42108484729](https://github.com/giangocquoc-art/pip-browser/actions/runs/42108484729).

## Điều kiện phát hành

- Android: unit tests và lint xanh; cài APK trên emulator và ít nhất một thiết bị thật; kiểm tra xoay màn hình, Back, tab, bàn phím, media, TLS, popup và khởi động lại. APK debug dùng khóa debug, không phải gói phát hành. APK/AAB công khai cần khóa ký riêng được giữ bí mật; nâng cấp phải dùng cùng khóa.
- macOS: hai kiến trúc build xanh; mở app thật, duyệt trang/test localhost, tab, Cmd shortcuts, download, resume và permission; kiểm tra chữ ký, Gatekeeper/notarization trước public distribution. Ad-hoc signing không tương đương Developer ID/notarization.
- Windows: kiểm tra lại installer và chức năng hiện có để tránh hồi quy.
- Website: chỉ bật nút tải nền tảng khi artifact đúng phiên bản đã kiểm tra tồn tại, kèm nguồn/hash/kích thước; không để nút Android hoặc Mac tải nhầm EXE Windows.
- Chưa đo dung lượng, RAM, CPU hay độ trễ trên thiết bị thì không công bố số hoặc cam kết “nhẹ nhất”.

Trang chủ và Download Worker hiện nhận diện asset từ bản phát hành ổn định theo tên có phiên bản:
`Pip-Android-<version>-universal.apk`, `Pip-macOS-arm64-<version>.zip` và
`Pip-macOS-x64-<version>.zip`. Android chỉ nên dùng APK release đã ký bằng khóa bền vững;
macOS chỉ dùng gói đã kiểm thử, ký và notarize trước khi gắn các asset này vào stable release.
Nếu asset thiếu, trang chủ không hiện nút tải nền tảng đó.

## Tài liệu nền

Đánh giá các browser viết bằng Zig và phần có thể tái sử dụng trong Pip nằm ở
[ZIG_BROWSER_RESEARCH.md](ZIG_BROWSER_RESEARCH.md). Hiện không thay engine hệ điều hành bằng
engine thử nghiệm: Lightpanda nhắm headless automation, còn Suzume chưa thay thế được mức tương
thích/bảo mật của WebView2, WKWebView hay Android WebView.

- [Wry](https://docs.rs/wry/0.57.0/wry/) và [winit](https://docs.rs/winit/0.30.12/winit/) là các thành phần đã dùng trong Pip.
- [Android WebView](https://developer.android.com/develop/ui/views/layout/webapps/webview), [bảo mật WebView](https://developer.android.com/privacy-and-security/risks/unsafe-uri-loading).
- [AGP 8.11: Gradle/JDK/SDK tương thích](https://developer.android.com/build/releases/past-releases/agp-8-11-0-release-notes).
- [GitHub hosted runner architectures](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
