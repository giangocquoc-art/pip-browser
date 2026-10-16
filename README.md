# Pip Browser

**Nhanh, gọn và dễ dùng — trình duyệt Windows mang dấu ấn PShareHub.**

Pip dùng WebView2 có sẵn trên Windows để mở các trang web hiện đại mà không phải đóng gói thêm một bộ máy Chromium cồng kềnh. Trang này là trang tải chính thức của Pip, làm bằng HTML, CSS và JavaScript thuần; không cần framework, bước build, font tải từ mạng hay hoạt ảnh nền.

## Tải Pip

Mở [pip.psharehub.com](https://pip.psharehub.com) và chọn **Tải Pip cho Windows**. Bộ cài và gói mã nguồn kèm giấy phép được phát hành tại [GitHub Releases](https://github.com/giangocquoc-art/pip-browser/releases/latest).

- Windows 10/11, 64-bit.
- Cài riêng cho tài khoản Windows hiện tại; không cần quyền quản trị.
- Nếu thiếu WebView2 Runtime, bộ cài sẽ hỏi trước khi tải trình cài chính thức của Microsoft.
- Bộ cài chưa ký số nên Windows SmartScreen có thể hiển thị cảnh báo.

SHA-256 của bộ cài Pip `0.1.1` (Windows x64):

```text
dafa3f7009f4d9b77ca9950861818d8680eaa7e913e66d63f72df1f8a4889c6c
```

## Pip có gì?

- Google là công cụ tìm kiếm mặc định.
- Bộ lọc quảng cáo Brave Adblock Rust chạy trong ứng dụng, kèm danh sách EasyList và quy tắc dành cho YouTube. Quảng cáo YouTube thay đổi thường xuyên theo tài khoản, khu vực và thời điểm nên hiệu quả có thể khác nhau.
- Hồ sơ người dùng lưu dữ liệu duyệt riêng; cửa sổ ẩn danh có giao diện mascot riêng và không ghi lịch sử.
- Gợi ý Google DNS nằm trên trang ẩn danh; bật bằng nút G DNS trên thanh công cụ. Cần khởi động lại Pip để áp dụng thay đổi.
- WebView2 Runtime được Windows cập nhật, giúp bộ cài gọn hơn vì không phải mang theo một trình duyệt Chromium khác.

## Cách cập nhật

Trang tải tự kiểm tra GitHub Releases khi mở. Nếu có phiên bản mới, nút tải trỏ đến bộ cài mới nhất và trang hiện thông báo cập nhật. Kết quả được lưu trong trình duyệt 10 phút; nếu GitHub tạm thời không phản hồi, trang dùng `version.json` làm phương án dự phòng.

Bản phát hành hiện tại là `0.1.1`. Mỗi bản mới được phát hành bằng GitHub Release kèm bộ cài Windows và gói mã nguồn; nút tải tự nhận bản mới nhất. `version.json` giữ liên kết, SHA-256 và kích thước để dùng khi GitHub tạm thời không phản hồi. Mỗi lần cập nhật website, Vercel triển khai commit mới từ nhánh `main`.

## Mã nguồn website

Đây là website tĩnh. Mở `index.html` để xem; đưa thư mục này lên Vercel là đủ. Thiết kế dùng hai ảnh mascot WebP tại `assets/`, không tải thư viện hoặc nội dung giao diện từ máy chủ bên ngoài.
