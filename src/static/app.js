function app() {
  return {
    depth: 0,
    session: null,
    compact: false,
    atBottom: true,
    query: "",
    model: stored("model", "small"),
    language: stored("language", "en"),
    railOpen: stored("rail", "open") === "open",
    theme: stored("theme", "system"),
    drawer: false,
    toasts: [],
    consent: null,
    onboarding: stored("onboarded", "") ? null : 0,
    desktop: !!window.__TAURI__,
    busy: null,
    staged: [],
    previewing: null,
    preview: null,

    init() {
      this.$watch("model", (value) => store("model", value));
      this.$watch("language", (value) => store("language", value));
      this.$watch("railOpen", (value) => store("rail", value ? "open" : "closed"));
      this.$watch("theme", (value) => {
        store("theme", value);
        applyTheme(value);
      });
      document.body.addEventListener("htmx:responseError", (event) => this.notify(errorMessage(event.detail.xhr), "error"));
      document.fonts.ready.then(() => {
        if (this.session && this.atBottom) this.toBottom();
        this.syncScroll();
      });
      document.body.addEventListener("htmx:beforeSwap", (event) => {
        const note = event.detail.target;
        const audio = note?.classList?.contains("vn") && note.querySelector("audio");
        if (audio && !audio.paused) resumePoints.set(note.id.slice("note-".length), audio.currentTime);
      });
      document.body.addEventListener("htmx:afterSettle", () => {
        if (this.atBottom) this.toBottom();
        this.syncScroll();
      });
      document.body.addEventListener("htmx:afterRequest", () => {
        const title = document.getElementById("meta-og-title");
        if (title) document.title = title.content;
      });
      document.body.addEventListener("htmx:sendError", () => this.notify(UNREACHABLE, "error"));
      window.addEventListener("consent-request", (event) => this.openConsent(event.detail));
      trayStore("readonly", (store) => store.getAll())
        .then((items) => (this.staged = [...items.sort((a, b) => a.id - b.id), ...this.staged]))
        .catch((error) => console.warn("Couldn't restore staged files", error));
      window.addEventListener("stage", (event) => this.stage([event.detail.file], event.detail.seconds));
      window.addEventListener("notify", (event) => this.notify(event.detail.message, event.detail.kind, event.detail.action));
      this.watchUploads();
      if (this.desktop) {
        document.addEventListener("contextmenu", (event) => {
          if (!event.target.closest("input, textarea, [contenteditable]")) event.preventDefault();
        });
        document.addEventListener("keydown", (event) => {
          if (event.key === "F5" || ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "r")) {
            event.preventDefault();
            location.reload();
          }
          if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") selectWithin(event);
        });
      }
      if (this.onboarding !== null) this.$nextTick(() => this.$refs.onboarding.showModal());
    },

    notify(message, kind = "success", action = null) {
      if (this.toasts.some((toast) => toast.message === message)) return;
      const id = Date.now() + Math.random();
      this.toasts.push({ id, message, kind, action });
      setTimeout(() => this.dismiss(id), kind === "error" || action ? 8000 : 3500);
    },

    windowAction(action) {
      window.__TAURI__.window.getCurrentWindow()[action]();
    },

    watchUploads() {
      const form = document.getElementById("upload");
      form.addEventListener("htmx:afterRequest", (event) => {
        if (!event.detail.successful) return;
        this.staged = [];
        trayStore("readwrite", (store) => store.clear()).catch((error) => console.warn("Couldn't clear staged files", error));
      });
    },

    async exportAll(url) {
      this.busy = { kind: "export", title: "Preparing your export", detail: "Collecting every transcript into one zip, a folder per session.", progress: null };
      try {
        const response = await fetch(url);
        if (!response.ok) throw new Error(errorMessage({ status: response.status, responseText: await response.text() }));
        const name = /filename="([^"]+)"/.exec(response.headers.get("Content-Disposition") || "")?.[1] || "voice-notes.zip";
        const link = Object.assign(document.createElement("a"), { href: URL.createObjectURL(await response.blob()), download: name });
        link.click();
        setTimeout(() => URL.revokeObjectURL(link.href), 60_000);
      } catch (error) {
        this.notify(error instanceof TypeError ? UNREACHABLE : error.message, "error");
      } finally {
        this.busy = null;
      }
    },

    dismiss(id) {
      this.toasts = this.toasts.filter((toast) => toast.id !== id);
    },

    cycleTheme() {
      this.theme = { light: "dark", dark: "system", system: "light" }[this.theme] || "system";
    },

    get themeLabel() {
      return { light: "Light theme", dark: "Dark theme", system: "System theme" }[this.theme];
    },

    toggleRail() {
      if (matchMedia("(max-width: 760px)").matches) {
        this.drawer = !this.drawer;
      } else {
        this.railOpen = !this.railOpen;
      }
    },

    syncScroll() {
      const reader = this.$refs.reader;
      const head = reader.querySelector(".session-head");
      this.compact = !!head && reader.scrollTop > head.offsetHeight;
      this.atBottom = reader.scrollHeight - reader.scrollTop - reader.clientHeight < 48;
    },

    toBottom(behavior = "instant") {
      this.$refs.reader.scrollTo({ top: this.$refs.reader.scrollHeight, behavior });
    },

    openConsent({ kind, resolve }) {
      this.consent = { kind, resolve, ...CONSENT[kind] };
      this.$refs.consent.returnValue = "";
      this.$refs.consent.showModal();
    },

    closeConsent(granted) {
      if (!this.consent) return;
      if (granted) store(`consent-${this.consent.kind}`, "granted");
      this.consent.resolve(granted);
      this.consent = null;
    },

    finishOnboarding() {
      store("onboarded", "yes");
      this.$refs.onboarding.close();
    },

    async pickFiles() {
      if (await askConsent("files")) document.getElementById("pick-files").click();
    },

    async pickFolder() {
      if (await askConsent("files")) document.getElementById("pick-folder").click();
    },

    stage(files, recorded = 0) {
      const repeated = [];
      for (const file of files) {
        if (this.staged.some((item) => item.file.name === file.name && item.file.size === file.size)) {
          repeated.push(file.name);
          continue;
        }
        const item = { id: Date.now() + Math.random(), file, recorded };
        this.staged.push(item);
        trayStore("readwrite", (store) => store.put(item)).catch((error) => console.warn("Couldn't keep a staged file", error));
      }
      if (repeated.length === 1) this.notify(`${repeated[0]} is already in the list.`, "error");
      if (repeated.length > 1) this.notify(`${repeated.length} of those files are already in the list.`, "error");
    },

    unstage(id) {
      if (this.previewing === id) this.stopPreview();
      this.staged = this.staged.filter((item) => item.id !== id);
      trayStore("readwrite", (store) => store.delete(id)).catch((error) => console.warn("Couldn't forget a staged file", error));
    },

    togglePreview(item) {
      const same = this.previewing === item.id;
      this.stopPreview();
      if (same) return;
      this.preview = new Audio(URL.createObjectURL(item.file));
      this.preview.onended = () => this.stopPreview();
      this.preview.onerror = () => {
        this.stopPreview();
        this.notify("This file can't be previewed here. It can still be transcribed.", "error");
      };
      this.previewing = item.id;
      document.querySelectorAll("audio").forEach((audio) => audio.pause());
      this.preview.play().catch(() => {});
    },

    stopPreview() {
      if (this.preview) {
        this.preview.pause();
        URL.revokeObjectURL(this.preview.src);
      }
      this.preview = null;
      this.previewing = null;
    },

    send() {
      this.stopPreview();
      const transfer = new DataTransfer();
      this.staged.forEach((item) => transfer.items.add(item.file));
      document.getElementById("send-files").files = transfer.files;
      htmx.trigger("#upload", "submit");
    },

    async drop(event) {
      this.depth = 0;
      const folders = [...event.dataTransfer.items].some((item) => item.webkitGetAsEntry?.()?.isDirectory);
      if (folders) {
        this.notify("Folders can't be dropped. Use the folder button in the add bar to convert a whole folder.", "error");
        return;
      }
      const files = [...event.dataTransfer.files];
      if (!files.length || !(await askConsent("files"))) return;
      this.stage(files);
    },
  };
}

