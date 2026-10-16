# Pip 0.1.1 — rà soát Ponytail ngày 16/10/2026

## Kết quả và phạm vi

Rà luồng đang chạy trong `crates/app/src`, `crates/core/src`, HTML/CSS/JavaScript của tab mới, thanh công cụ, menu và cài đặt; kiểm tra thêm đóng gói, bộ cài và các bài kiểm tra. Pip tiếp tục dùng Evergreen + WebView2 + Brave Adblock Rust hiện có. Không thêm dependency cho ứng dụng hay dịch vụ nền.

Đây là rà soát mã nguồn và kiểm tra các hành vi bên dưới, không phải khẳng định mọi website, mọi phiên bản WebView2 hoặc mọi dòng mã bên thứ ba đều không có lỗi. Mã vendored và engine được giữ theo dependency có sẵn.

| Phần | Vấn đề tìm thấy | Cách xử lý / bằng chứng cần đọc |
|---|---|---|
| Tab mới | Banner thấp phóng/cắt mạnh mascot; ô tìm kiếm nhỏ, lệch trái | Nền toàn vùng nội dung; tìm kiếm cao 76 px, giữa trang; bản hẹp 66 px. `ui-review/qa/home-desktop.png`, `home-mobile.png`, `home-dark.png` |
| Hướng dẫn | Người dùng phải tìm README để hiểu chức năng | Hướng dẫn nhanh ngay tab mới; các phần giải thích trong Cài đặt; mô tả trên menu và thanh công cụ; disclosure mở được bằng bàn phím |
| Ảnh | Viền mascot bị răng cưa; favicon lỗi biến mất; icon GitHub dùng ký tự không đúng | Làm sạch mascot có alpha; favicon dự phòng SVG và giữ đúng HTTP/HTTPS/cổng của website; SVG GitHub. Không dùng font/icon từ mạng |
| Cài đặt | Ba lựa chọn xem trước liên kết chồng nhau; mã whitelist cũ không có giao diện gọi | Một lựa chọn điều khiển cả cấu hình thực tế; xóa JavaScript và chuỗi HTML whitelist không còn dùng; giữ cấu hình cũ để tương thích |
| Quản lý tab | Chuyển/mở tab gửi lệnh ẩn và giảm bộ nhớ cho mọi tab còn lại | Chỉ ẩn tab trước, hiện tab được chọn. Tab cũ giữ WebView, form và JavaScript; tạo tab lỗi khôi phục tab trước |
| Nội dung nội bộ | Mỗi tab website giữ sẵn bản HTML/ảnh của trang chủ, 404 và loading | Tạo đúng trang khi protocol được yêu cầu; không giữ các mảng HTML/ảnh dựng sẵn trong mỗi tab. Lịch sử/dấu trang dùng chung một mascot |
| Phiên đã lưu | Cần xác minh tải lười hoạt động thật | Kiểm tra native xác nhận chỉ một WebView khi khôi phục, thêm đúng một khi chọn tab chưa tải |
| Menu / tải xuống | Cập nhật menu ẩn; ảnh base64 lặp trong từng thẻ tải và dựng lại theo mỗi sự kiện | Không gửi snapshot/menu UI khi đóng; chỉ render danh sách khi đang xem Tải xuống; một mascot ở đầu danh sách |
| Vòng đời tải xuống | Giữ handle COM của các tệp hoàn tất sau khi đóng cửa sổ | Dọn handle hoàn tất và giải quyết prompt đang chờ khi đóng; chuyển đang chạy được giữ tới sự kiện kết thúc |
| Dung lượng đĩa | Chưa có thao tác dọn cache dễ hiểu | Cài đặt → Bộ nhớ và dữ liệu → Xóa bộ nhớ đệm, dùng API DISK_CACHE của hồ sơ hiện tại; callback native và cookie/localStorage được kiểm tra |
| Thanh tab / truy cập | ARIA tham chiếu panel không nằm trong WebView; điều khiển tương tác lồng nhau | Bỏ tham chiếu sai; có hướng dẫn bàn phím và Delete đóng tab. Kiểm tra axe-core cho thanh tab / hồ sơ |
| Tìm trong trang | Mục trong menu chỉ mở khoảng trống native, chưa mở ô nhập | Cùng luồng mở và focus ô tìm. Ctrl+J tôn trọng lựa chọn tắt quản lý tải xuống |
| Tải lại | `location.reload(true)` không bảo đảm bỏ cache trên Chromium | Gọi native `Page.reload` với `ignoreCache:true`; không mở cửa sổ DevTools; kiểm tra tài liệu được tải lại |
| Lịch sử / callback | Đồng bộ giao diện dù URL/tiêu đề/trạng thái không đổi; lịch sử phụ thuộc đổi tiêu đề | Bỏ đồng bộ không đổi, dùng URL do WebView báo, ghi cả chuyển trang có cùng tiêu đề; có kiểm tra native riêng |
| Lưu dữ liệu | Cài đặt và phiên tab ghi đè trực tiếp | Ghi tệp tạm cùng thư mục rồi thay thế; giữ cách lưu thư viện hiện có |
| Thông tin trang | Cache theo hostname có thể lấy trạng thái HTTPS cho lần xem HTTP | Cache theo origin; kiểm tra HTTP và HTTPS cùng hostname/cổng |
| Mã thừa | Registry trong app chỉ đăng ký sáu bộ metadata, không cung cấp script thực thi | Bỏ registry khỏi đường chạy app. API plugin và adapter engine cũ còn trong source cho tương thích; adapter không được app liên kết nên không làm nặng binary |
| Đóng gói / báo cáo | Gói source thiếu `.py`; renderer tự ghi trạng thái pass dù chưa chạy kiểm tra | Đóng gói cả công cụ Python; renderer chỉ báo đã tạo HTML, kết quả kiểm chứng nằm trong báo cáo chạy thật |

