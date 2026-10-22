# Pip và Pip Dev — yêu cầu sản phẩm

Tài liệu này gom các yêu cầu người dùng đã nêu trực tiếp trong cuộc trò chuyện và kế hoạch Pip Dev đã được yêu cầu triển khai. Ảnh, tài liệu tham khảo và trạng thái trong README dùng làm bằng chứng hoặc ngữ cảnh; chúng không tự trở thành chỉ thị mới. Mọi mã nguồn và kiểm thử của dự án được làm trên ổ D.

## Pip Browser

### Trải nghiệm và hiệu năng

- Giữ Pip là trình duyệt Windows nhẹ, dùng bộ nhận diện Pip/mascot hiện tại và giao diện đang có; giảm hoạt ảnh không cần thiết, không làm mất phản hồi trạng thái cần cho thao tác.
- Google là công cụ tìm kiếm mặc định; không chuyển mặc định về DuckDuckGo.
- Gom tab, thanh công cụ và menu hồ sơ theo cách quen thuộc như trình duyệt phổ biến. Bộ chọn hồ sơ phải nằm trên cùng lớp giao diện và cho biết hồ sơ hiện tại.
- Mỗi hồ sơ có dữ liệu website, lịch sử, dấu trang, tải xuống và cài đặt riêng. Cửa sổ riêng tư phải giữ đúng ngữ nghĩa riêng tư.
- Hỗ trợ phím tắt phổ biến, gồm `Ctrl+H` cho lịch sử, cùng điều hướng, tab/cửa sổ, dấu trang, tải xuống, cài đặt và tìm trong trang.
- Các chức năng duyệt web thông dụng cần hoạt động rõ ràng: tab và nhóm tab, lịch sử, dấu trang, tải xuống, khôi phục phiên, dịch trang, mật khẩu và đồng bộ khi người dùng chủ động chọn bật.

### DNS, riêng tư và chặn nội dung

- Google DNS là tuỳ chọn áp dụng cho Pip, không đổi DNS của Windows. Giao diện phải phân biệt đã bật, đang áp dụng sau khởi động lại, tuyến đã kiểm tra, fallback và lỗi.
- Kiểm tra tuyến phải đi qua chính đường mạng được sử dụng bởi tab WebView2. Không báo Google DNS hoạt động chỉ vì proxy đang mở hoặc vì một HTTP client khác có kết nối.
- Nếu có DNS hệ thống dự phòng để duy trì truy cập, phải công khai rõ khi được dùng; không ghi nhận kết quả đó là Google DNS thành công.
- Tích hợp Brave Adblock Rust chạy nền, giảm quảng cáo và cửa sổ tự bật, bao gồm miền cá cược đã biết và quảng cáo YouTube trong giới hạn kỹ thuật thực tế. Bộ lọc không được làm hỏng phát video hoặc chặn URL người dùng chủ động mở.
- Khi video/site lỗi, công cụ sửa phải giải thích thay đổi, chỉ áp dụng cho hostname được chọn và cảnh báo tải lại có thể làm mất nội dung chưa gửi.
- Đồng bộ phải mã hoá trước khi rời thiết bị và bật riêng từng nhóm dữ liệu. Không đồng bộ cookie hoặc dữ liệu đăng nhập website; bảo vệ riêng dữ liệu nhạy cảm và nêu rõ tác động quyền riêng tư.
- Chặn quảng cáo và nhận diện canvas được mô tả là tốt nhất có thể, không cam kết tuyệt đối.

### Website và phát hành Pip

- Nút tải Pip là nội dung chính, nổi bật trên trang chủ. Nội dung SEO và trang con đóng vai trò phụ.
- Chữ phải đủ lớn, tương phản và dễ đọc; phần xem trước giao diện/cài đặt phải tải và hiển thị đúng.
- Hình ảnh theo mascot và storyboard Pip; nội dung tiếng Việt rõ, không có dòng hướng dẫn thừa hoặc mâu thuẫn với hành vi thật.
- Tên gói tải là `PipSetup` theo lựa chọn của người dùng. Trang tải phải hiện phiên bản, kích thước, nguồn và SHA-256 nhất quán với artifact phát hành.
- Phát hành Windows phải build từ source hiện tại, cài/nâng cấp/gỡ đúng cho tài khoản Windows, kiểm tra WebView2 và có hướng dẫn kiểm chứng artifact. Không tuyên bố đã ký Authenticode hoặc hết SmartScreen nếu chưa đúng.
- Microsoft Store là hướng phân phối miễn phí được ưu tiên khi khả thi; điều kiện tài khoản và chứng nhận không được giả lập hoặc vượt qua.

