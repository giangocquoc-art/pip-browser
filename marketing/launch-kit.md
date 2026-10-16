# Pip — gói nội dung giới thiệu (bản nháp)

Ngày chuẩn bị: 2026-10-16. Nội dung này là tài liệu để dùng/chỉnh sửa; chưa đăng lên mạng xã hội và chưa gửi email.

## Nghiên cứu và định vị

**Đối tượng giả định để kiểm tra:** người dùng Windows nói tiếng Việt muốn giao diện browser dễ hiểu, dùng Google, và cần tách dữ liệu giữa các tài khoản. Đây là giả thuyết dựa trên các tính năng Pip đang có và yêu cầu người dùng; website chưa có dữ liệu phân tích người truy cập để xác nhận nhân khẩu học hay nhu cầu.

**Bối cảnh cạnh tranh:** Brave giới thiệu trình duyệt Windows với chặn quảng cáo tích hợp; Microsoft Edge mô tả tính năng ngăn theo dõi; Firefox giải thích cách cửa sổ riêng tư xử lý lịch sử. Các hãng đã có hệ sinh thái và tính năng trưởng thành hơn, vì vậy Pip không nên tự nhận nhanh nhất, riêng tư nhất hoặc chặn quảng cáo tốt hơn.

- [Brave — trang tải chính thức](https://brave.com/download/)
- [Microsoft Edge — ngăn theo dõi](https://support.microsoft.com/en-us/edge/learn-about-tracking-prevention-in-microsoft-edge)
- [Firefox — duyệt riêng tư](https://www.firefox.com/en-US/features/private-browsing/)

**Hàm ý:** Pip nên nói rõ điều mình có hôm nay: giao diện Windows mang mascot Pip, Google làm mặc định, hồ sơ riêng, cửa sổ ẩn danh và bộ lọc quảng cáo chạy nền. Ghi rõ giới hạn YouTube, DNS và SmartScreen để tạo niềm tin thay vì hứa hơn sản phẩm.

**Lợi ích cốt lõi:** Pip gom các thao tác duyệt web quen thuộc của người dùng Windows vào một ứng dụng gọn gàng, có hồ sơ riêng và giao diện PShareHub.

**Định vị:** Pip giúp người dùng Windows tách các phiên duyệt web và bắt đầu tìm kiếm ngay bằng Google, thông qua giao diện đơn giản cùng WebView2 có sẵn trên Windows.

**Góc chiến dịch:** “Trình duyệt vừa đủ, theo cách của bạn.” Tập trung vào trải nghiệm thực tế thay vì so sánh thắng thua với Chrome, Edge, Brave hoặc Firefox.

**Giọng:** gần gũi, cụ thể, tự tin vừa phải; giải thích tính năng bằng lời đơn giản; không dùng lời hứa tuyệt đối.

## Thông điệp trang đích

**Tiêu đề:** Trình duyệt Pip gọn nhẹ cho Windows.  
**Mô tả:** Tìm kiếm Google, hồ sơ riêng và bộ lọc quảng cáo trong một giao diện quen thuộc.  
**CTA chính:** Tải Pip cho Windows.  
**Bằng chứng trên trang:** ảnh chụp ứng dụng thật, yêu cầu Windows 10/11 x64, tải từ GitHub Releases, mã SHA-256, và ghi chú về chữ ký số.

## Email (bản nháp cho danh sách đã đăng ký nhận tin)

1. **Ngày 1 — Giới thiệu:** Tiêu đề “Pip — trình duyệt Windows mang dấu ấn PShareHub”. Nêu Google, hồ sơ riêng và link tải. CTA: Mở trang tải Pip.
2. **Ngày 2 — Giao diện:** Tiêu đề “Một cái nhìn nhanh vào Pip”. Dẫn xem ảnh thật của tab mới, ẩn danh và cài đặt. CTA: Xem giao diện Pip.
3. **Ngày 3 — Hồ sơ:** Tiêu đề “Tách các phiên duyệt trong Pip”. Giải thích mỗi hồ sơ lưu dữ liệu riêng; không gọi đây là đồng bộ tài khoản đám mây. CTA: Xem tính năng hồ sơ.
4. **Ngày 4 — Bộ lọc:** Tiêu đề “Bộ lọc quảng cáo của Pip hoạt động thế nào?”. Mô tả lọc chạy nền, nói rõ YouTube không được đảm bảo. CTA: Đọc giới hạn bộ lọc.
5. **Ngày 5 — Ẩn danh:** Tiêu đề “Cửa sổ ẩn danh làm gì trong Pip?”. Phân biệt không lưu lịch sử với ẩn danh trên mạng; giải thích Google DNS không phải VPN. CTA: Đọc câu hỏi thường gặp.
6. **Ngày 6 — Cài đặt:** Tiêu đề “Trước khi cài Pip trên Windows”. Nêu Windows 10/11 x64, WebView2 và SmartScreen. CTA: Kiểm tra yêu cầu rồi tải.
7. **Ngày 7 — Nhắc tải:** Tiêu đề “Khi nào bạn muốn thử Pip?”. Không tạo khan hiếm giả; nhắc link Releases và gói mã nguồn. CTA: Tải Pip từ GitHub.

Chỉ gửi chuỗi này cho người đã tự nguyện đăng ký; website hiện chưa có form hoặc hệ thống gửi email.

## LinkedIn

1. **Giới thiệu sản phẩm:** Pip là một trình duyệt Windows nhỏ do nhóm PShareHub phát triển. Google, hồ sơ riêng, cửa sổ ẩn danh và bộ lọc quảng cáo tích hợp nằm trong cùng một ứng dụng. Bộ cài và mã nguồn được phát hành qua GitHub. Xem giao diện và yêu cầu tại https://pip.psharehub.com/.
2. **Cách tiếp cận kỹ thuật:** Pip dùng WebView2 có trên Windows để hiển thị website, không đóng gói thêm một Chromium runtime. Mục tiêu là giữ bản cài gọn, trong khi trang tải ghi rõ runtime và điều kiện cần thiết. Chi tiết: https://pip.psharehub.com/tinh-nang/.
3. **Minh bạch về riêng tư:** Cửa sổ ẩn danh không lưu lịch sử vào lịch sử thông thường, nhưng không giấu hoạt động với website hay ISP. Google DNS cũng không phải VPN. Xem giải thích đầy đủ: https://pip.psharehub.com/cau-hoi/.

## X

1. Pip cho Windows: Google mặc định, hồ sơ riêng và giao diện PShareHub. Xem ảnh ứng dụng thật: https://pip.psharehub.com/
2. Mỗi hồ sơ Pip dùng dữ liệu riêng để bạn tách các phiên duyệt trong cùng trình duyệt. https://pip.psharehub.com/tinh-nang/
3. Pip có bộ lọc quảng cáo chạy nền. Quảng cáo YouTube có thể thay đổi và kết quả không được đảm bảo — chi tiết rõ trên trang Pip. https://pip.psharehub.com/cau-hoi/
4. Ẩn danh trong Pip nghĩa là không lưu lịch sử vào lịch sử thông thường; nó không che IP hoặc làm bạn vô hình với website.
5. Pip dùng WebView2 trên Windows, nên không phải mang một bộ Chromium riêng trong ứng dụng. Tải bản Windows x64: https://pip.psharehub.com/

**Thread:** “Pip có gì?” → (1) Trang tab mới có Google và lối tắt. (2) Hồ sơ duyệt lưu dữ liệu riêng. (3) Cửa sổ ẩn danh không lưu lịch sử thông thường. (4) Bộ lọc quảng cáo chạy nền, hiệu quả tùy website. (5) Pip cho Windows 10/11 x64; xem ảnh thật và tải tại pip.psharehub.com.

## Video ngắn

**Kịch bản 1 — 20 giây, “Mở Pip”**  
0–3s: quay icon Pip trên Windows; lời: “Đây là Pip, trình duyệt Windows của PShareHub.”  
3–8s: mở tab mới, gõ Google; lời: “Tìm bằng Google ngay từ trang mới.”  
8–14s: chuyển sang hồ sơ và ẩn danh; lời: “Tách hồ sơ, mở cửa sổ ẩn danh khi cần.”  
14–20s: quay nút tải trên website; lời: “Xem giao diện thật và tải Pip tại pip.psharehub.com.”  
Chú thích trên màn hình: Windows 10/11 x64.

**Kịch bản 2 — 25 giây, “Ẩn danh có nghĩa gì?”**  
0–5s: mở cửa sổ ẩn danh; “Pip không ghi lịch sử của cửa sổ này vào lịch sử thường.”  
5–12s: hiện chú giải; “Nhưng website và mạng bạn dùng vẫn có thể thấy kết nối.”  
12–18s: trỏ nút Google DNS; “DNS là tùy chọn phân giải tên miền, không phải VPN.”  
18–25s: quay trang câu hỏi; “Đọc giới hạn và tải Pip tại pip.psharehub.com/cau-hoi/.”

## Quảng cáo thử nghiệm

- **Giao diện:** Tiêu đề “Trình duyệt Pip cho Windows” · Nội dung “Google, hồ sơ riêng và giao diện PShareHub. Xem ảnh thật, tải qua GitHub Releases.”
- **Tách hồ sơ:** Tiêu đề “Tách phiên duyệt trong Pip” · Nội dung “Dùng hồ sơ Pip riêng cho các phiên làm việc khác nhau. Xem tính năng và yêu cầu Windows.”
- **Chặn quảng cáo:** Tiêu đề “Bộ lọc quảng cáo tích hợp” · Nội dung “Pip chạy bộ lọc nền trên Windows. Đọc giới hạn thực tế, gồm cả YouTube, trước khi tải.”

## Lịch đăng đề xuất

| Ngày | Kênh | Nội dung | Mục tiêu |
|---|---|---|---|
| 1 | Website | Cập nhật landing, ảnh thật, tải và FAQ | Chuyển đổi tải |
| 2 | X | Giới thiệu Pip | Nhận biết |
| 3 | Video ngắn | Mở Pip, tìm Google | Trình diễn |
| 4 | LinkedIn | Cách Pip dùng WebView2 | Giải thích kỹ thuật |
| 5 | X | Hồ sơ riêng | Nêu một lợi ích |
| 6 | Video ngắn | Giới hạn cửa sổ ẩn danh | Xây niềm tin |
| 7 | LinkedIn/X | Bộ lọc và giới hạn YouTube | Đặt kỳ vọng đúng |

## Rà soát trước khi phát hành nội dung

- Không khẳng định Pip là trình duyệt nhẹ nhất, nhanh nhất, an toàn tuyệt đối hoặc chặn hết quảng cáo.
- “Google DNS” không được gọi là VPN, tính năng chặn quảng cáo hay bảo vệ ẩn danh.
- Không đăng số lượt tải, lời chứng thực hay so sánh hiệu năng khi chưa có dữ liệu kiểm chứng.
- Website hiện chưa có analytics/UTM collection hay form email; nội dung trên chỉ là bản nháp, không tự đăng/gửi.