const UNREACHABLE = `Can't reach the transcription engine. Restart ${VN.name} and try again.`;

const CONSENT = {
  mic: {
    title: "Use your microphone?",
    body: `${VN.name} can record audio straight from your microphone.`,
    points: [
      "It only listens while the red recording dot is showing.",
      "Recordings are saved and transcribed on this computer. Nothing is uploaded.",
      "You can delete any recording from its session.",
    ],
    allow: "Allow microphone",
  },
  files: {
    title: "Add files from your computer?",
    body: `${VN.name} keeps its own copy of the audio you pick or drop, so it can play and transcribe it.`,
    points: [
      "Your original files are never changed or moved.",
      `Copies are kept in the app's private folder (${window.VN?.dataDir ?? "your data folder"}).`,
      "Deleting a session deletes its copies too.",
    ],
    allow: "Allow file access",
  },
};

function askConsent(kind) {
  if (stored(`consent-${kind}`, "") === "granted") return Promise.resolve(true);
  return new Promise((resolve) => window.dispatchEvent(new CustomEvent("consent-request", { detail: { kind, resolve } })));
}

function applyTheme(theme) {
  if (theme === "light" || theme === "dark") {
    document.documentElement.dataset.theme = theme;
  } else {
    delete document.documentElement.dataset.theme;
  }
}

