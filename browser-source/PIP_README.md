# Pip

Trình duyệt Windows 10/11 64 bit, dùng WebView2 chung của Microsoft và Brave Adblock Rust. Giao diện theo bộ nhận diện Pip, Google là công cụ tìm kiếm mặc định.

Chạy bộ cài `PipSetup.exe` bằng tài khoản thường. Bộ cài tạo lối tắt và mục gỡ cài đặt cho tài khoản hiện tại. Nếu thiếu WebView2, Pip hỏi trước khi chạy bootstrapper Microsoft đi kèm; bước đó cần Internet. Không cần cài Rust, Node hoặc mã nguồn để dùng Pip.

- Trang tab mới dùng mascot làm nền, có ô tìm kiếm lớn ở giữa và mục **Lần đầu dùng Pip?**. Trong Cài đặt có hướng dẫn về chức năng, phím tắt, RAM và dữ liệu.
- **Cài đặt → Bộ nhớ và dữ liệu → Xóa bộ nhớ đệm** dọn cache đĩa của hồ sơ hiện tại, giữ đăng nhập, lịch sử, dấu trang và tệp tải xuống.
- Nút **G DNS** trên thanh công cụ đổi Google DNS bảo mật. Viền nét đứt báo lựa chọn cần mở lại Pip để áp dụng. DNS không thay thế VPN hoặc mở được mọi loại chặn mạng.
- Nút **khiên** bật/tắt chặn quảng cáo cho hostname hiện tại; phím tắt `Ctrl+Shift+A`. Bấm F5 khi muốn áp dụng thay đổi cho trang đang mở. Pip giữ nội dung đang nhập, không tự tải lại trang.
- Brave xử lý yêu cầu mạng; EasyList, uBlock Origin và danh sách popup HaGeZi cung cấp bộ lọc quảng cáo. Khi bộ lọc đang bật, Pip chặn cửa sổ tự bật và đối chiếu đích popup với danh sách quảng cáo, popup và cá cược HaGeZi. Danh sách cá cược cũng nhận diện tên miền phụ. Quy tắc cá cược chỉ áp dụng cho popup, không chặn địa chỉ người dùng mở trong tab hiện tại hoặc nhập vào thanh địa chỉ. Scriptlet YouTube từ uBlock/Brave chạy trước script trang để lọc cấu hình và phản hồi trình phát. Bộ lọc được đóng gói sẵn, không tải mã thực thi từ Internet khi duyệt web.
- Nút **ngôi sao** lưu/bỏ dấu trang. Dấu trang và lịch sử có màn hình xem, xóa; lưu riêng theo hồ sơ. Lịch sử tối đa 500 trang gần nhất; dấu trang tối đa 200 mục.
- Hồ sơ trong Cài đặt tách cookie, đăng nhập và dữ liệu website. Đây là hồ sơ trên thiết bị, chưa có đồng bộ đám mây.
- `Ctrl+Shift+N` mở ẩn danh. Pip không ghi lịch sử/phiên tab của cửa sổ ẩn danh. Đóng toàn bộ cửa sổ ẩn danh của hồ sơ để WebView2 dọn dữ liệu phiên; tệp tải xuống và dấu trang bạn chủ động lưu vẫn được giữ.
- Danh sách tải xuống chỉ hiện trong cửa sổ đã tải tệp, có Hủy, Mở tệp và Mở thư mục. Nếu lưu tự động, Pip chọn tên khác khi tệp cùng tên đã tồn tại. Danh sách này thuộc phiên hiện tại.
- Menu có ghim, nhân đôi, tắt tiếng tab, mở lại tab đã đóng và khôi phục phiên. Mật khẩu/tự điền dùng tính năng gốc WebView2; tắt trong ẩn danh. Chưa có màn hình xem/sửa từng mật khẩu.
- Giao diện Pip tắt hoạt ảnh và chuyển động. Không có nền động, font mạng, dịch vụ proxy hoặc tiến trình chặn quảng cáo riêng. Tab ẩn dùng mức bộ nhớ thấp của WebView2; tab khôi phục chỉ tải khi chọn. Pip giữ các trang đang nhập liệu hoặc phát media.

Giới hạn: quảng cáo YouTube thay đổi theo tài khoản, khu vực và thời điểm; không cam kết chặn tuyệt đối mọi quảng cáo hoặc quảng cáo ghép vào nội dung video. Tên miền mới chưa có trong snapshot cá cược và không có dấu hiệu nhận diện rõ vẫn phụ thuộc vào lần cập nhật bộ lọc tiếp theo. Scriptlet hiện dành cho tài liệu YouTube chính; chưa có bộ thực thi procedural cosmetic, redirect resource hay chặn đầy đủ service/shared worker. Bộ lọc cần cập nhật bằng bản Pip mới.

WebView2 do Microsoft quản lý và cập nhật. Google DNS đã được xác minh trên WebView2 `155.0.4283.45`; cấu hình phụ thuộc định dạng Local State của Chromium và cần kiểm tra lại khi runtime thay đổi. Bản Pip này chưa ký số.

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
