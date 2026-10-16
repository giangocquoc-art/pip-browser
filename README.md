# Pip Browser

**Nhanh, gọn và dễ dùng — trình duyệt Windows mang dấu ấn PShareHub.**

Pip dùng WebView2 có sẵn trên Windows để mở các trang web hiện đại mà không phải đóng gói thêm một bộ máy Chromium cồng kềnh. Trang này là trang tải chính thức của Pip, làm bằng HTML, CSS và JavaScript thuần; không cần framework, bước build, font tải từ mạng hay hoạt ảnh nền.

## Tải Pip

Mở [pip.psharehub.com](https://pip.psharehub.com) và chọn **Tải Pip cho Windows**. Bộ cài và gói mã nguồn kèm giấy phép được phát hành tại [GitHub Releases](https://github.com/giangocquoc-art/pip-browser/releases/latest).

- Windows 10/11, 64-bit.
- Cài riêng cho tài khoản Windows hiện tại; không cần quyền quản trị.
- Nếu thiếu WebView2 Runtime, bộ cài sẽ hỏi trước khi tải trình cài chính thức của Microsoft.
- Bộ cài chưa ký số nên Windows SmartScreen có thể hiển thị cảnh báo.

SHA-256 của bản `0.1.0`:

```text
164d07fe426a113f2c7dc47dc4bbdc83ba33ce24bca70a17a091af8f38ab03b4
```

## Pip có gì?

- Google là công cụ tìm kiếm mặc định.
- Bộ lọc quảng cáo Brave Adblock Rust chạy trong ứng dụng, kèm danh sách EasyList và quy tắc dành cho YouTube. Quảng cáo YouTube thay đổi thường xuyên theo tài khoản, khu vực và thời điểm nên hiệu quả có thể khác nhau.
- Hồ sơ người dùng lưu dữ liệu duyệt riêng; cửa sổ ẩn danh dùng phiên riêng.
- WebView2 Runtime được Windows cập nhật, giúp bộ cài gọn hơn vì không phải mang theo một trình duyệt Chromium khác.

## Cách cập nhật

Trang tải đọc `version.json` khi mở. Khi có phiên bản mới, trang hiện thông báo và nút tải bản mới. Để phát hành cập nhật:

1. Đăng bộ cài mới trong một GitHub Release, với tag dạng `v0.1.1`.
2. Cập nhật `version`, `url`, `sha256`, `size` và `updated` trong `version.json`.
3. Đặt phiên bản phát hành mới trong trang, rồi đẩy thay đổi lên nhánh `main`.

Vercel tự triển khai thay đổi của website sau mỗi lần push lên `main`. Không cần tải lại trang hoặc phát hành lại website bằng tay.

## Mã nguồn website

Đây là website tĩnh. Mở `index.html` để xem; đưa thư mục này lên Vercel là đủ. Thiết kế dùng hai ảnh mascot WebP tại `assets/`, không tải thư viện hoặc nội dung giao diện từ máy chủ bên ngoài.
