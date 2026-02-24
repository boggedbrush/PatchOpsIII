const themeStorageKey = "patchopsiii-theme";

function storageGet(key) {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storageSet(key, value) {
  try {
    window.localStorage.setItem(key, value);
    return true;
  } catch {
    return false;
  }
}

function getSystemTheme() {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function setThemeColor(theme) {
  const meta = document.querySelector('meta[name="theme-color"]');
  if (!meta) return;
  meta.setAttribute("content", theme === "light" ? "#ffffff" : "#0d0d0f");
}

function applyTheme(theme, animate = false) {
  document.documentElement.dataset.theme = theme;
  setThemeColor(theme);

  document.querySelectorAll("[data-theme-toggle]").forEach((button) => {
    button.setAttribute("aria-pressed", theme === "dark" ? "true" : "false");
    button.setAttribute("title", theme === "dark" ? "Switch to light theme" : "Switch to dark theme");

    // Only animate if user explicitly changed theme, not on initial load
    if (animate) {
      button.setAttribute("data-transitioning", "true");
      setTimeout(() => button.removeAttribute("data-transitioning"), 500);
    }
  });
}

function initTheme() {
  const stored = storageGet(themeStorageKey);
  applyTheme(stored || getSystemTheme(), false);

  const media = window.matchMedia("(prefers-color-scheme: light)");
  const onChange = () => {
    if (storageGet(themeStorageKey)) return;
    applyTheme(getSystemTheme(), false);
  };

  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", onChange);
  } else if (typeof media.addListener === "function") {
    media.addListener(onChange);
  }

  document.querySelectorAll("[data-theme-toggle]").forEach((button) => {
    button.addEventListener("click", () => {
      const next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
      storageSet(themeStorageKey, next);
      applyTheme(next, true);
    });
  });
}

function initYear() {
  const year = String(new Date().getFullYear());
  document.querySelectorAll("[data-year]").forEach((el) => (el.textContent = year));
}

function initNav() {
  const toggles = Array.from(document.querySelectorAll("[data-nav-toggle]"));
  const overlay = document.querySelector("[data-nav-overlay]");
  const nav = document.getElementById("site-nav");

  if (toggles.length === 0 || !overlay || !nav) return;

  function setOpen(isOpen) {
    document.body.dataset.navOpen = isOpen ? "true" : "false";
    toggles.forEach((button) => {
      button.setAttribute("aria-expanded", isOpen ? "true" : "false");
      if (!button.classList.contains("nav-close")) {
        button.setAttribute("aria-label", isOpen ? "Close menu" : "Open menu");
      }
    });
    overlay.hidden = !isOpen;
    document.body.style.overflow = isOpen ? "hidden" : "";
  }

  toggles.forEach((button) => {
    button.addEventListener("click", () => {
      const isOpen = document.body.dataset.navOpen === "true";
      setOpen(!isOpen);
    });
  });

  overlay.addEventListener("click", () => setOpen(false));

  nav.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target.closest("a") : null;
    if (target) setOpen(false);
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") setOpen(false);
  });

  window.addEventListener("resize", () => {
    if (window.matchMedia("(min-width: 901px)").matches) setOpen(false);
  });

  setOpen(false);
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    try {
      const el = document.createElement("textarea");
      el.value = text;
      el.setAttribute("readonly", "true");
      el.style.position = "fixed";
      el.style.left = "-9999px";
      document.body.appendChild(el);
      el.select();
      const ok = document.execCommand("copy");
      document.body.removeChild(el);
      return ok;
    } catch {
      return false;
    }
  }
}

function initCopyButtons() {
  document.querySelectorAll("[data-copy]").forEach((button) => {
    if (button.dataset.copyBound === "true") return;
    button.dataset.copyBound = "true";

    button.addEventListener("click", async () => {
      const selector = button.getAttribute("data-copy");
      if (!selector) return;

      const target = document.querySelector(selector);
      if (!target) return;

      const text = target.textContent || "";
      const ok = await copyText(text.trim());

      const previous = button.textContent;
      button.textContent = ok ? "Copied" : "Copy failed";
      button.disabled = true;

      window.setTimeout(() => {
        button.textContent = previous;
        button.disabled = false;
      }, 1100);
    });
  });
}

function safeText(value) {
  if (typeof value !== "string") return "";
  return value;
}

