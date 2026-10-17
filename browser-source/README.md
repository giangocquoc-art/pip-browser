# Pip Browser

Pip là trình duyệt Windows 10/11 x64 dựa trên Microsoft Edge WebView2, với phần giao diện và quản lý tab viết bằng Rust. Pip dùng Google làm công cụ tìm kiếm mặc định, hỗ trợ hồ sơ riêng, cửa sổ ẩn danh và bộ lọc nội dung tích hợp.

## Tính năng

- Tab, thanh địa chỉ, trang tab mới và menu theo giao diện Pip.
- Hồ sơ riêng lưu dữ liệu website độc lập; cửa sổ ẩn danh không lưu lịch sử duyệt web.
- Brave Adblock Rust xử lý bộ lọc EasyList, uBlock Origin và danh sách popup HaGeZi được đóng gói sẵn. Khi bộ lọc bật cho website, Pip chặn popup tự mở và đối chiếu đích popup với danh sách quảng cáo, popup và cá cược HaGeZi; tên miền phụ của miền cá cược trong danh sách cũng bị nhận diện. Quy tắc cá cược chỉ áp dụng cho popup, không chặn điều hướng bình thường. YouTube và website khác có thể thay đổi cách phân phối quảng cáo nên không thể cam kết chặn mọi quảng cáo.
- Google DNS bảo mật là tùy chọn. DNS không ẩn địa chỉ IP và không thay thế VPN.
- Dùng WebView2 Runtime do Microsoft cập nhật; Pip không mang theo một bản Chromium riêng.

Xem [hướng dẫn sử dụng](PIP_README.md), [ghi chú Pip 0.1.5](RELEASE_NOTES.md), [nguồn và giấy phép bên thứ ba](THIRD_PARTY.md), cùng thư mục `licenses/`.

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
