# Pip Browser website

[![Verify Pip](https://github.com/giangocquoc-art/pip-browser/actions/workflows/verify-pip.yml/badge.svg)](https://github.com/giangocquoc-art/pip-browser/actions/workflows/verify-pip.yml)

Trang giới thiệu và tải Pip cho Windows: https://pip.psharehub.com

Website là HTML, CSS và JavaScript thuần. Không dùng framework, font ngoài hay thư viện hoạt ảnh. Pip dùng Microsoft Edge WebView2 Runtime do Microsoft cập nhật; bộ cài hướng dẫn cài Runtime chính thức nếu máy chưa có.

## Cấu trúc

- `index.html`: landing page ngắn, nút tải ở phần đầu trang và ảnh chụp ứng dụng thật.
- `tinh-nang/`, `cau-hoi/` và `huong-dan/`: nội dung theo từng ý định tìm kiếm. Tám bài hướng dẫn gom 40 cụm có liên quan vào từng chủ đề thay vì tạo 40 trang mỏng.
- `robots.txt`, `sitemap.xml`: cho phép crawl và liệt kê các URL chuẩn của trang chính, trang tính năng, FAQ, chính sách quyền riêng tư và cụm hướng dẫn.
- `quyen-rieng-tu/`: chính sách mô tả lưu trữ cục bộ, website bên thứ ba, tìm kiếm Google, DNS tùy chọn và chẩn đoán WebView2.
- `tin-cay/`: hướng dẫn kiểm tra SHA-256, GitHub build attestation và trạng thái Authenticode hiện tại.
- `guide.css`, `assets/guides/`: bố cục hướng dẫn responsive và tám ảnh WebP có mascot Pip.
- `marketing/seo-keyword-map.md`: bản đồ 40 cụm tìm kiếm, ý định và URL đích; gợi ý autocomplete không phải số liệu lượt tìm kiếm.
- `assets/app-*.webp`: ảnh giao diện được chụp từ ứng dụng Pip; WebP để giảm dung lượng tải.
- `assets/og-pip.jpg`: ảnh chia sẻ mạng xã hội.
- `version.json`: phương án dự phòng nếu GitHub Releases không phản hồi.
- `browser-source/`: mã nguồn ứng dụng Pip; mỗi bản phát hành đính kèm ZIP mã nguồn và SHA-256 tương ứng. Mã phiên bản mới nhất được hiển thị trên trang tải.
- `marketing/launch-kit.md`: định vị và bản nháp nội dung marketing.

## Tải và cập nhật

Nút tải kiểm tra GitHub Releases để lấy bản mới nhất; nếu API không phản hồi, website dùng `version.json`. Khi đẩy tag đúng dạng `v<version>` khớp với phiên bản trong Cargo, workflow `.github/workflows/release-pip.yml` sẽ build bộ cài và mã nguồn, tạo checksum, ký attestations bằng Sigstore thông qua GitHub, rồi tạo bản phát hành nháp. Người duyệt có thể xác nhận nội dung rồi mới xuất bản. Workflow không sửa các bản phát hành đã công bố.

Người tải có thể cài [GitHub CLI](https://cli.github.com/) rồi kiểm tra nguồn gốc build sau khi tải file:

```powershell
gh attestation verify .\Pip-Setup-<version>-Windows-x64.exe --repo giangocquoc-art/pip-browser
gh attestation verify .\Pip-source-<version>.zip --repo giangocquoc-art/pip-browser
```

Attestation xác nhận repository, commit và workflow đã tạo file; nó không phải chữ ký Authenticode của Windows và không tự gỡ cảnh báo SmartScreen. Để Windows nhận diện nhà phát hành khi người dùng cài đặt, Pip vẫn cần phân phối MSIX qua Microsoft Store hoặc ký Authenticode bằng chứng thư được tin cậy.

Website triển khai từ nhánh `main` trên Vercel. Sau khi cập nhật, kiểm tra trang chủ, trang phụ, ảnh OG, sitemap, liên kết tải và giao diện mobile.

## Thông tin cài đặt

Pip hỗ trợ Windows 10/11 64-bit và cài riêng cho tài khoản Windows hiện tại. Bộ cài chưa ký số; SmartScreen có thể cảnh báo. Trang giới thiệu ghi rõ giới hạn của chế độ ẩn danh, DNS và bộ lọc quảng cáo.

## Android và macOS đang phát triển

Android dùng Android System WebView và Brave Rust qua JNI; macOS dùng WKWebView qua Wry, bộ lọc WebKit native và hộp thoại lưu tệp hệ thống. Đây là các bản phát triển riêng, chưa ngang chức năng Windows và chưa có nút tải công khai. iOS để sau.

Xem [phạm vi, bằng chứng kiểm thử và điều kiện phát hành đa nền tảng](browser-source/MULTIPLATFORM.md), [build Android](browser-source/android/README.md) và [workflow hosted](.github/workflows/verify-pip-platforms.yml). Có thể dùng GitHub Actions để build thay vì cài SDK hoặc biên dịch trên máy Windows. Artifact thử nghiệm chỉ giữ 7 ngày, cần đăng nhập GitHub để tải; chúng dùng chữ ký debug/ad-hoc, không phải gói phát hành chính thức.

## Pip Dev cho Codex và agent AI

Pip Dev là dự án trình duyệt Chromium riêng đang phát triển trong [`pip-dev/`](pip-dev/). Nó bổ sung kết nối MCP để Codex và agent tương thích đọc, thao tác trên tab Pip Dev. Plugin không cài trình duyệt; hiện chưa có bộ cài Pip Dev cho người dùng cuối và trình duyệt cần chạy trên cùng máy với agent.

Codex CLI có thể thêm GitHub repo marketplace và cài plugin bằng các lệnh sau:

```powershell
codex plugin marketplace add giangocquoc-art/pip-browser --sparse .agents/plugins --sparse pip-dev/plugins/pip-dev-browser
codex plugin add pip-dev-browser --marketplace pip-dev-plugins
```

Xem [hướng dẫn cài Pip Dev trên Windows](pip-dev/docs/INSTALL_WINDOWS.md), [kết nối Codex qua MCP](pip-dev/docs/PIP_DEV_AGENT_SETUP.md) và [trạng thái triển khai](pip-dev/PIP_DEV_STATUS.md). Plugin sẽ chỉ điều khiển tab khi Pip Dev và MCP server đang chạy trên PC đó.

Đây là marketplace của repo GitHub, chưa phải mục được liệt kê trong thư mục plugin công khai của Codex/ChatGPT. Pip Dev cũng chưa có bộ cài trình duyệt Windows cho người dùng cuối.

## Báo cáo bảo mật

Đọc [chính sách bảo mật và cách báo cáo lỗ hổng](SECURITY.md). Không đăng chi tiết khai thác hoặc dữ liệu cá nhân trong issue công khai.
