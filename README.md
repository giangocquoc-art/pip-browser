# Pip Browser website

Trang giới thiệu và tải Pip cho Windows: https://pip.psharehub.com

Website là HTML, CSS và JavaScript thuần. Không dùng framework, font ngoài hay thư viện hoạt ảnh. Pip dùng Microsoft Edge WebView2 Runtime có sẵn trên Windows.

## Cấu trúc

- `index.html`: landing page ngắn, nút tải ở phần đầu trang và ảnh chụp ứng dụng thật.
- `tinh-nang/`, `cau-hoi/` và `huong-dan/`: nội dung theo từng ý định tìm kiếm. Tám bài hướng dẫn gom 40 cụm có liên quan vào từng chủ đề thay vì tạo 40 trang mỏng.
- `robots.txt`, `sitemap.xml`: cho phép crawl và liệt kê các URL chuẩn của trang chính, trang tính năng, FAQ, chính sách quyền riêng tư và cụm hướng dẫn.
- `quyen-rieng-tu/`: chính sách mô tả lưu trữ cục bộ, website bên thứ ba, tìm kiếm Google, DNS tùy chọn và chẩn đoán WebView2.
- `guide.css`, `assets/guides/`: bố cục hướng dẫn responsive và tám ảnh WebP có mascot Pip.
- `marketing/seo-keyword-map.md`: bản đồ 40 cụm tìm kiếm, ý định và URL đích; gợi ý autocomplete không phải số liệu lượt tìm kiếm.
- `assets/app-*.webp`: ảnh giao diện được chụp từ ứng dụng Pip; WebP để giảm dung lượng tải.
- `assets/og-pip.jpg`: ảnh chia sẻ mạng xã hội.
- `version.json`: phương án dự phòng nếu GitHub Releases không phản hồi.
- `marketing/launch-kit.md`: định vị và bản nháp nội dung marketing.

## Tải và cập nhật

Nút tải kiểm tra GitHub Releases để lấy bản mới nhất; nếu API không phản hồi, website dùng `version.json`. Mỗi bản mới cần phát hành GitHub Release với tệp `PipSetup.exe`.

Website triển khai từ nhánh `main` trên Vercel. Sau khi cập nhật, kiểm tra trang chủ, trang phụ, ảnh OG, sitemap, liên kết tải và giao diện mobile.

## Thông tin cài đặt

Pip hỗ trợ Windows 10/11 64-bit và cài riêng cho tài khoản Windows hiện tại. Bộ cài chưa ký số; SmartScreen có thể cảnh báo. Trang giới thiệu ghi rõ giới hạn của chế độ ẩn danh, DNS và bộ lọc quảng cáo.
