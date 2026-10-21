# Microsoft Store listing draft

Use this copy only after reserving the Pip product name and confirming the final Store identity. The app is a 64-bit Windows desktop browser built on Microsoft Edge WebView2.

## Product details

- Product name: Pip
- Version: 0.1.14 (candidate; publish only after this build is approved)
- Supported architecture: x64
- Minimum Windows version: Windows 10, version 2004 (build 19041)
- Runtime: Microsoft Edge WebView2 Evergreen Runtime
- Website: https://pip.psharehub.com/
- Privacy policy: https://pip.psharehub.com/quyen-rieng-tu/
- Support: https://github.com/giangocquoc-art/pip-browser/issues
- Source code: https://github.com/giangocquoc-art/pip-browser

## Vietnamese listing

### Short description

Pip là trình duyệt Windows gọn nhẹ với tìm kiếm Google, hồ sơ riêng, DNS tùy chọn và công cụ lọc quảng cáo.

### Description

Pip là trình duyệt Windows tập trung vào trải nghiệm gọn gàng và quyền riêng tư.

- Tìm kiếm Google ngay từ thanh địa chỉ; có thể đổi công cụ tìm kiếm trong cài đặt.
- Tách dữ liệu duyệt web theo hồ sơ trên cùng thiết bị.
- Dùng cửa sổ ẩn danh khi không muốn lưu phiên duyệt vào lịch sử thông thường của Pip.
- Bật Google DNS-over-HTTPS tùy chọn để mã hóa truy vấn DNS trên đường truyền.
- Lọc một số yêu cầu quảng cáo và theo dõi; khi bật, Pip cũng chặn popup tự mở và đối chiếu địa chỉ popup với EasyList, uBlock Origin, HaGeZi Pop-Up Ads và snapshot domain cá cược HaGeZi.
- Gom tab theo nhóm, khôi phục phiên duyệt và dịch trang sang tiếng Việt trên thiết bị khi phiên bản WebView2 hỗ trợ Translator API.
- Tùy chọn đồng bộ dấu trang và dữ liệu đã chọn bằng mã khôi phục. Pip mã hóa dữ liệu trên thiết bị trước khi gửi đến Pip Sync trên Cloudflare. Lịch sử, tab đang mở, cài đặt được chọn và kho mật khẩu Pip có thể bật riêng, mặc định tắt; bật đồng bộ mật khẩu cần xác nhận thêm. Cookie, dữ liệu website, mật khẩu WebView2 và thông tin thanh toán không được đồng bộ.
- Lưu dấu trang, lịch sử và tệp tải xuống trên thiết bị.

Pip dùng Microsoft Edge WebView2 để hiển thị trang web. Bộ lọc quảng cáo không chặn được mọi quảng cáo; kết quả, đặc biệt trên YouTube, có thể thay đổi theo website và cập nhật của website. Google DNS không phải VPN và không ẩn địa chỉ IP. Xem chính sách quyền riêng tư để biết dữ liệu nào được lưu trên máy và dịch vụ bên ngoài nào nhận dữ liệu khi bạn duyệt web.

## English listing

### Short description

Pip is a lightweight Windows browser with Google search, separate profiles, optional DNS-over-HTTPS, and content filtering.

### Description

Pip is a Windows browser focused on a simple browsing experience and clear privacy controls.

- Search with Google from the address bar, or choose another search provider in settings.
- Keep browsing data in separate profiles on the same device.
- Use an InPrivate window to keep that session out of Pip's normal browsing history.
- Optionally enable Google DNS-over-HTTPS to encrypt DNS queries in transit.
- Filter some advertising and tracking requests; when enabled, Pip also blocks automatic popups and checks popup destinations against bundled EasyList, uBlock Origin, HaGeZi Pop-Up Ads, and a HaGeZi gambling domain snapshot.
- Organize tabs into groups, restore browsing sessions, and translate pages into Vietnamese on-device when the installed WebView2 supports its Translator API.
- Optionally sync bookmarks and selected data with a recovery code. Pip encrypts data on the device before sending it to Pip Sync on Cloudflare. History, open tabs, selected settings, and the Pip password vault can be enabled individually and are off by default; password sync requires separate confirmation. Cookies, website data, WebView2 passwords, and payment details are not synced.
- Keep bookmarks, history, and downloads on your device.

Pip uses Microsoft Edge WebView2 to display websites. Ad filtering does not block every ad, and results—especially on YouTube—can vary with the website and its updates. Google DNS is not a VPN and does not hide your IP address. See the privacy policy for details about on-device data and external services used while browsing.

## Certification notes

- This package is a full-trust Win32 app (`runFullTrust`) and uses Microsoft Edge WebView2. The app explains how to install the official WebView2 Runtime if it is missing.
- Pip does not provide a Pip account or usage analytics. Optional sync encrypts data on-device. Bookmarks sync when Pip Sync is set up; history, open tabs, selected settings, and Pip vault passwords are opt-in. Cookies, website data, WebView2 passwords, payment details, and profiles are not synced.
- Store listing and privacy policy must describe the exact release being submitted. Optional sync can include history, open tabs, selected settings, and Pip vault passwords only when explicitly enabled; cookies, website data, WebView2 passwords, and payment details are excluded.
- Run the Windows App Certification Kit against the final identity-bound package and test install, first launch, update, and uninstall before submission.
- Do not claim universal ad removal, YouTube ad removal, VPN protection, or that browsing is invisible to websites or network providers.
