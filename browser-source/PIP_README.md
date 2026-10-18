# Pip

Trình duyệt Windows 10/11 64 bit, dùng WebView2 chung của Microsoft và Brave Adblock Rust. Giao diện theo bộ nhận diện Pip, Google là công cụ tìm kiếm mặc định.

Chạy bộ cài `PipSetup.exe` bằng tài khoản thường. Bộ cài tạo lối tắt và mục gỡ cài đặt cho tài khoản hiện tại. Nếu thiếu WebView2, Pip hỏi trước khi chạy bootstrapper Microsoft đi kèm; bước đó cần Internet. Không cần cài Rust, Node hoặc mã nguồn để dùng Pip.

- Trang tab mới dùng mascot làm nền, có ô tìm kiếm lớn ở giữa và mục **Lần đầu dùng Pip?**. Trong Cài đặt có hướng dẫn về chức năng, phím tắt, RAM và dữ liệu.
- **Cài đặt → Bộ nhớ và dữ liệu → Xóa bộ nhớ đệm** dọn cache đĩa của hồ sơ hiện tại, giữ đăng nhập, lịch sử, dấu trang và tệp tải xuống. Trong **Hồ sơ và riêng tư**, bạn có thể xóa cookie cùng dữ liệu website để đăng xuất khỏi các trang (các tab có thể cần tải lại); dấu trang, lịch sử, mật khẩu và tệp tải xuống được giữ lại.
- Nút **G DNS** trên thanh công cụ bật/tắt Google DNS bảo mật cho kết nối web của Pip. Khi nút hiện **Áp dụng**, bấm nút đó hoặc mở **Cài đặt → Kết nối → Khởi động lại Pip**; Pip lưu tab thường và khôi phục chúng, còn cửa sổ ẩn danh sẽ đóng. **Kiểm tra tuyến đang dùng** xác nhận DNS-over-HTTPS tới Google và tải một trang qua proxy nội bộ. Khi bật, Pip dùng proxy loopback riêng; Windows DNS không bị đổi. DNS không thay thế VPN, không ẩn địa chỉ IP và không đảm bảo mở được mọi loại chặn mạng. Mạng chặn kết nối trực tiếp hoặc yêu cầu proxy công ty có thể không tương thích.
- Nút **khiên** bật/tắt chặn quảng cáo cho hostname hiện tại; phím tắt `Ctrl+Shift+A`. Bấm F5 khi muốn áp dụng thay đổi cho trang đang mở. Pip giữ nội dung đang nhập, không tự tải lại trang.
- Brave xử lý yêu cầu mạng; EasyList, uBlock Origin và danh sách popup HaGeZi cung cấp bộ lọc quảng cáo. Khi bộ lọc đang bật, Pip chặn cửa sổ tự bật và đối chiếu đích popup với danh sách quảng cáo, popup và cá cược HaGeZi. Danh sách cá cược cũng nhận diện tên miền phụ. Quy tắc cá cược chỉ áp dụng cho popup, không chặn địa chỉ người dùng mở trong tab hiện tại hoặc nhập vào thanh địa chỉ. Scriptlet YouTube từ uBlock/Brave chạy trước script trang để lọc cấu hình và phản hồi trình phát. Bộ lọc được đóng gói sẵn, không tải mã thực thi từ Internet khi duyệt web.
- Nút **ngôi sao** lưu/bỏ dấu trang. Dấu trang và lịch sử có màn hình xem, tìm theo tên hoặc địa chỉ; có thể xóa từng mục hoặc xóa toàn bộ lịch sử sau khi xác nhận. Dấu trang nhập/xuất theo HTML chuẩn trình duyệt. Dữ liệu lưu riêng theo hồ sơ. Lịch sử tối đa 500 trang gần nhất; dấu trang tối đa 200 mục.
- Hồ sơ trong Cài đặt tách cookie, đăng nhập và dữ liệu website. Tùy chọn **Đồng bộ dấu trang có mã hóa** tạo mã khôi phục để dùng trên thiết bị khác; dấu trang và mục đã xóa được mã hóa bằng AES-256-GCM trước khi gửi tới Cloudflare. Khóa và token truy cập không được gửi; bản lưu cục bộ được bảo vệ bằng Windows DPAPI. Lịch sử, mật khẩu, cookie, tab và phiên duyệt không đồng bộ. Hãy giữ mã khôi phục riêng tư; người có mã có thể đọc và cập nhật dấu trang đã đồng bộ. Nút **Tắt và xóa** xóa gói mã hóa trên máy chủ và bỏ mã ở hồ sơ hiện tại; các thiết bị khác dùng mã cũ không thể tạo lại kho đã xóa.
- `Ctrl+Shift+N` mở ẩn danh. Pip không ghi lịch sử/phiên tab của cửa sổ ẩn danh. Đóng toàn bộ cửa sổ ẩn danh của hồ sơ để WebView2 dọn dữ liệu phiên; tệp tải xuống và dấu trang bạn chủ động lưu vẫn được giữ.
- Danh sách tải xuống chỉ hiện trong cửa sổ đã tải tệp, có Hủy, Mở tệp và Mở thư mục. Nếu lưu tự động, Pip chọn tên khác khi tệp cùng tên đã tồn tại. Danh sách này thuộc phiên hiện tại.
- Nhấp phải tab để tạo nhóm; nhập lại cùng tên cho tab khác để thêm vào nhóm, đổi tên cho tab hiện tại để chuyển nhóm, hoặc xóa trắng để bỏ nhóm. Nhóm được lưu cùng phiên tab của cửa sổ. Menu có ghim, nhân đôi, tắt tiếng tab, mở lại tab đã đóng và khôi phục phiên. Mật khẩu/tự điền dùng tính năng gốc WebView2; tắt trong ẩn danh. Chưa có màn hình xem/sửa từng mật khẩu.
- Menu Pip → **Dịch trang sang tiếng Việt** dùng Translator API tích hợp trong Edge WebView2 để dịch văn bản đang hiển thị ngay trên thiết bị. Lần đầu có thể cần tải mô hình ngôn ngữ; dữ liệu trang không được gửi tới dịch vụ dịch bên ngoài. Nhấn lại để khôi phục văn bản gốc. Tính năng cần runtime có Translator API và mô hình hỗ trợ cặp ngôn ngữ; không dịch chữ trong ảnh, mã nguồn hay ô nhập liệu.
- Giao diện Pip tắt hoạt ảnh và chuyển động. Không có nền động, font mạng, dịch vụ proxy hoặc tiến trình chặn quảng cáo riêng. Tab ẩn dùng mức bộ nhớ thấp của WebView2; tab khôi phục chỉ tải khi chọn. Pip giữ các trang đang nhập liệu hoặc phát media.

