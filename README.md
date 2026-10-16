# Pip Browser website

Trang giới thiệu và tải Pip cho Windows: https://pip.psharehub.com

Website là HTML, CSS và JavaScript thuần. Không dùng framework, font ngoài hay thư viện hoạt ảnh. Pip dùng Microsoft Edge WebView2 Runtime có sẵn trên Windows.

## Cấu trúc

- `index.html`: landing page ngắn, nút tải ở phần đầu trang và ảnh chụp ứng dụng thật.
- `tinh-nang/` và `cau-hoi/`: nội dung chi tiết theo ý định tìm kiếm, liên kết từ trang chủ.
- `robots.txt`, `sitemap.xml`: cho phép crawl và liệt kê ba URL chuẩn.
- `assets/app-*.webp`: ảnh giao diện được chụp từ ứng dụng Pip; WebP để giảm dung lượng tải.
- `assets/og-pip.jpg`: ảnh chia sẻ mạng xã hội.
- `version.json`: phương án dự phòng nếu GitHub Releases không phản hồi.
- `marketing/launch-kit.md`: định vị và bản nháp nội dung marketing.

## Tải và cập nhật

Nút tải kiểm tra GitHub Releases để lấy bản mới nhất; nếu API không phản hồi, website dùng `version.json`. Mỗi bản mới cần phát hành GitHub Release với tệp `Pip-Setup-<version>-Windows-x64.exe`.

Website triển khai từ nhánh `main` trên Vercel. Sau khi cập nhật, kiểm tra trang chủ, trang phụ, ảnh OG, sitemap, liên kết tải và giao diện mobile.

## Thông tin cài đặt

Pip hỗ trợ Windows 10/11 64-bit và cài riêng cho tài khoản Windows hiện tại. Bộ cài chưa ký số; SmartScreen có thể cảnh báo. Trang giới thiệu ghi rõ giới hạn của chế độ ẩn danh, DNS và bộ lọc quảng cáo.