function reportLink(email, version) {
  const body = [
    "What happened:",
    "",
    "What you expected:",
    "",
    "File type (if it's about a recording):",
    "",
    "---",
    `${VN.name} ${version}`,
    navigator.userAgent,
  ].join("\n");
  return `mailto:${email}?subject=${encodeURIComponent(`${VN.name} issue (${version})`)}&body=${encodeURIComponent(body)}`;
}

function micError(error) {
  if (error.name === "NotAllowedError") return "Microphone access is blocked. Allow it for this site in the address bar, then try again.";
  if (error.name === "NotFoundError") return "No microphone was found. Plug one in and try again.";
  if (error.name === "NotReadableError") return "The microphone is busy in another app. Close it there and try again.";
  return `Couldn't start recording: ${error.message}`;
}

function recordingName(extension) {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  return `Recording ${date} at ${pad(now.getHours())}.${pad(now.getMinutes())}.${pad(now.getSeconds())}.${extension}`;
}

function recorder() {
  return {
    recording: false,
    paused: false,
    seconds: 0,
    levels: [],
    stream: null,
    chunks: [],
    context: null,
    analyser: null,
    capture: null,
    timer: null,
    frame: null,
    lastSample: 0,

    async start() {
      if (!navigator.mediaDevices?.getUserMedia || !window.AudioWorkletNode) {
        toast("This browser can't record audio.");
        return;
      }
      if (!(await askConsent("mic"))) return;
      try {
        this.stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      } catch (error) {
        toast(micError(error));
        return;
      }
      this.context = new AudioContext();
      await this.context.audioWorklet.addModule("/static/recorder-worklet.js");
      const source = this.context.createMediaStreamSource(this.stream);
      this.analyser = this.context.createAnalyser();
      this.analyser.fftSize = 1024;
      this.capture = new AudioWorkletNode(this.context, "capture");
      this.capture.port.onmessage = (event) => this.paused || this.chunks.push(event.data);
      const silent = this.context.createGain();
      silent.gain.value = 0;
      source.connect(this.analyser);
      source.connect(this.capture).connect(silent).connect(this.context.destination);

      this.chunks = [];
      this.levels = [];
      this.seconds = 0;
      this.paused = false;
      this.recording = true;
      this.timer = setInterval(() => this.paused || this.seconds++, 1000);
      this.$nextTick(() => this.draw());
    },

    togglePause() {
      this.paused = !this.paused;
    },

    finish() {
      const wav = encodeWav(this.chunks, this.context.sampleRate);
      this.teardown();
      if (!wav) {
        toast("Nothing was recorded. Check your microphone and try again.");
        return;
      }
      const file = new File([wav], recordingName("wav"), { type: "audio/wav" });
      window.dispatchEvent(new CustomEvent("stage", { detail: { file, seconds: this.seconds } }));
    },

    cancel() {
      this.teardown();
    },

    teardown() {
      clearInterval(this.timer);
      cancelAnimationFrame(this.frame);
      this.capture?.port.close();
      this.stream?.getTracks().forEach((track) => track.stop());
      this.context?.close();
      this.recording = false;
    },

    draw() {
      const canvas = this.$refs.wave;
      if (!this.recording || !canvas) return;
      const ratio = devicePixelRatio || 1;
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      if (canvas.width !== Math.round(width * ratio)) {
        canvas.width = Math.round(width * ratio);
        canvas.height = Math.round(height * ratio);
      }
      const now = performance.now();
      if (!this.paused && now - this.lastSample > 70) {
        const data = new Uint8Array(this.analyser.fftSize);
        this.analyser.getByteTimeDomainData(data);
        let sum = 0;
        for (const value of data) sum += ((value - 128) / 128) ** 2;
        this.levels.push(Math.min(1, Math.sqrt(sum / data.length) * 5));
        this.lastSample = now;
      }

      const ctx = canvas.getContext("2d");
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
      ctx.clearRect(0, 0, width, height);
      ctx.fillStyle = getComputedStyle(canvas).color;
      const step = 6;
      const count = Math.floor(width / step);
      const recent = this.levels.slice(-count);
      for (let i = 0; i < count; i++) {
        const level = recent[i - (count - recent.length)];
        const bar = level === undefined ? 3 : Math.max(3, level * height);
        ctx.globalAlpha = level === undefined ? 0.25 : 1;
        ctx.beginPath();
        ctx.roundRect(i * step, (height - bar) / 2, 3, bar, 1.5);
        ctx.fill();
      }
      this.frame = requestAnimationFrame(() => this.draw());
    },
  };
}