function isSafeHref(href) {
  const value = safeText(href).trim();
  if (!value) return false;
  if (value.startsWith("/")) return true;
  return /^(https?:\/\/|mailto:)/i.test(value);
}

function appendInlineMarkdown(container, sourceText) {
  const text = safeText(sourceText);
  if (!text) return;

  const tokenRe = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*|\[[^\]]+\]\([^)]+\))/g;
  let lastIndex = 0;
  let match;

  while ((match = tokenRe.exec(text)) !== null) {
    if (match.index > lastIndex) {
      container.appendChild(document.createTextNode(text.slice(lastIndex, match.index)));
    }

    const token = match[0];

    if (token.startsWith("`") && token.endsWith("`")) {
      const code = document.createElement("code");
      code.textContent = token.slice(1, -1);
      container.appendChild(code);
    } else if (token.startsWith("**") && token.endsWith("**")) {
      const strong = document.createElement("strong");
      strong.textContent = token.slice(2, -2);
      container.appendChild(strong);
    } else if (token.startsWith("*") && token.endsWith("*")) {
      const em = document.createElement("em");
      em.textContent = token.slice(1, -1);
      container.appendChild(em);
    } else if (token.startsWith("[")) {
      const linkMatch = token.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
      if (linkMatch && isSafeHref(linkMatch[2])) {
        const a = document.createElement("a");
        a.textContent = linkMatch[1];
        a.href = linkMatch[2].trim();
        a.rel = "noreferrer";
        container.appendChild(a);
      } else {
        container.appendChild(document.createTextNode(token));
      }
    } else {
      container.appendChild(document.createTextNode(token));
    }

    lastIndex = tokenRe.lastIndex;
  }

  if (lastIndex < text.length) {
    container.appendChild(document.createTextNode(text.slice(lastIndex)));
  }
}

function setText(selector, text) {
  document.querySelectorAll(selector).forEach((el) => {
    el.textContent = text;
  });
}

function setHref(selector, href) {
  document.querySelectorAll(selector).forEach((el) => {
    if (!(el instanceof HTMLAnchorElement)) return;
    if (!href) return;
    el.href = href;
  });
}

function normalizeHeaderTitle(text) {
  return safeText(text).replace(/[^\w\s]/g, "").trim().toLowerCase();
}

function extractSection(markdown, headerName) {
  const body = safeText(markdown);
  if (!body) return [];
  const want = normalizeHeaderTitle(headerName);

  const lines = body.split(/\r?\n/);
  let start = -1;
  for (let i = 0; i < lines.length; i += 1) {
    const match = lines[i].match(/^\s*#{1,6}\s+(.*)$/);
    if (!match) continue;
    const title = normalizeHeaderTitle(match[1] || "");
    if (title === want) {
      start = i + 1;
      break;
    }
  }

  if (start === -1) return [];
  const out = [];
  for (let i = start; i < lines.length; i += 1) {
    if (/^\s*#{1,6}\s+/.test(lines[i])) break;
    out.push(lines[i]);
  }
  return out;
}

function extractOverview(markdown) {
  const lines = extractSection(markdown, "Overview");
  if (!lines.length) return "";

  const paragraph = [];
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) {
      if (paragraph.length) break;
      continue;
    }
    if (trimmed.startsWith("-") || trimmed.startsWith("*")) {
      if (paragraph.length) break;
      continue;
    }
    paragraph.push(trimmed);
  }
  return paragraph.join(" ").trim();
}

function extractHighlights(markdown) {
  let lines = extractSection(markdown, "Major Highlights");
  if (!lines.length) lines = extractSection(markdown, "Major highlights");
  if (!lines.length) lines = extractSection(markdown, "🚀 Major Highlights");

  const out = [];
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) {
      if (out.length) break;
      continue;
    }
    if (trimmed.startsWith("- ") || trimmed.startsWith("* ")) {
      const item = trimmed.slice(2).trim();
      if (item) out.push(item);
    }
  }
  return out.slice(0, 5);
}

function pickAssetUrl(assets, exactName, suffixes) {
  if (!Array.isArray(assets)) return "";
  for (const asset of assets) {
    if (safeText(asset?.name) === exactName) return safeText(asset?.browser_download_url);
  }
  for (const asset of assets) {
    const name = safeText(asset?.name);
    if (suffixes.some((sfx) => name.endsWith(sfx))) return safeText(asset?.browser_download_url);
  }
  return "";
}