Giới hạn: quảng cáo YouTube thay đổi theo tài khoản, khu vực và thời điểm; không cam kết chặn tuyệt đối mọi quảng cáo hoặc quảng cáo ghép vào nội dung video. Tên miền mới chưa có trong snapshot cá cược và không có dấu hiệu nhận diện rõ vẫn phụ thuộc vào lần cập nhật bộ lọc tiếp theo. Scriptlet hiện dành cho tài liệu YouTube chính; chưa có bộ thực thi procedural cosmetic, redirect resource hay chặn đầy đủ service/shared worker. Bộ lọc cần cập nhật bằng bản Pip mới.

WebView2 do Microsoft quản lý và cập nhật. Khi Google DNS bật, Pip cấu hình proxy loopback cho các WebView2 của cửa sổ và nội dung; DNS tên miền đích được hỏi qua HTTPS tới `dns.google`, với kết nối bootstrap tới IP công khai `8.8.8.8` hoặc `8.8.4.4`. Nếu proxy không khởi động được, Pip báo trong Cài đặt và quay về DNS Windows. Bản Pip này chưa ký số.

Dữ liệu portable nằm trong `user_data` cạnh exe. Bản cài dùng `%APPDATA%\evergreen-browser` kế thừa từ nền Evergreen; trình gỡ chỉ xóa dữ liệu khi bạn chọn tùy chọn đó. Không chia sẻ thư mục dữ liệu cá nhân khi phát hành.

Mã nguồn, giấy phép và nguồn scriptlet có trong `Pip-source.zip` tại thư mục cài đặt. Xem `THIRD_PARTY.md` để biết nguồn và giấy phép.

Build từ source: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1`. Kiểm tra dành cho người phát triển:

```
cargo test -p evergreen-browser -p evergreen-core -p evergreen-installer --locked
cargo run -p evergreen-browser -- --verify-native-ui
cargo run -p evergreen-browser --example adblock_smoke --locked
python tools/check_adblock_binary.py dist-pip/Pip.exe
```

Các bài kiểm tra dùng hồ sơ tạm, cửa sổ ẩn và không mở cổng debug. `--verify-native-ui` chỉ có trong bản debug. Thử nghiệm YouTube thực tế đã xác nhận phát nội dung ở hai video, chưa quan sát quảng cáo trong đoạn mở đầu đã kiểm tra; đây không phải bảo đảm cho mọi video tương lai.