const WAV_RATE = 16000;

function encodeWav(chunks, rate) {
  const length = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const count = Math.floor((length * WAV_RATE) / rate);
  if (!count) return null;
  const input = new Float32Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    input.set(chunk, offset);
    offset += chunk.length;
  }
  const view = new DataView(new ArrayBuffer(44 + count * 2));
  const text = (at, value) => [...value].forEach((c, i) => view.setUint8(at + i, c.charCodeAt(0)));
  text(0, "RIFF");
  view.setUint32(4, 36 + count * 2, true);
  text(8, "WAVEfmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, WAV_RATE, true);
  view.setUint32(28, WAV_RATE * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, count * 2, true);
  const step = rate / WAV_RATE;
  for (let i = 0; i < count; i++) {
    const at = i * step;
    const low = Math.floor(at);
    const high = Math.min(low + 1, length - 1);
    const sample = input[low] + (input[high] - input[low]) * (at - low);
    view.setInt16(44 + i * 2, Math.max(-1, Math.min(1, sample)) * 0x7fff, true);
  }
  return view.buffer;
}

function stored(key, fallback) {
  try {
    return localStorage.getItem(`${VN.id}-${key}`) || fallback;
  } catch {
    return fallback;
  }
}

function store(key, value) {
  try {
    localStorage.setItem(`${VN.id}-${key}`, value);
  } catch {}
}

let trayDatabase;

