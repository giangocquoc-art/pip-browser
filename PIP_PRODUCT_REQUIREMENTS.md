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

- The new public 0.1.16 GitHub setup is 25,443,840 bytes, SHA-256 `3ffd295cd68e8d65b096cd538946be46238ece143c1fc5ecfaa26badb41be7c2`; source archive SHA-256 is `caacdf1a409fd7c6ac1d4aeb3de9bd2c5fef2e816784a88f913826e2faf6a7f7`. Both hashes were checked against the uploaded `SHA256SUMS.txt`, and GitHub attestations for the setup and source archive verified. The exact release setup installed as version 0.1.16 in an isolated D-only profile; its Pip.exe hash is `512fc64b41ed7e56ed2026df73aa9569fd06134059cb2407f002d63f121b8526`. With Google DNS on, YouTube playback advanced 10.11 seconds, no offline UI appeared, six media responses succeeded, and NetLog showed the loopback proxy with 0 direct DNS transactions. With Google DNS off, playback advanced 10.15 seconds, no proxy was configured, and NetLog recorded 168 Windows DNS transactions. The exact CI-installed app was uninstalled, test data remained, and shortcuts were removed. Authenticode is `NotSigned`; these results reflect this test network and do not demonstrate YouTube ad blocking or guarantee connectivity on another network.
- Pip 0.1.16 is published as GitHub release `v0.1.16`. The website `version.json`, homepage fallback version, installation guide, privacy page, and Google DNS guide contain matching release metadata or fallback disclosure locally. The Cloudflare download endpoint was downloaded and matched the release's checksum and size; the Vercel website production deploy still needs verification.
- DNS source candidate is versioned 0.1.16 in both Cargo manifests and Cargo.lock. Bounded Windows DNS fallback is allowed only when both Google A and AAAA queries fail transiently; authoritative NXDOMAIN, NODATA, invalid responses and proxy/HTTPS failures stay fail-closed. Privacy and Google DNS help pages disclose fallback and hostname visibility.
- The pre-bump local binary `browser-source/dist/PipSetup.exe` was built as 0.1.15 (before the 0.1.16 version change): SHA-256 `2c4b83bc5618ea0caa186ff46aab0b7c2fc5ab046c04cc41ff062c777b79ee68`, 25,434,112 bytes; installed `Pip.exe` hash `31980b2162f2d7eaa3302c6b55edc5353e5a75ca9a1942d52090d31eb7cd1a38`. Silent install/uninstall and separate YouTube+Google-DNS portable playback passed for that pre-bump payload. These historical hashes do not identify 0.1.16; use the published hashes above.
- Remote tag `v0.1.15` already exists and points to different source; do not reuse its tag or checksum. Pip 0.1.16 source is committed as `41d9fbe`, CI tests/build/Clippy/dependency audit/attestations passed, tag `v0.1.16` is pushed, and release 0.1.16 is public. Cloudflare download and checksum responses now match; Vercel production deployment is still pending.
- Test token crop Pip Dev trước đây đạt 9/9. Chromium Windows build ở `D:\src\out\PipDevPortableSDK` vẫn còn tiến trình Ninja hoạt động; lần kiểm tra ngày 2026-10-23 chưa có `chrome.exe` hoặc `mini_installer.exe`. Việc build chưa xác minh app, MCP hoặc khóa input lúc chạy.
- Trạng thái chi tiết theo repo nằm trong [`browser-source/RELEASE_NOTES.md`](browser-source/RELEASE_NOTES.md), [`pip-dev/PIP_DEV_STATUS.md`](pip-dev/PIP_DEV_STATUS.md) và các báo cáo kiểm thử; cập nhật snapshot này khi có kiểm chứng mới.

## Snapshot đối chiếu hiện tại — 2026-11-06

