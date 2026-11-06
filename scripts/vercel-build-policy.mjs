const topLevelWebsiteFile = /^[^/]+\.(?:html|css|js|json|xml|txt)$/i;
const websiteDirectories = [
  ".well-known/",
  "assets/",
  "cau-hoi/",
  "huong-dan/",
  "marketing/",
  "pip-dev/docs/",
  "pip-dev/plugins/",
  "pip-dev/signatures/",
  "pip-dev/updates/",
  "quyen-rieng-tu/",
  "tin-cay/",
  "tinh-nang/",
];

export function isWebsitePath(filePath) {
  const normalized = filePath.replaceAll("\\", "/").replace(/^\.\//, "");
  return normalized === "vercel.json"
    || topLevelWebsiteFile.test(normalized)
    || websiteDirectories.some((directory) => normalized.startsWith(directory));
}

export function shouldBuildWebsite(changedPaths) {
  return changedPaths.some(isWebsitePath);
}