function trayStore(mode, action) {
  trayDatabase ??= new Promise((resolve, reject) => {
    const request = indexedDB.open(VN.id, 1);
    request.onupgradeneeded = () => request.result.createObjectStore("staged", { keyPath: "id" });
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  return trayDatabase.then(
    (db) =>
      new Promise((resolve, reject) => {
        const request = action(db.transaction("staged", mode).objectStore("staged"));
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      }),
  );
}

function fileSize(bytes) {
  if (bytes < 1_000_000) return `${Math.max(1, Math.round(bytes / 1000))} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

function clock(seconds) {
  const s = Math.max(0, Math.floor(seconds || 0));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function errorMessage(xhr) {
  try {
    const detail = JSON.parse(xhr.responseText).detail;
    if (typeof detail === "string") return detail;
  } catch {}
  if (xhr.status === 404) return "That item no longer exists.";
  if (xhr.status >= 500) return `Something went wrong in the transcription engine. Details are in ${window.VN?.logFile ?? "the log file"}.`;
  return `Request failed (${xhr.status}).`;
}

function toast(message, kind = "error") {
  window.dispatchEvent(new CustomEvent("toast", { detail: { message, kind } }));
}

window.downloaded = (path, name) => {
  const action = { label: "Show in folder", run: () => window.__TAURI__.opener.revealItemInDir(path) };
  window.dispatchEvent(new CustomEvent("notify", { detail: { message: `Saved ${name} to Downloads`, kind: "success", action } }));
};

function picker(options, value) {
  return {
    entries: Object.entries(options),
    value,
    open: false,

    text() {
      return options[this.value] ?? this.value;
    },

    toggle() {
      this.open = !this.open;
      if (this.open) this.$nextTick(() => this.$el.querySelector('[aria-selected="true"]')?.focus());
    },

    choose(key) {
      this.value = key;
      this.open = false;
      this.$refs.button.focus();
    },

    move(step) {
      const options = [...this.$el.querySelectorAll(".picker-option")];
      const at = options.indexOf(document.activeElement);
      options[(at + step + options.length) % options.length]?.focus();
    },
  };
}

const SELECT_ALL_SCOPE = ".transcript, .status-hint, [role=alert], .toast, .banner, .about-facts";

function selectWithin(event) {
  if (event.target.closest("input, textarea, [contenteditable]")) return;
  event.preventDefault();
  const selection = window.getSelection();
  const scope = selection.anchorNode?.parentElement?.closest(SELECT_ALL_SCOPE);
  if (scope) selection.selectAllChildren(scope);
}

function chooseModelFolder() {
  return window.__TAURI__.dialog.open({ directory: true, title: "Choose a model folder" });
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    toast("Couldn't copy to the clipboard. Your browser blocked it.");
    return false;
  }
}

async function copySession(id) {
  try {
    const response = await fetch(`/sessions/${id}/download`);
    if (!response.ok) throw new Error(errorMessage({ status: response.status, responseText: await response.text() }));
    return copyText(await response.text());
  } catch (error) {
    toast(error instanceof TypeError ? UNREACHABLE : error.message);
    return false;
  }
}

const resumePoints = new Map();

function reader(id, plain, stamped, duration) {
  return {
    plain,
    stamped,
    duration,
    now: 0,
    playing: false,
    copied: null,
    confirming: false,

    init() {
      const audio = this.$refs.audio;
      const resumeAt = resumePoints.get(id);
      resumePoints.delete(id);
      if (!audio || resumeAt === undefined) return;
      const resume = () => {
        if (audio.currentTime >= resumeAt - 0.5 && !audio.paused) return;
        audio.currentTime = resumeAt;
        this.play();
      };
      audio.addEventListener("loadedmetadata", resume, { once: true });
      if (audio.readyState >= 1) resume();
    },

    async copy(text, which) {
      if (!(await copyText(text))) return;
      this.copied = which;
      setTimeout(() => (this.copied = null), 1400);
    },

    toggle() {
      const audio = this.$refs.audio;
      audio.paused ? this.play() : audio.pause();
    },

    seek(seconds) {
      this.$refs.audio.currentTime = seconds;
      this.play();
    },

    play() {
      document.querySelectorAll("audio").forEach((other) => other !== this.$refs.audio && other.pause());
      this.$refs.audio.play().catch(() => {});
    },

    audioFailed() {
      toast("Your browser can't play this recording's format. The transcript still works.");
    },
  };
}
