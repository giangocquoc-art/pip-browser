# Pip Browser

Pip là trình duyệt Windows 10/11 x64 dựa trên Microsoft Edge WebView2, với phần giao diện và quản lý tab viết bằng Rust. Pip dùng Google làm công cụ tìm kiếm mặc định, hỗ trợ hồ sơ riêng, cửa sổ ẩn danh và bộ lọc nội dung tích hợp.

## Tính năng

- Tab, thanh địa chỉ, trang tab mới và menu theo giao diện Pip.
- Nhấp phải vào tab để tạo nhóm, chuyển tab giữa các nhóm hoặc bỏ nhóm. Tên nhóm và màu được lưu cùng phiên tab của cửa sổ.
- Dịch văn bản trang sang tiếng Việt bằng Translator API trên thiết bị của WebView2 khi runtime hỗ trợ; có thể khôi phục văn bản gốc.
- Hồ sơ riêng lưu dữ liệu website độc lập; cửa sổ ẩn danh không lưu lịch sử duyệt web.
- Quản lý quyền website theo hồ sơ; xem và đặt lại quyền đã lưu để website hỏi lại.
- Quản lý tiện ích mở rộng tương thích WebView2: cài thư mục unpacked, bật/tắt và gỡ theo hồ sơ. WebView2 không có thanh công cụ extension cho biểu tượng/badge/popup, và Pip chưa đưa mục context menu do extension khai báo vào menu chuột phải. Tiện ích có thể chạy script nền/nội dung nhưng popup có thể không dùng được; hãy xem `permissions` và `host_permissions` trong manifest trước khi cài.
- Pip Sync mã hóa đầu-cuối theo hồ sơ, cho phép bật riêng dấu trang, lịch sử, tab đang mở, một nhóm cài đặt allowlist và kho mật khẩu Pip. Lịch sử, tab và mật khẩu cần bật riêng; mật khẩu có xác nhận và mặc định tắt. Tab ẩn danh luôn bị loại; cookie, mật khẩu WebView2 và thông tin thanh toán không đồng bộ.
- Tự ngủ tab nền sau thời gian bạn chọn để giảm hoạt động nền và yêu cầu WebView2 tiết kiệm bộ nhớ; mức RAM thu hồi tùy trang và runtime. Khi RAM trống dưới 512 MiB, tab ngủ ít nhất 30 phút có thể được nhả renderer nếu Pip không phát hiện tương tác, dữ liệu biểu mẫu chưa gửi, media hoặc `beforeunload`. Chọn lại sẽ tải URL; dữ liệu mà website chỉ giữ trong mã trang có thể mất. Tab đang tải, phát âm thanh/video, dùng camera/micrô, được ghim hoặc được chọn giữ thức sẽ không bị nhả.
- Brave Adblock Rust xử lý luật mạng EasyList/uBlock Origin, procedural cosmetic filters và popup HaGeZi; request từ document, service worker và shared worker được lọc khi WebView2 Runtime hỗ trợ. Menu Pip có **Sửa lỗi phát video** để tắt lọc cho website hiện tại rồi tải lại sau khi người dùng xác nhận. Luật redirect dùng một số tài nguyên thay thế cục bộ đã đóng gói. YouTube và website khác có thể thay đổi cách phân phối quảng cáo nên không thể cam kết chặn mọi quảng cáo.
- Google DNS bảo mật là tùy chọn. DNS không ẩn địa chỉ IP và không thay thế VPN.
- Dùng WebView2 Runtime do Microsoft cập nhật; Pip không mang theo một bản Chromium riêng.

Xem [hướng dẫn sử dụng](PIP_README.md), [ghi chú phát hành](RELEASE_NOTES.md), [nguồn và giấy phép bên thứ ba](THIRD_PARTY.md), cùng thư mục `licenses/`.

## Build trên Windows

Cần Rust stable cho MSVC, Python 3 và WebView2 SDK/runtime theo yêu cầu của dự án. Chạy kiểm tra và build:

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1
```

Bộ cài `dist/PipSetup.exe` được cài theo tài khoản Windows hiện tại. Nếu Pip chưa được ký bằng chứng thư Authenticode công khai, Windows có thể hiển thị cảnh báo SmartScreen. Bản phát hành GitHub có checksum và GitHub artifact attestation để kiểm tra hash cũng như nguồn build; attestation không thay thế chữ ký Authenticode.

## Giấy phép

Pip kế thừa mã nguồn từ Evergreen Browser và sử dụng các thành phần bên thứ ba. Xem `LICENSE`, `THIRD_PARTY.md` và các giấy phép được đóng gói để biết điều khoản áp dụng.