## Nhiều tab, RAM và dữ liệu

Phương án đang dùng: một WebView2 Runtime chung, tab ẩn yêu cầu mức LOW, khôi phục phiên tải theo nhu cầu, giảm sao chép ảnh/HTML và các lần giao tiếp không cần thiết với WebView. Không có cơ chế tự hủy trang đang nhập liệu/cuộc gọi. Ghim tab chỉ giữ vị trí, không làm tab nhẹ hơn.

RAM của engine và website vẫn lớn hơn phần Rust host. Dữ liệu đĩa gồm cache, cookie, lưu trữ website và tệp tải; cache giúp giảm tải Internet nên không tự xóa liên tục. Người dùng có thể chủ động dọn cache. Hồ sơ tách tài khoản cũng lưu các bộ dữ liệu riêng; không có lời hứa nhiều tab dùng bằng không RAM hoặc dung lượng.

Cơ sở API: [Microsoft — MemoryUsageTargetLevel](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19). LOW là yêu cầu cho runtime, không phải giới hạn RAM cứng. Không thay sandbox, không tắt cách ly website.

Kịch bản đo: hai executable trước/sau, mỗi executable ba lần chạy với hồ sơ portable mới, đổi thứ tự chạy xen kẽ, 12 trang tĩnh giống nhau trên loopback; cửa sổ ẩn, CDP chỉ mở ở loopback trong công cụ đo. Mỗi lần chạy lấy ba mẫu; bảng dưới là trung vị giữa ba lần chạy. Báo cáo gồm hash executable ở `target/tab-benchmark.json`. Đây không phải benchmark YouTube, thiết bị 4 GB hoặc phiên sử dụng dài. Tổng working set có thể đếm lặp bộ nhớ dùng chung; RAM riêng là số cần đọc chính.