function mapRelease(repoSlug, rel) {
  const tag = safeText(rel?.tag_name);
  const url = safeText(rel?.html_url) || (tag ? `https://github.com/${repoSlug}/releases/tag/${tag}` : "");
  const body = safeText(rel?.body);
  const assets = Array.isArray(rel?.assets) ? rel.assets : [];
  const isPrerelease = Boolean(rel?.prerelease);

  const fallbackBase = `https://github.com/${repoSlug}/releases/latest/download`;

  return {
    tag,
    url,
    isPrerelease,
    overview: extractOverview(body),
    highlights: extractHighlights(body),
    downloads: {
      windows: {
        url: pickAssetUrl(assets, "PatchOpsIII.exe", [".exe"]) || `${fallbackBase}/PatchOpsIII.exe`,
      },
      linux: {
        url: pickAssetUrl(assets, "PatchOpsIII.AppImage", [".AppImage"]) || `${fallbackBase}/PatchOpsIII.AppImage`,
      },
    },
  };
}

function renderChangelog(releases, currentStableTag) {
  const root = document.querySelector("[data-changelog]");
  if (!root) return;
  if (!Array.isArray(releases) || releases.length === 0) return;

  const frag = document.createDocumentFragment();

  releases.forEach((rel) => {
    const tag = safeText(rel?.tag);
    const url = safeText(rel?.url);
    const overview = safeText(rel?.overview);
    const isPrerelease = Boolean(rel?.isPrerelease);
    const isCurrentStable = Boolean(tag && currentStableTag && tag === currentStableTag);
    const highlights = Array.isArray(rel?.highlights) ? rel.highlights.slice(0, 3).map(safeText).filter(Boolean) : [];

    const item = document.createElement("article");
    item.className = "timeline-item";
    item.setAttribute("role", "listitem");

    const dot = document.createElement("div");
    dot.className = "timeline-dot";
    dot.setAttribute("aria-hidden", "true");

    const card = document.createElement("div");
    card.className = "timeline-card";

    const head = document.createElement("div");
    head.className = "timeline-head";
    const h2 = document.createElement("h2");
    h2.textContent = tag || "Release";
    head.appendChild(h2);

    if (isCurrentStable) {
      const current = document.createElement("span");
      current.className = "tag";
      current.textContent = "Current";
      head.appendChild(current);
    }

    if (isPrerelease) {
      const beta = document.createElement("span");
      beta.className = "tag";
      beta.textContent = "Beta";
      head.appendChild(beta);
    }

    card.appendChild(head);

    if (overview) {
      const p = document.createElement("p");
      p.className = "muted";
      appendInlineMarkdown(p, overview);
      card.appendChild(p);
    }

    if (highlights.length) {
      const ul = document.createElement("ul");
      ul.className = "checklist";
      highlights.forEach((text) => {
        const li = document.createElement("li");
        appendInlineMarkdown(li, text);
        ul.appendChild(li);
      });
      card.appendChild(ul);
    }

    const cta = document.createElement("div");
    cta.className = "cta-row";

    const a = document.createElement("a");
    a.className = "btn subtle";
    a.rel = "noreferrer";
    a.href = url || "https://github.com/boggedbrush/PatchOpsIII/releases";
    a.textContent = "Release";
    cta.appendChild(a);

    if (isCurrentStable) {
      const download = document.createElement("a");
      download.className = "btn subtle";
      download.href = "/download/";
      download.textContent = "Download";
      cta.appendChild(download);
    }

    card.appendChild(cta);

    item.appendChild(dot);
    item.appendChild(card);
    frag.appendChild(item);
  });

  root.replaceChildren(frag);
}

const ghRepoSlug = "boggedbrush/PatchOpsIII";
const ghReleasesUrl = `https://api.github.com/repos/${ghRepoSlug}/releases?per_page=30`;
const releasesCacheKey = "patchopsiii-gh-releases-v2";
const releasesCacheTtlMs = 15 * 60 * 1000;

function loadReleasesCache() {
  try {
    const raw = window.localStorage.getItem(releasesCacheKey);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return null;
    if (!Array.isArray(parsed.releases)) return null;
    if (typeof parsed.savedAt !== "number") return null;
    return parsed;
  } catch {
    return null;
  }
}

function saveReleasesCache(releases) {
  try {
    window.localStorage.setItem(releasesCacheKey, JSON.stringify({ savedAt: Date.now(), releases }));
  } catch {
    // ignore
  }
}

