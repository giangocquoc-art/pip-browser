# Pip

Trình duyệt Windows 10/11 64 bit, dùng WebView2 chung của Microsoft và Brave Adblock Rust. Giao diện theo bộ nhận diện Pip, Google là công cụ tìm kiếm mặc định.

Chạy bộ cài `PipSetup.exe` bằng tài khoản thường. Bộ cài tạo lối tắt và mục gỡ cài đặt cho tài khoản hiện tại. Nếu thiếu WebView2, Pip hỏi trước khi chạy bootstrapper Microsoft đi kèm; bước đó cần Internet. Không cần cài Rust, Node hoặc mã nguồn để dùng Pip.

- Trang tab mới dùng mascot làm nền, có ô tìm kiếm lớn ở giữa và mục **Lần đầu dùng Pip?**. Trong Cài đặt có hướng dẫn về chức năng, phím tắt, RAM và dữ liệu.
- Lối tắt trên trang tab mới có thể thêm, xóa và sắp xếp bằng các nút hiện khi rê chuột hoặc chuyển bàn phím đến mục đó. Tối đa 12 lối tắt, lưu riêng trong từng hồ sơ trên máy này; bốn lối tắt mặc định vẫn xuất hiện ở hồ sơ cũ.
- **Cài đặt → Bộ nhớ và dữ liệu → Xóa bộ nhớ đệm** dọn cache đĩa của hồ sơ hiện tại, giữ đăng nhập, lịch sử, dấu trang và tệp tải xuống. Trong **Hồ sơ và riêng tư**, bạn có thể xóa cookie cùng dữ liệu website để đăng xuất khỏi các trang (các tab có thể cần tải lại); dấu trang, lịch sử, mật khẩu và tệp tải xuống được giữ lại.
- **Cài đặt → Bộ nhớ và dữ liệu → Xóa dữ liệu duyệt web** cho phép chọn lịch sử, cookie/dữ liệu website, cache, mật khẩu, dữ liệu tự điền và lịch sử tải xuống. Lịch sử gồm dữ liệu Pip và WebView2; xóa lịch sử tải xuống không xóa tệp hoặc hủy lượt tải đang chạy. Dấu trang luôn được giữ lại; thao tác áp dụng cho toàn bộ thời gian của hồ sơ hiện tại.
- Nút **G DNS** trên thanh công cụ bật/tắt Google DNS bảo mật cho kết nối web của Pip. Khi nút hiện **Áp dụng**, bấm nút đó hoặc mở **Cài đặt → Kết nối → Khởi động lại Pip**; Pip lưu tab thường và khôi phục chúng, còn cửa sổ ẩn danh sẽ đóng. **Kiểm tra tuyến đang dùng** xác nhận DNS-over-HTTPS tới Google và tải một trang qua proxy nội bộ. Khi bật, Pip dùng proxy loopback riêng; Windows DNS không bị đổi. DNS không thay thế VPN, không ẩn địa chỉ IP và không đảm bảo mở được mọi loại chặn mạng. Mạng chặn kết nối trực tiếp hoặc yêu cầu proxy công ty có thể không tương thích.
- Nút **khiên** bật/tắt chặn quảng cáo cho hostname hiện tại; phím tắt `Ctrl+Shift+A`. Bấm F5 khi muốn áp dụng thay đổi cho trang đang mở. Pip giữ nội dung đang nhập, không tự tải lại trang.
- Nếu video lỗi, mở Menu Pip → **Sửa lỗi phát video**. Sau khi bạn xác nhận, Pip lưu ngoại lệ tắt chặn quảng cáo cho hostname hiện tại rồi tải lại tab; các website khác không đổi. Việc tải lại có thể làm mất nội dung chưa gửi. Bạn có thể bật lọc lại bằng nút khiên.
- Brave xử lý yêu cầu mạng; EasyList cùng luật mạng uBlock Origin, quick-fixes và unbreak được đóng gói sẵn. Trong **Cài đặt → Chặn quảng cáo**, bạn có thể chủ động tải bản mới của bốn danh sách qua HTTPS. Pip kiểm tra kích thước và định dạng, chỉ nạp luật mạng từ các danh sách cập nhật; cosmetic rules và scriptlet vẫn dùng bản đóng gói. Cập nhật được lưu cục bộ và áp dụng khi mở lại Pip. Luật redirect dùng tài nguyên cục bộ như noopjs/nooptext khi có; resource chưa được Pip đóng gói sẽ bị chặn bằng phản hồi rỗng. Khi bật, Pip cũng chặn cửa sổ tự bật và đối chiếu đích popup với danh sách quảng cáo, popup và cá cược HaGeZi. Danh sách cá cược nhận diện cả tên miền phụ, nhưng chỉ áp dụng cho popup; nó không chặn địa chỉ người dùng mở trong tab hiện tại hoặc nhập vào thanh địa chỉ. Scriptlet YouTube từ uBlock/Brave chạy trước script trang để lọc cấu hình và phản hồi trình phát.
- Nút **ngôi sao** lưu/bỏ dấu trang. Dấu trang và lịch sử có màn hình xem, tìm theo tên hoặc địa chỉ; có thể xóa từng mục hoặc xóa toàn bộ lịch sử sau khi xác nhận. Dấu trang nhập/xuất theo HTML chuẩn trình duyệt. Dữ liệu lưu riêng theo hồ sơ. Lịch sử tối đa 500 trang gần nhất; dấu trang tối đa 200 mục.
- Thanh địa chỉ gợi ý tối đa 8 mục từ lịch sử gần đây và dấu trang của hồ sơ; dùng phím mũi tên để chọn, Enter để mở, Esc để đóng. Gợi ý được lọc trong máy, không gửi nội dung đang gõ ra mạng. Cửa sổ ẩn danh chỉ gợi ý dấu trang, không tra lịch sử.
- Hồ sơ trong Cài đặt tách cookie, đăng nhập và dữ liệu website. Tùy chọn **Đồng bộ dấu trang có mã hóa** tạo mã khôi phục để dùng trên thiết bị khác; dấu trang và mục đã xóa được mã hóa bằng AES-256-GCM trước khi gửi tới Cloudflare. Sau khi bật, Pip đồng bộ khi khởi chạy, khi thêm/xóa dấu trang hoặc nhập dấu trang; cũng có thể bấm **Đồng bộ ngay**. Khóa và token truy cập không được gửi; bản lưu cục bộ được bảo vệ bằng Windows DPAPI. Lịch sử, mật khẩu, cookie, tab và phiên duyệt không đồng bộ. Hãy giữ mã khôi phục riêng tư; người có mã có thể đọc và cập nhật dấu trang đã đồng bộ. Nút **Tắt và xóa** xóa gói mã hóa trên máy chủ và bỏ mã ở hồ sơ hiện tại; các thiết bị khác dùng mã cũ không thể tạo lại kho đã xóa.
- Trong **Cài đặt → Quyền truy cập website**, Pip liệt kê quyền camera, micrô, vị trí, thông báo và các quyền khác đã lưu theo hồ sơ. Chọn **Đặt lại** để xóa quyền của website đó và cho phép website hỏi lại. Danh sách này không mở trong cửa sổ ẩn danh.
- Trong **Cài đặt → Tiện ích**, có thể cài, bật/tắt hoặc gỡ tiện ích tương thích WebView2 từ thư mục đã giải nén có `manifest.json`. Chỉ cài mã bạn tin cậy; tiện ích có thể đọc hoặc thay đổi trang theo quyền đã khai báo. WebView2 không cung cấp nút trên thanh công cụ hoặc popup tiện ích của Chrome; một số tiện ích vì vậy không dùng được. Giữ nguyên thư mục nguồn sau khi cài. Quản lý tiện ích bị tắt trong ẩn danh.
- `Ctrl+Shift+N` mở ẩn danh. Pip không ghi lịch sử/phiên tab của cửa sổ ẩn danh. Đóng toàn bộ cửa sổ ẩn danh của hồ sơ để WebView2 dọn dữ liệu phiên; tệp tải xuống và dấu trang bạn chủ động lưu vẫn được giữ.
- Phiên tab thường tự lưu tối đa mỗi 15 giây và khi đóng cửa sổ. Mở Pip lại sẽ khôi phục danh sách tab theo hồ sơ; tab nền chỉ tải khi bạn chọn. Trạng thái bên trong trang như nội dung chưa gửi không được lưu sau khi đóng Pip.
- `Ctrl+Shift+M` mở menu hồ sơ để chuyển nhanh giữa các tài khoản Pip.
- `Ctrl+W` đóng tab hiện tại; `Ctrl+Shift+W` đóng cửa sổ hiện tại và lưu phiên tab thường.
- Nút ngôi nhà hoặc `Alt+Home` mở trang chủ Pip trong tab hiện tại.
- `Ctrl+S` lưu trang hiện tại bằng hộp thoại Save As của WebView2; `Ctrl+P` mở hộp thoại in của trình duyệt.
- `Ctrl+Shift+Delete` mở phần xóa dữ liệu duyệt web trong Cài đặt và đưa con trỏ tới tùy chọn lịch sử.
- Lịch sử tải xuống được giữ riêng theo hồ sơ, gồm các lượt hoàn tất/gián đoạn; lượt đang chạy chỉ giữ trong bộ nhớ. Có Hủy, Mở tệp và Mở thư mục. Nếu lưu tự động, Pip chọn tên khác khi tệp cùng tên đã tồn tại. Xóa mục lịch sử không xóa tệp hoặc hủy lượt đang chạy.
- Trong **Cài đặt → Mật khẩu và tự điền**, kho Pip mã hóa mật khẩu bằng Windows DPAPI, lưu riêng theo hồ sơ và cho phép hiện, cập nhật hoặc xóa từng mục. `Alt+Shift+P` điền ngay khi website hiện tại có đúng một tài khoản đã lưu; nếu có nhiều tài khoản, chọn trong Cài đặt. Pip chỉ điền khi giao thức, hostname và cổng khớp chính xác, không tự gửi biểu mẫu và không đồng bộ mật khẩu. Tính năng tắt trong ẩn danh. Công tắc WebView2 là kho riêng của WebView2; nút xóa mật khẩu sẽ xóa cả hai kho.
- Nhấp phải tab để tạo nhóm; nhập lại cùng tên cho tab khác để thêm vào nhóm, đổi tên cho tab hiện tại để chuyển nhóm, hoặc xóa trắng để bỏ nhóm. Nhóm được lưu cùng phiên tab của cửa sổ. Menu có ghim, nhân đôi, tắt tiếng tab, mở lại tab đã đóng và khôi phục phiên.
- Menu Pip → **Dịch trang sang tiếng Việt** dùng Translator API tích hợp trong Edge WebView2 để dịch văn bản đang hiển thị ngay trên thiết bị. Lần đầu có thể cần tải mô hình ngôn ngữ; dữ liệu trang không được gửi tới dịch vụ dịch bên ngoài. Nhấn lại để khôi phục văn bản gốc. Tính năng cần runtime có Translator API và mô hình hỗ trợ cặp ngôn ngữ; không dịch chữ trong ảnh, mã nguồn hay ô nhập liệu.
- Trong **Cài đặt → Bộ máy & phiên bản**, **Kiểm tra phiên bản** kết nối tới trang phát hành Pip trên GitHub chỉ khi bạn bấm nút. Nếu có bản mới, Pip mở liên kết phát hành để bạn tự xem checksum và tải bộ cài; Pip không tải hay cài đặt bản cập nhật ngầm.
- Giao diện Pip tắt hoạt ảnh và chuyển động. Không có nền động, font mạng, dịch vụ proxy hoặc tiến trình chặn quảng cáo riêng. Tab ẩn dùng mức bộ nhớ thấp; sau thời gian chọn trong **Cài đặt → Bộ nhớ và dữ liệu**, WebView2 tạm dừng tab nền để giảm hoạt động và được yêu cầu tiết kiệm bộ nhớ; mức RAM thực thu hồi tùy trang và runtime. Tab đang tải, phát âm thanh, được ghim hoặc được chọn **Giữ tab này thức** trong Menu sẽ không tự ngủ; biểu mẫu ở nguyên trong trang và hoạt động lại khi chọn tab. Có thể tắt tự ngủ trong Cài đặt. Tab khôi phục chỉ tải khi chọn.