| Số đo (MiB, 12 trang tĩnh) | Trước | Pip 0.1.1 |
|---|---:|---:|
| RAM riêng của Rust host | 18,08 | 9,58 |
| RAM riêng cả nhóm Pip + WebView2 | 662,10 | 639,02 |
| Khoảng RAM riêng cả nhóm qua các lần chạy | 658,80–666,23 | 631,89–653,04 |
| Tổng working set (có thể đếm lặp trang dùng chung) | 589,46 | 578,90 |
| Số tiến trình | 19 | 19 |

Mức giảm trung vị cả nhóm khoảng 3,5% trong bài thử này; phần Rust giảm khoảng 8,5 MiB. Không dùng mức giảm của riêng Rust để quảng cáo thành mức giảm của cả trình duyệt. Không có số đo CPU hoặc tuyên bố nhẹ hơn Chrome/Edge.

## Xác minh

Các đường dẫn bằng chứng dưới đây thuộc workspace kiểm tra. Gói source không chứa hồ sơ thử và log runtime. Các lệnh và kết quả cuối cùng được ghi trong `docs/ponytail-verification.json` sau khi chạy. Bằng chứng chi tiết:

- `ui-review/qa/report.json`: tương tác, responsive, giao diện sáng/tối, hướng dẫn, fallback ảnh và axe-core.
- `target/native-ui-verification.json`: ứng dụng Windows thật với mọi cửa sổ ẩn; hồ sơ/ẩn danh, tải xuống, cache, 12 tab, form chưa gửi, khôi phục tải lười, tìm kiếm giữa nền mascot.
- `target/adblock-binary-verification.json`: yêu cầu quảng cáo bị chặn trước khi đến máy chủ thử; nội dung thường hoạt động.
- `target/google-dns-verification.json`: kiểm tra DNS của executable; xem tên tệp do công cụ kiểm tra in ra.
- `target/tab-benchmark.json`: số đo thực tế và giới hạn diễn giải.
- `dist/build-manifest.json`, `dist/SHA256SUMS.txt`: bộ cài local mới, hash và kích thước.

Giới hạn còn lại: chặn quảng cáo YouTube thay đổi theo tài khoản/khu vực/thời điểm; chưa có bảo đảm chặn mọi quảng cáo. Các trường sleep/throttling cũ trong cấu hình chưa phải bộ tự hủy tab. Khôi phục phiên hiện giới hạn 100 tab theo cơ chế cũ. Chưa có đồng bộ đám mây hoặc hỗ trợ đầy đủ extension Chrome. Chưa có đối chứng Chrome/Edge trên cùng workload hoặc kiểm tra máy 4 GB. Các giới hạn này không được trình bày như chức năng đã hoàn thành.

## Asset mascot

Dùng công cụ Imagegen tích hợp, chế độ chỉnh ảnh, với `mascot-empty.webp` làm ảnh đích và `home-hero.webp` làm tham chiếu nhân vật. Đầu ra được mã hóa WebP giữ alpha, dung lượng 180.660 byte. Asset đưa vào ứng dụng: `crates/app/ui/assets/mascot-clean.webp` và bản base64 tương ứng. Nền tab mới tiếp tục dùng ảnh mascot/phòng làm việc hiện có, `home-hero.webp`.

Prompt thực tế:

> Edit the first image for production use as a transparent browser mascot asset. First image is the edit target: Pip the friendly orange-brown beaver sitting, wearing a blue denim jacket, holding a blank cream sheet of paper. Second image is only identity/style reference. Preserve the exact character identity, face, colors, body proportions, happy expression, sitting pose, jacket and blank paper. Repair the ugly jagged cutout edges, stray dark colored horizontal lines and compression artifacts. Produce crisp smooth antialiased natural fur contours with a genuinely transparent alpha background and no halo or background remnants. Keep entire character, ears, feet and tail visible, modest transparent padding. Refined clean 3D illustration with detailed eyes and fur, no text, no logos, no added objects. This is asset cleanup, not a new character.