function isStableRelease(rel) {
  return !rel?.draft && !rel?.prerelease;
}

async function fetchReleases() {
  const resp = await fetch(ghReleasesUrl, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (!resp.ok) throw new Error(`GitHub fetch failed: ${resp.status}`);
  const data = await resp.json();
  if (!Array.isArray(data)) return [];

  const ordered = data.slice();
  ordered.sort((a, b) => safeText(b?.published_at).localeCompare(safeText(a?.published_at)));
  return ordered;
}

function applyReleaseData(mappedLatest) {
  const tag = safeText(mappedLatest?.tag);
  const url = safeText(mappedLatest?.url);
  const winUrl = safeText(mappedLatest?.downloads?.windows?.url);
  const linuxUrl = safeText(mappedLatest?.downloads?.linux?.url);

  if (tag) setText("[data-latest-version]", tag);
  if (url) setHref("[data-latest-release-url]", url);
  if (winUrl) setHref("[data-download-windows]", winUrl);
  if (linuxUrl) setHref("[data-download-linux]", linuxUrl);
}

function applyChangelog(mappedReleases, currentStableTag) {
  if (document.body.dataset.page !== "changelog") return;
  renderChangelog(mappedReleases, currentStableTag);
}

async function initReleaseData() {
  const cached = loadReleasesCache();
  const isCacheFresh = cached && Date.now() - cached.savedAt < releasesCacheTtlMs;

  if (cached?.releases?.length) {
    const stable = cached.releases.filter(isStableRelease);
    const latestStable = stable[0] || null;
    const currentStableTag = safeText(latestStable?.tag_name);
    const mappedLatest = latestStable ? mapRelease(ghRepoSlug, latestStable) : null;

    if (mappedLatest) applyReleaseData(mappedLatest);

    const mappedChangelog = cached.releases.slice(0, 15).map((r) => mapRelease(ghRepoSlug, r));
    if (mappedChangelog.length) applyChangelog(mappedChangelog, currentStableTag);
  }

  if (isCacheFresh) return;

  try {
    const releases = await fetchReleases();
    saveReleasesCache(releases);

    const stable = releases.filter(isStableRelease);
    const latestStable = stable[0] || null;
    const currentStableTag = safeText(latestStable?.tag_name);
    const mappedLatest = latestStable ? mapRelease(ghRepoSlug, latestStable) : null;
    if (mappedLatest) applyReleaseData(mappedLatest);

    const mappedChangelog = releases.slice(0, 15).map((r) => mapRelease(ghRepoSlug, r));
    if (mappedChangelog.length) applyChangelog(mappedChangelog, currentStableTag);
  } catch {
    // Keep static fallbacks if GitHub API isn't available.
  }
}

function getLightboxController() {
  if (window.__patchopsLightboxController) return window.__patchopsLightboxController;

  const lightbox = document.createElement("div");
  lightbox.className = "lightbox";
  lightbox.setAttribute("role", "dialog");
  lightbox.setAttribute("aria-modal", "true");
  lightbox.setAttribute("aria-label", "Screenshot preview");

  const inner = document.createElement("div");
  inner.className = "lightbox-inner";

  const img = document.createElement("img");
  img.className = "lightbox-img";
  img.alt = "";

  const closeBtn = document.createElement("button");
  closeBtn.className = "lightbox-close";
  closeBtn.setAttribute("aria-label", "Close preview");
  closeBtn.textContent = "×";

  inner.appendChild(img);
  inner.appendChild(closeBtn);
  lightbox.appendChild(inner);
  document.body.appendChild(lightbox);

  let previousFocus = null;

  function open(src, alt) {
    img.src = src;
    img.alt = alt || "";
    previousFocus = document.activeElement;
    lightbox.classList.add("is-open");
    document.body.style.overflow = "hidden";
    closeBtn.focus();
  }

  function close() {
    lightbox.classList.remove("is-open");
    document.body.style.overflow = "";
    if (previousFocus instanceof HTMLElement) previousFocus.focus();
  }

  closeBtn.addEventListener("click", close);
  lightbox.addEventListener("click", (e) => {
    if (e.target === lightbox) close();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && lightbox.classList.contains("is-open")) close();
  });

  const controller = { open };
  window.__patchopsLightboxController = controller;
  return controller;
}

