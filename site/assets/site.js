const REPO = "Invalid8/vscribe";
const API = `https://api.github.com/repos/${REPO}`;
const LATEST = `https://github.com/${REPO}/releases/latest/download`;

const FILES = {
  windows: "vScribe-windows-x64-setup.exe",
  macArm: "vScribe-macos-arm64.dmg",
  macIntel: "vScribe-macos-x64.dmg",
  deb: "vScribe-linux-amd64.deb",
  appimage: "vScribe-linux-amd64.AppImage",
};

const PLATFORMS = {
  windows: { name: "Windows", icon: "windows", file: "windows", detail: "Windows 10 and 11 · 64-bit installer" },
  mac: { name: "Mac", icon: "apple", file: "macArm", detail: "Apple silicon · macOS 11 or later" },
  macIntel: { name: "Mac", icon: "apple", file: "macIntel", detail: "Intel Mac · macOS 11 or later" },
  linux: { name: "Linux", icon: "linux", file: "deb", detail: "Ubuntu and Debian · .deb package" },
};

function detectOs() {
  const ua = navigator.userAgent;
  if (navigator.userAgentData?.mobile || /Android|iPhone|iPad|Mobile/.test(ua)) return "phone";
  if (/Mac/.test(ua)) return navigator.maxTouchPoints > 1 ? "phone" : "mac";
  if (/Windows/.test(ua)) return "windows";
  if (/Linux|X11|CrOS/.test(ua)) return "linux";
  return "other";
}

function remember(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}

const megabytes = (bytes) => `${Math.max(1, Math.round(bytes / 1e6))} MB`;
const compact = new Intl.NumberFormat("en", { notation: "compact" });
const longDate = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "long", year: "numeric" });
const github = (path = "") => fetch(API + path).then((response) => (response.ok ? response.json() : Promise.reject(response.status)));

document.addEventListener("alpine:init", () => {
  Alpine.store("theme", {
    dark: document.documentElement.dataset.theme === "dark",
    toggle() {
      this.dark = !this.dark;
      document.documentElement.dataset.theme = this.dark ? "dark" : "light";
      remember("theme", document.documentElement.dataset.theme);
    },
  });

  Alpine.store("device", {
    os: detectOs(),
    intel: false,
    get phone() {
      return this.os === "phone";
    },
    get platform() {
      return PLATFORMS[this.os === "mac" && this.intel ? "macIntel" : this.os] ?? PLATFORMS.windows;
    },
    async init() {
      if (this.os !== "mac") return;
      const hints = await navigator.userAgentData?.getHighEntropyValues(["architecture"]).catch(() => null);
      this.intel = hints?.architecture === "x86";
    },
  });

  Alpine.store("release", {
    version: "",
    date: "",
    stars: "",
    sizes: {},
    url(key) {
      return `${LATEST}/${FILES[key]}`;
    },
    size(key) {
      return this.sizes[key] ?? "";
    },
    async init() {
      const [release, repo] = await Promise.allSettled([github("/releases/latest"), github()]);
      if (repo.status === "fulfilled") this.stars = compact.format(repo.value.stargazers_count);
      if (release.status !== "fulfilled") return;
      const { tag_name, published_at, assets } = release.value;
      this.version = tag_name.replace(/^v/, "");
      this.date = longDate.format(new Date(published_at));
      for (const [key, name] of Object.entries(FILES)) {
        const asset = assets.find((a) => a.name === name);
        if (asset) this.sizes[key] = megabytes(asset.size);
      }
    },
  });

  Alpine.data("share", () => ({
    copied: false,
    send() {
      if (!navigator.share) return this.copy();
      navigator.share({ title: "vScribe", text: "Voice notes to text, on your computer.", url: location.href }).catch(() => {});
    },
    async copy() {
      await navigator.clipboard.writeText(location.href);
      this.copied = true;
      setTimeout(() => (this.copied = false), 2000);
    },
  }));
});