Giới hạn: quảng cáo YouTube thay đổi theo tài khoản, khu vực và thời điểm; không cam kết chặn tuyệt đối mọi quảng cáo hoặc quảng cáo ghép vào nội dung video. Tên miền mới chưa có trong snapshot cá cược và không có dấu hiệu nhận diện rõ vẫn phụ thuộc vào lần cập nhật bộ lọc tiếp theo. Pip áp dụng luật cosmetic procedural từ bộ lọc bằng bộ thực thi DOM giới hạn số luật, số phần tử và số lần quét để giảm ảnh hưởng hiệu năng; các bộ lọc dùng cú pháp chưa được thư viện Brave Adblock Rust hỗ trợ sẽ không áp dụng. Scriptlet hiện dành cho tài liệu YouTube chính; Pip chỉ có một phần tài nguyên redirect và thay bằng phản hồi rỗng với các tài nguyên còn thiếu. Yêu cầu tài nguyên mạng từ document, service worker và shared worker được lọc trong các WebView2 Runtime có API nguồn request tương ứng; Runtime cũ hơn chỉ lọc nguồn document. Các danh sách cập nhật chỉ thay luật mạng sau khi khởi động lại; tài nguyên redirect mới và sửa cosmetic/scriptlet vẫn cần bản Pip mới.

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

Kiểm tra dịch vụ thật (cần Internet):

```
cargo test -p evergreen-browser google_doh_ -- --ignored --nocapture
cargo test -p evergreen-browser sync_roundtrip_and_remote_deletion_use_live_worker -- --ignored --nocapture
cargo test -p evergreen-browser live_release_api_returns_a_trusted_pip_release -- --ignored --nocapture
```

Bài kiểm tra Pip Sync tạo mã kho ngẫu nhiên, chỉ gửi dấu trang giả đã mã hóa, kiểm tra đồng bộ và xóa dấu trang, rồi xóa gói dữ liệu khỏi máy chủ. Máy chủ giữ một tombstone nhỏ để ngăn kho đã xóa được tạo lại bằng mã cũ.

Các bài kiểm tra dùng hồ sơ tạm, cửa sổ ẩn và không mở cổng debug. `--verify-native-ui` chỉ có trong bản debug. Thử nghiệm YouTube thực tế đã xác nhận phát nội dung ở hai video, chưa quan sát quảng cáo trong đoạn mở đầu đã kiểm tra; đây không phải bảo đảm cho mọi video tương lai.