function initLightbox() {
  const frames = Array.from(document.querySelectorAll(".media-frame"));
  if (frames.length === 0) return;
  const lightbox = getLightboxController();

  frames.forEach((frame) => {
    if (frame.dataset.lightboxBound === "true") return;
    frame.dataset.lightboxBound = "true";

    const frameImg = frame.querySelector("img");
    if (!frameImg) return;

    frame.setAttribute("tabindex", "0");
    frame.setAttribute("role", "button");
    frame.setAttribute("aria-label", `View full size: ${frameImg.alt || "screenshot"}`);

    frame.addEventListener("click", () => lightbox.open(frameImg.src, frameImg.alt));
    frame.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        lightbox.open(frameImg.src, frameImg.alt);
      }
    });

    const ZOOM = 1.2;

    frame.addEventListener("mouseenter", () => {
      frameImg.style.transform = `translate(0px, 0px) scale(${ZOOM})`;
    });

    frame.addEventListener("mousemove", (e) => {
      const rect = frame.getBoundingClientRect();
      const renderedImgWidth = frame.clientWidth;
      const renderedImgHeight =
        frameImg.naturalWidth > 0
          ? renderedImgWidth * (frameImg.naturalHeight / frameImg.naturalWidth)
          : renderedImgWidth * (838 / 1150);

      const maxOffsetX = Math.max(0, renderedImgWidth * ZOOM - frame.clientWidth);
      const maxOffsetY = Math.max(0, renderedImgHeight * ZOOM - frame.clientHeight);

      const xFraction = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
      const yFraction = Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));

      const offsetX = xFraction * maxOffsetX;
      const offsetY = yFraction * maxOffsetY;
      frameImg.style.transform = `translate(${-offsetX}px, ${-offsetY}px) scale(${ZOOM})`;
    });

    frame.addEventListener("mouseleave", () => {
      frameImg.style.transform = "";
    });
  });
}

function initScrollReveal() {
  if (!("IntersectionObserver" in window)) return;

  const io = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-visible");
          io.unobserve(entry.target);
        }
      });
    },
    { threshold: 0.08, rootMargin: "0px 0px -40px 0px" }
  );

  const tagged = [];

  function tag(el, dir) {
    el.dataset.reveal = dir || "up";
    tagged.push(el);
  }

  function tagStagger(el) {
    el.dataset.stagger = "";
    tagged.push(el);
  }

  document.querySelectorAll(".section-title, .section-subtitle").forEach((el) => tag(el));
  document.querySelectorAll(".section-header-row").forEach((el) => tag(el));

  document.querySelectorAll(".feature-split:not(.reverse) .feature-copy").forEach((el) => tag(el, "left"));
  document.querySelectorAll(".feature-split:not(.reverse) .feature-media").forEach((el) => tag(el, "right"));
  document.querySelectorAll(".feature-split.reverse .feature-copy").forEach((el) => tag(el, "right"));
  document.querySelectorAll(".feature-split.reverse .feature-media").forEach((el) => tag(el, "left"));

  document.querySelectorAll(".cta-card").forEach((el) => tag(el, "scale"));
  document.querySelectorAll(".grid.cards, .grid.gallery, .timeline").forEach((el) => tagStagger(el));

  requestAnimationFrame(() => {
    tagged.forEach((el) => {
      const rect = el.getBoundingClientRect();
      const alreadyVisible = rect.top < window.innerHeight - 40 && rect.bottom > 0;
      if (alreadyVisible) {
        // Reveal instantly — no animation for elements visible on load
        el.style.transition = "none";
        el.classList.add("is-visible");
        requestAnimationFrame(() => {
          el.style.transition = "";
        });
      } else {
        io.observe(el);
      }
    });
  });
}

function initHeaderScroll() {
  const header = document.querySelector("[data-header]");
  if (!header) return;

  function update() {
    header.classList.toggle("is-scrolled", window.scrollY > 16);
  }

  window.addEventListener("scroll", update, { passive: true });
  update();
}