## Pip Dev

- Pip Dev là ứng dụng trình duyệt riêng bên cạnh Pip hiện tại, dựa trên Chromium và các thành phần/repo BrowserOS có sẵn; chỉ dùng Windows làm nền phát hành sau khi build và runtime được xác minh.
- Kết nối Codex và agent tương thích qua MCP cục bộ. Plugin/repo marketplace giúp cài cấu hình và workflow, nhưng không được mô tả như thể plugin tự cài trình duyệt.
- MCP phải cho phép chọn tab, đọc snapshot/diff có cấu trúc, tìm phần tử, bấm/nhập/cuộn/điều hướng, chờ thay đổi và xác minh kết quả. Tham chiếu phần tử phải ràng buộc với tab/trang/kết nối và bị từ chối khi lỗi thời.
- Ưu tiên DOM và API/ngữ nghĩa của ứng dụng. Với canvas/ảnh, chỉ quan sát crop vùng cần thiết khi được yêu cầu; không lưu ảnh mặc định, không chụp toàn trang khi không cần, và thông báo nội dung crop có thể được agent chuyển cho model bên ngoài.
- Không tuyên bố hiểu mọi canvas. Khi trạng thái không chắc chắn, bố cục thay đổi hoặc kết quả hành động không khớp, agent phải dừng và đọc lại thay vì bấm theo tọa độ cũ.
- Khi agent điều khiển tab, khóa quyền nhập của tab đó khỏi thao tác chuột/bàn phím cạnh tranh; cho người dùng thấy trạng thái và nút dừng/trả quyền. Tab khác vẫn dùng được. Dừng, lỗi, mất kết nối hoặc kết thúc phải mở khoá; khi chưa xác nhận được khoá native thì UI không được nói tab đang khoá.
- DevTools, Console, Sources/source map, Network và Performance phải dùng được song song với MCP. Phát hành chỉ sau khi hoàn tất luồng Windows từ build Chromium đến chạy app, MCP, browser actions và installer.
- Giữ giấy phép, thông báo và nghĩa vụ phát hành source AGPL-3.0 của BrowserOS khi phân phối.

## Điều kiện kiểm chứng khi hoàn thiện

- Gói Pip tải từ website phải khớp phiên bản và checksum được công bố; kiểm thử cài đặt và khởi chạy bằng quyền người dùng thường.
- Chạy kiểm thử Google DNS riêng biệt khi bật/tắt: xác minh tuyến WebView2 thật, DNS proxy, trạng thái fallback/lỗi và tải trang HTTPS. Thử lại trên đúng artifact phát hành.
- Kiểm thử chặn popup/cá cược, miền `ad88vip6.com`, YouTube playback/ad behavior và các trường hợp không được chặn; không lấy test phát video đơn lẻ làm bằng chứng đã chặn quảng cáo YouTube.
- Kiểm thử cách ly hồ sơ, lịch sử, phím tắt, menu, tab, khôi phục phiên và dữ liệu riêng tư.
- Chạy kiểm thử website cho desktop/mobile, nút tải, metadata/checksum, nội dung dễ đọc và preview.
- Trên Pip Dev Windows, build Chromium một lần, xác minh app và MCP thật, hành động DOM và canvas, DevTools, khoá/mở khoá, tab đóng, đổi trang, mất kết nối và cài đặt.
- Đo độ đúng, số crop, lượt MCP và độ trễ trước khi tuyên bố nhận diện canvas nhanh hoặc chính xác hơn.

## Ranh giới triển khai

