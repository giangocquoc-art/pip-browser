# Đánh giá repo trình duyệt viết bằng Zig cho Pip

Cập nhật: 2026-11-07. Phạm vi: tìm code/repo có thể giảm footprint Pip mà không làm mất
khả năng duyệt web phổ thông, tương thích nền tảng hoặc bảo mật.

## Kết luận

Không nhập engine Zig nào vào Pip hiện tại. Hai ứng viên nổi bật giải quyết hai bài toán khác
nhau: Lightpanda là trình duyệt headless cho automation; Suzume là browser GUI tự triển khai
HTML/CSS/JavaScript còn thử nghiệm. Cả hai không thay được WebView2 trên Windows, WKWebView
trên macOS và Android WebView trong bản Pip dùng hằng ngày.

Pip đã áp dụng phần có ích và ít rủi ro hơn cho mục tiêu nhẹ: dùng engine có sẵn của hệ điều
hành thay vì đóng gói Chromium; tạo WebView theo nhu cầu và treo/giải phóng tab nền theo thời
gian nhàn rỗi hoặc áp lực bộ nhớ. Tham khảo triển khai hiện có ở `crates/core/src/tabs.rs` và
`crates/app/src/main.rs`. Thay đổi thêm chỉ nên làm sau khi có baseline RAM/CPU và kiểm tra
không làm mất form hoặc phiên của tab.

## Ứng viên

| Repo | Điều hữu ích | Vì sao không nhúng vào Pip thường |
|---|---|---|
| [Lightpanda](https://github.com/lightpanda-io/browser) | Kiến trúc browser headless và CDP dành cho agent; có thể làm tài liệu tham khảo cho Pip Dev. | Không có binary Windows native; Android không dùng được binary Linux ARM64 do khác libc; không có GUI. README tự công bố benchmark crawl 100 trang, không phải phép đo GUI Pip. Giấy phép AGPL-3.0 cần đánh giá kỹ trước khi phân phối chung. |
| [Suzume](https://github.com/midasdf/suzume) | Một ví dụ GUI Zig nhỏ, tách lớp DOM/layout/paint và cấu hình build ReleaseSafe. | Nhắm Raspberry Pi Zero 2W/Linux X11; không cung cấp tương thích web hiện đại như WebKit/Chromium. Bộ kiểm thử WPT chỉ là một số vùng; README ghi đây là code có AI hỗ trợ và có thể chưa hoàn hảo. Không thay thế an toàn được engine hệ thống. MIT nhưng việc dùng lại vẫn cần audit từng file/dependency. |
| [zchrome](https://github.com/shishtpal/zchrome) | CDP client Zig tối giản để nghiên cứu điều khiển Chrome. | Là client điều khiển Chrome đã cài, không phải engine nhẹ hay browser runtime đa nền tảng. |

## Những gì có thể học và áp dụng

1. Tách browser headless/automation khỏi GUI hằng ngày. Pip Dev tiếp tục là sản phẩm riêng;
   không khởi động Chromium chỉ để dùng Pip thường.
2. Đặt giới hạn tài nguyên tường minh cho tab nền, cache, queue và snapshot; tránh tạo WebView
   hoặc giữ bản sao HTML/ảnh khi chưa cần.
3. Không dùng số benchmark của repo khác làm tuyên bố cho Pip. Đo cùng tác vụ, cùng thiết bị,
   cùng phiên bản engine, gồm toàn bộ tiến trình WebView và tab renderer.
4. Không mang theo code Zig chỉ để giảm ngôn ngữ hoặc kích thước file. Phần nặng trong Pip là
   engine web và nội dung trang; đổi ngôn ngữ shell không làm engine nhẹ hơn.

## Điều kiện xem xét lại

Chỉ thử engine khác khi repo có hỗ trợ native đủ ba nền tảng, license phù hợp, đạt kiểm thử
WPT/bảo mật ở mức cần thiết và có bản spike so sánh RAM, CPU, pin, media, profile isolation,
DNS, download, extension/filter và tương thích website. Không thay engine phát hành trước khi
so sánh đạt trên thiết bị thật.