function initImageLoading() {
  const frames = Array.from(document.querySelectorAll(".media-frame"));
  if (frames.length === 0) return;

  frames.forEach((frame) => {
    if (frame.dataset.imageLoadingBound === "true") return;
    frame.dataset.imageLoadingBound = "true";

    const img = frame.querySelector("img");
    if (!img) return;

    // Create loading placeholder
    const loading = document.createElement("div");
    loading.className = "media-loading";
    const shimmer = document.createElement("div");
    shimmer.className = "media-shimmer";
    loading.appendChild(shimmer);
    frame.insertBefore(loading, img);

    const startTime = Date.now();
    const minDisplayTime = 600; // Minimum time to show loading animation

    // Handle image load
    const completeLoading = () => {
      const elapsed = Date.now() - startTime;
      const delay = Math.max(0, minDisplayTime - elapsed);

      setTimeout(() => {
        img.classList.add("is-loaded");
        loading.classList.add("is-complete");
      }, delay);
    };

    // Handle error state
    const onError = () => {
      completeLoading();
    };

    if (img.complete) {
      // Image already cached/loaded, but still show loading briefly
      requestAnimationFrame(() => completeLoading());
    } else {
      img.addEventListener("load", completeLoading, { once: true });
      img.addEventListener("error", onError, { once: true });
    }
  });
}

const clientNavigablePaths = new Set(["/", "/features/", "/download/", "/docs/", "/changelog/"]);
let currentNavRequestId = 0;

function normalizePathname(pathname) {
  if (!pathname) return "/";
  let out = pathname.endsWith("/") ? pathname : `${pathname}/`;
  out = out.replace(/\/index\.html\/$/i, "/");
  return out;
}

function updateActiveNav(pathname) {
  const current = normalizePathname(pathname);
  document.querySelectorAll("#site-nav a[href^='/']").forEach((a) => {
    if (!(a instanceof HTMLAnchorElement)) return;
    const target = normalizePathname(new URL(a.href, window.location.origin).pathname);
    if (target === current) {
      a.setAttribute("aria-current", "page");
    } else {
      a.removeAttribute("aria-current");
    }
  });
}

function initPageContent() {
  initCopyButtons();
  initImageLoading();
  initLightbox();
  initReleaseData();
  initScrollReveal();
  updateActiveNav(window.location.pathname);
}

async function navigateClient(url, { pushState = true } = {}) {
  const requestId = ++currentNavRequestId;
  const targetUrl = new URL(url, window.location.href);

  try {
    const response = await fetch(targetUrl.href, {
      method: "GET",
      credentials: "same-origin",
      headers: { "X-Requested-With": "PatchOpsIIINav" },
    });
    if (!response.ok) {
      window.location.href = targetUrl.href;
      return;
    }

    const html = await response.text();
    if (requestId !== currentNavRequestId) return;

    const nextDoc = new DOMParser().parseFromString(html, "text/html");
    const nextMain = nextDoc.querySelector("main");
    const currentMain = document.querySelector("main");
    if (!nextMain || !currentMain) {
      window.location.href = targetUrl.href;
      return;
    }

    const nextBodyPage = nextDoc.body?.dataset?.page || "";
    if (nextBodyPage) document.body.dataset.page = nextBodyPage;
    document.title = nextDoc.title || document.title;

    // Update the URL before swapping content so relative asset paths resolve correctly.
    if (pushState) {
      window.history.pushState({ path: targetUrl.pathname + targetUrl.search }, "", targetUrl.pathname + targetUrl.search);
    }

    currentMain.replaceWith(nextMain);
    window.scrollTo({ top: 0, left: 0, behavior: "auto" });

    document.body.dataset.navOpen = "false";
    document.body.style.overflow = "";
    const overlay = document.querySelector("[data-nav-overlay]");
    if (overlay) overlay.hidden = true;

    initPageContent();
  } catch {
    window.location.href = targetUrl.href;
  }
}

function initClientNavigation() {
  document.addEventListener("click", (event) => {
    if (event.defaultPrevented) return;
    if (event.button !== 0) return;
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;

    const link = event.target instanceof Element ? event.target.closest("a") : null;
    if (!(link instanceof HTMLAnchorElement)) return;
    if (link.target && link.target !== "_self") return;
    if (link.hasAttribute("download")) return;
    if (!link.href) return;

    const url = new URL(link.href, window.location.href);
    if (url.origin !== window.location.origin) return;
    if (url.hash) return;
    if (!clientNavigablePaths.has(normalizePathname(url.pathname))) return;
    if (normalizePathname(url.pathname) === normalizePathname(window.location.pathname)) return;

    event.preventDefault();
    navigateClient(`${url.pathname}${url.search}`);
  });

  window.addEventListener("popstate", () => {
    navigateClient(`${window.location.pathname}${window.location.search}`, { pushState: false });
  });
}

initYear();
initTheme();
initNav();
initHeaderScroll();
initClientNavigation();
initPageContent();
