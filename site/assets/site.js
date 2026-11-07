const REPO = "Invalid8/vscrybe";
const API = `https://api.github.com/repos/${REPO}`;
const LATEST = `https://github.com/${REPO}/releases/latest/download`;

const FILES = {
  windows: "VScrybe-windows-x64-setup.exe",
  macArm: "VScrybe-macos-arm64.dmg",
  macIntel: "VScrybe-macos-x64.dmg",
  deb: "VScrybe-linux-amd64.deb",
  appimage: "VScrybe-linux-amd64.AppImage",
};

const PLATFORMS = {
  windows: { name: "Windows", icon: "windows", file: "windows", detail: "Windows 10 and 11 · 64-bit installer" },
  mac: { name: "Mac", icon: "apple", file: "macArm", detail: "Apple silicon · macOS 11 or later" },
  macIntel: { name: "Mac", icon: "apple", file: "macIntel", detail: "Intel Mac · macOS 11 or later" },
  linux: { name: "Linux", icon: "linux", file: "deb", detail: "Ubuntu and Debian · .deb package" },
};

const OPEN_IT = "Open it from your browser's downloads.";
const START = (from) => ({ mock: "app", title: "Start transcribing", text: `${from} and drop in a voice note.` });
const MAC = [
  { mock: "dmg", title: "Drag it to Applications", text: "Open the .dmg, then drag VScrybe onto the Applications folder." },
  { mock: "security", title: "Allow it to open", text: "Open it once, then in System Settings → Privacy & Security click Open Anyway. Only the first time." },
  START("Open VScrybe from Launchpad"),
];

const GUIDES = {
  windows: [
    { mock: "file", title: "Open the installer", text: OPEN_IT },
    { mock: "smartscreen", title: "Run it anyway", text: "Windows may say it protected your PC. Choose More info, then Run anyway. Only the first time." },
    START("Open VScrybe from the Start menu"),
  ],
  macArm: MAC,
  macIntel: MAC,
  deb: [
    { mock: "file", title: "Open the package", text: OPEN_IT },
    { mock: "terminal", title: "Install it", text: "Click Install in your software centre, or run the command shown." },
    START("Open VScrybe from your app menu"),
  ],
  appimage: [
    { mock: "file", title: "Find the file", text: "It's in your browser's downloads." },
    { mock: "permissions", title: "Make it executable", text: "In the file's properties turn on Allow executing, or run chmod +x on it." },
    { mock: "app", title: "Start transcribing", text: "Double-click it to open VScrybe, then drop in a voice note." },
  ],
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
    started: "",
    url(key) {
      return `${LATEST}/${FILES[key]}`;
    },
    size(key) {
      return this.sizes[key] ?? "";
    },
    get file() {
      return FILES[this.started];
    },
    get steps() {
      return GUIDES[this.started] ?? [];
    },
    start(key) {
      this.started = key;
      scrollTo({ top: 0, behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
    },
    async init() {
      const asked = new URLSearchParams(location.search).get("get");
      if (Object.hasOwn(FILES, asked ?? "")) {
        history.replaceState(null, "", location.pathname);
        this.started = asked;
        location.assign(this.url(asked));
      }
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

  Alpine.data("love", () => ({
    burst() {
      if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
      const { left, top, width } = this.$el.getBoundingClientRect();
      for (let i = 0; i < 9; i++) {
        const heart = document.createElement("span");
        heart.className = "spark";
        heart.textContent = "♥︎";
        heart.style.left = `${left + width / 2}px`;
        heart.style.top = `${top}px`;
        document.body.append(heart);
        const drift = (Math.random() - 0.5) * 90;
        const rise = 70 + Math.random() * 70;
        const size = 0.6 + Math.random() * 0.7;
        heart.animate(
          [
            { transform: "translate(-50%, 0) scale(0.2)", opacity: 1 },
            { transform: `translate(calc(-50% + ${drift * 0.3}px), -24px) scale(${size * 1.25})`, opacity: 1, offset: 0.2 },
            { transform: `translate(calc(-50% + ${drift}px), -${rise}px) scale(${size})`, opacity: 0 },
          ],
          { duration: 900 + Math.random() * 500, delay: i * 35, easing: "cubic-bezier(0.2, 0.8, 0.3, 1)", fill: "backwards" },
        ).finished.then(() => heart.remove());
      }
    },
  }));

  Alpine.data("share", () => ({
    copied: false,
    send() {
      if (!navigator.share) return this.copy();
      navigator.share({ title: "VScrybe", text: "Voice notes to text, on your computer.", url: location.href }).catch(() => {});
    },
    async copy() {
      await navigator.clipboard.writeText(location.href);
      this.copied = true;
      setTimeout(() => (this.copied = false), 2000);
    },
  }));
});