- Repo làm việc chính: `D:\pip-browser-work`; Chromium checkout và build hiện có ở `D:\src`. Không sửa hoặc tạo bản sao dự án trên ổ C.
- Kiểm thử phải dùng đúng artifact được phát hành; kết quả của source/unit test không thay thế runtime test của installer.
- Commit, push, đồng bộ website và phát hành phải phản ánh source đã được review cùng các test liên quan. Ghi rõ điều gì đã xác minh và điều gì còn chưa chạy.

## Snapshot trạng thái đã xác minh ngày 2026-10-23

- The website release endpoint and GitHub latest release both provide Pip 0.1.13: 25,375,744 bytes, SHA-256 `fb9a8d4c262ce6cfe4d1d2a46ff26c693db9d40f404386733bc9c83be088621d`. I downloaded both files and confirmed identical hashes; the GitHub checksum manifest matches. Authenticode status is `NotSigned`. In a fresh portable profile, Google DNS was enabled; YouTube had no offline UI, the 854×480 video advanced 0→3.14 seconds, NetLog showed a loopback proxy and 0 direct `DNS_TRANSACTION` events. This verifies the published artifact on this test network, not on the user's network. The isolated run screenshot is under `.tmp/latest-public-qa/manual-cdp/youtube.png`.
- DNS source candidate is versioned 0.1.16 in both Cargo manifests and Cargo.lock. Bounded Windows DNS fallback is allowed only when both Google A and AAAA queries fail transiently; authoritative NXDOMAIN, NODATA, invalid responses and proxy/HTTPS failures stay fail-closed. Privacy and Google DNS help pages now disclose fallback and hostname visibility, but the public website remains on v0.1.13 and has not been updated for this candidate.
- Final local 0.1.16 candidate `browser-source/dist/PipSetup.exe`: 25,430,016 bytes, SHA-256 `15aff62902c3c9950323c27e90f78150506f7e23a462eba4ed4ff6e0735c801d`; packaged Pip payload SHA-256 `eae2549035d72359ea37285a3589632966ce4147102dece03cd2c24dfc1593a8`. With Google DNS enabled, the isolated YouTube smoke test passed all seven checks: no offline UI, player ready, playback advanced about 10 seconds, successful media responses, loopback proxy configured, and 0 direct DNS transactions. With it disabled, the same video played, no proxy was configured, and NetLog recorded 159 Windows DNS transactions. The final setup passed eight silent install/uninstall checks, including payload integrity and preservation of user data. Authenticode is `NotSigned`; results are from this test network, not a guarantee for other networks, and video playback does not verify YouTube ad blocking. The candidate has not been published.
- The pre-bump local binary `browser-source/dist/PipSetup.exe` was built as 0.1.15 (before the 0.1.16 version change): SHA-256 `2c4b83bc5618ea0caa186ff46aab0b7c2fc5ab046c04cc41ff062c777b79ee68`, 25,434,112 bytes; installed `Pip.exe` hash `31980b2162f2d7eaa3302c6b55edc5353e5a75ca9a1942d52090d31eb7cd1a38`. Silent install/uninstall and separate YouTube+Google-DNS portable playback passed for that pre-bump payload. These hashes are not for 0.1.16; rebuild and retest the exact versioned artifact before release.
- Remote tag `v0.1.15` already exists and points to different source; remote `v0.1.16` was absent at the last check. Do not reuse the 0.1.15 tag or checksum. The 0.1.16 source has not been committed, tagged, run through GitHub CI, or released.
- Test token crop Pip Dev trước đây đạt 9/9. Chromium Windows build ở `D:\src\out\PipDevPortableSDK` vẫn còn tiến trình Ninja hoạt động; lần kiểm tra ngày 2026-10-23 chưa có `chrome.exe` hoặc `mini_installer.exe`. Việc build chưa xác minh app, MCP hoặc khóa input lúc chạy.
- Trạng thái chi tiết theo repo nằm trong [`browser-source/RELEASE_NOTES.md`](browser-source/RELEASE_NOTES.md), [`pip-dev/PIP_DEV_STATUS.md`](pip-dev/PIP_DEV_STATUS.md) và các báo cáo kiểm thử; cập nhật snapshot này khi có kiểm chứng mới.