- Mã `v0.1.19` được phát hành tại commit `b9b6cb4f6726712e27596417e7bbe0bd18fac9c6`; PR #74 sau đó gộp sửa race trong native WebView2 harness tại `2a9ab32800dd4150ce61d0dd0ce4599573b0f75f`. Release GitHub `v0.1.19` đã public; Windows installer 25,527,296 byte, SHA-256 `c449f8c0ca61778198ef1f82085a0fcf95e4ee26ab27b70213a0ec32a8c6e6ed`. Workflow release [42178787592](https://github.com/giangocquoc-art/pip-browser/actions/runs/42178787592) và kiểm chứng main sau PR #74 [42185194413](https://github.com/giangocquoc-art/pip-browser/actions/runs/42185194413) đều đạt build, popup fixtures, Google DNS/WebView connectivity, installer dưới quyền user thường, tests và Clippy. Installer/source ZIP attestations đã xác minh; Authenticode vẫn `NotSigned`. Cloudflare download `/` và `/checksums` phục vụ đúng hash/kích thước 0.1.19. Website production được đồng bộ sau PR #80: deployment `dpl_DnRTnn583E4n1jfc9Daxn3YjNoa2` chạy commit `4e2160f176df6a6ece019f2ed880c621b48e5d25`; `version.json` và `/checksums` công khai trả cùng phiên bản/hash/kích thước.
- PR [#65](https://github.com/giangocquoc-art/pip-browser/pull/65) đã gộp tại `6352abb`. Android thêm lịch sử/dấu trang SQLite, giới hạn lịch sử 1.000 lượt và khôi phục URL/tab chọn sau khi mở lại. PR-head CI gồm Windows, APK/JNI, emulator Android 15, macOS Intel/ARM và worker tests đều xanh. Đây vẫn là artifact Android development, không phải bản tải public.
- PR [#68](https://github.com/giangocquoc-art/pip-browser/pull/68) thêm GeckoView 153 **chỉ cho debug pilot**, giữ Pip app/release trên Android System WebView. CI [42170419773](https://github.com/giangocquoc-art/pip-browser/actions/runs/42170419773) qua 13/13 instrumentation tests; PR [#70](https://github.com/giangocquoc-art/pip-browser/pull/70), CI [42174487130](https://github.com/giangocquoc-art/pip-browser/actions/runs/42174487130), thêm xác nhận cấu hình URI `https://dns.google/dns-query`/`TRR_MODE_ONLY` và đạt 14/14. PR [#75](https://github.com/giangocquoc-art/pip-browser/pull/75) bổ sung kiểm chứng cookie hồ sơ còn sau khi đóng/mở lại GeckoSession cùng context ID; emulator report [42186914391](https://github.com/giangocquoc-art/pip-browser/actions/runs/42186914391) xác nhận 3/3 bài GeckoView và 14/14 bài instrumentation Android đều qua. Main merge commit `7fe1022216ff7a993bf8a6979e2cce7869b97a0b`; post-merge platform CI [42188580198](https://github.com/giangocquoc-art/pip-browser/actions/runs/42188580198) đạt. Cách ly/persistence được chứng minh trong pilot, chưa tích hợp vào Android release. Google DoH mới được kiểm tra cấu hình; chưa có network trace chứng minh request Internet dùng resolver đó.
- Android bản Pip hiện dùng WebView: tab sản phẩm vẫn chung cookie store; chưa có hồ sơ riêng/ẩn danh được tích hợp vào app, HTTP, DNS dùng thật, upload/chọn tệp, dịch, mật khẩu, Sync hay luồng đặt trình duyệt mặc định. PR #81 đã thêm tải HTTPS cơ bản qua Android DownloadManager sau xác nhận, không chuyển cookie Pip. CI build/unit tests và emulator lifecycle đạt; chưa có bài thử end-to-end hoàn tất tải xuống trên thiết bị thật. Pilot không bật các chức năng GeckoView trên release và chưa chọn GeckoView làm engine mặc định. Chặn nội dung hiện thiên về network filtering và policy tên miền; không có cosmetic/scriptlet parity hoặc bằng chứng chặn hết quảng cáo YouTube. Chưa có khóa release, thiết bị thật hay Google Play review.
- macOS đã build/package trên Intel và Apple Silicon trong CI, nhưng vẫn là artifact development ký ad-hoc; chưa có Developer ID/notarization. Cần kiểm thử mạng, hồ sơ/riêng tư, DNS, download, quyền hệ thống và nâng cấp trên hai kiến trúc trước khi bật tải public.
- Windows public hiện là 0.1.19; gồm nội dung Sync tự động đã sửa ở PR #62 và giảm biên dịch lặp danh sách miền popup. Website manifest/checksum đã khớp installer production; cảnh báo SmartScreen vẫn có thể hiện do installer chưa ký Authenticode.
- Pip Dev: PR #54 đã harden MCP/API và thêm per-tab lease ở source; PR #64 giảm I/O launcher. Chưa có Pip Dev Chromium riêng được build/đóng gói và chạy thật. Khóa input vật lý, DevTools đồng thời với MCP, luồng DOM/canvas runtime, hủy tác vụ và cài đặt chưa đạt nghiệm thu. Canvas hiện chỉ có crop, chưa có OCR/adapter trạng thái; plugin trong repo không đồng nghĩa được duyệt lên marketplace. Cần giữ license/notices và nghĩa vụ AGPL-3.0 khi phân phối.
- Ưu tiên tiếp theo theo kế hoạch: (1) Android: xác minh tuyến DoH thật, hoàn thiện upload/quyền/media và thử GeckoView adblock trước khi đổi engine mặc định; (2) macOS: kiểm thử các luồng hằng ngày và chuẩn bị ký/notarization; (3) phát hành beta mobile sau khi các luồng nền tảng đạt, rồi quay lại Pip Dev Chromium runtime. Chưa có baseline RAM/CPU/pin để tuyên bố Pip nhẹ hơn Chrome.

## Cập nhật kiểm chứng sau PR #81 — 2026-11-06

- PR [#79](https://github.com/giangocquoc-art/pip-browser/pull/79) đã gộp tại `b6738ac529413558c5e2ab1b93aa62048873c957`; fixture popup kiểm tra redirect sang host giả `rotating-cdn.example`. CI PR-head đạt popup WebView2, DNS/YouTube, installer dưới quyền user thường, tests và Clippy. CI hậu merge ban đầu thất bại ở fixture service worker `/sw.js`; PR [#80](https://github.com/giangocquoc-art/pip-browser/pull/80) thêm retry tối đa ba lần với backoff. Đây là sửa fixture kiểm thử, không thay đổi engine production.
- PR #80 đã gộp tại `4e2160f176df6a6ece019f2ed880c621b48e5d25`; Windows, Android APK/emulator, macOS Intel/ARM và Vercel preview đều đạt. Vercel production deployment `dpl_DnRTnn583E4n1jfc9Daxn3YjNoa2` đạt `READY` và gắn alias `pip.psharehub.com`. Kiểm tra HTTP ngày 2026-11-06: `version.json` trả `0.1.19`, installer `PipSetup.exe`, kích thước `25527296`, SHA-256 `c449f8c0ca61778198ef1f82085a0fcf95e4ee26ab27b70213a0ec32a8c6e6ed`; Cloudflare `/checksums` trả cùng hash.
- PR [#78](https://github.com/giangocquoc-art/pip-browser/pull/78) đã gộp tại `7d638d73454aa150c81afa4a6d07d047646ce2b6`. `vercel.json` dùng `ignoreCommand` fail-open để tránh deploy khi commit không thay đổi file website; website/Pip Dev checks và Vercel preview đều đạt sau khi cập nhật base. Thay đổi này giảm build không cần thiết, không thay thế kiểm tra production khi nội dung website thay đổi.
- PR [#81](https://github.com/giangocquoc-art/pip-browser/pull/81) đã gộp tại `ef8d19fd20f5b9b06844fe6dbef1e8cfc964f801`. Sau khi cập nhật base, CI đạt: Android APK/unit tests [42260790430](https://github.com/giangocquoc-art/pip-browser/actions/runs/42260790430), Android emulator [42260790430](https://github.com/giangocquoc-art/pip-browser/actions/runs/42260790430), Windows [42260790358](https://github.com/giangocquoc-art/pip-browser/actions/runs/42260790358), Pip Dev [42260790339](https://github.com/giangocquoc-art/pip-browser/actions/runs/42260790339), macOS Intel/ARM [42260790430](https://github.com/giangocquoc-art/pip-browser/actions/runs/42260790430) và Vercel preview. Các workflow mobile kiểm tra build, unit/instrumentation suite hiện hữu; chưa có kịch bản riêng kiểm tra DownloadManager tải xong và mở file trên thiết bị thật.
- DownloadManager yêu cầu xác nhận, ghi vào thư mục Downloads, làm sạch tên tệp và không nhận cookie/credential từ Pip. Nó dùng network stack hệ thống ngoài WebView/adblock/DNS của Pip; tải tài nguyên cần đăng nhập có thể thất bại. Chưa hỗ trợ upload, resume trong Pip hoặc chọn vị trí lưu tùy ý.
- Rà soát Android: chưa có profile/ẩn danh tách cookie, upload, quyền camera/mic, Google DNS thực, mật khẩu, Sync hoặc chọn GeckoView làm engine release. DownloadManager chưa có giao diện resume.
- Rà soát macOS: hiện vẫn là preview, chưa có profile/ẩn danh, lịch sử/dấu trang, quyền website, Google DNS hoặc Sync; CI tạo app development ad-hoc và đặt deployment target macOS 12, trong khi kế hoạch đề xuất macOS 14. Chưa có Developer ID/notarization.
- Pip Dev: vẫn chưa có binary Chromium riêng được đóng gói và chạy thật. `Stop` hiện hủy cả phiên agent và các tab khác, dù UI mô tả như dừng một tab; cần sửa ngữ nghĩa page-scoped hoặc nói rõ phạm vi dừng. Native input lock và luồng DevTools+MCP chưa được nghiệm thu trên runtime.
