(() => {
  const toolbarSelector = "#toolbar-shell";
  const contentSelector = "#content-root";

  function closeFullscreenPreview() {
    const active = document.querySelector("[data-preview-panel].is-fullscreen");
    if (!active) return;
    active.classList.remove("is-fullscreen");
    document.body.classList.remove("preview-fullscreen");
    const button = active.querySelector("[data-preview-toggle]");
    if (button) button.textContent = "展开";
  }

  function togglePreview(preview) {
    const isActive = preview.classList.contains("is-fullscreen");
    closeFullscreenPreview();
    if (isActive) return;
    preview.classList.add("is-fullscreen");
    document.body.classList.add("preview-fullscreen");
    const button = preview.querySelector("[data-preview-toggle]");
    if (button) button.textContent = "收起";
  }

  function shouldHandleLink(anchor, event) {
    if (!anchor) return false;
    if (anchor.classList.contains("download")) return false;
    if (anchor.hasAttribute("download")) return false;
    if (anchor.target && anchor.target !== "_self") return false;
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return false;

    const url = new URL(anchor.href, window.location.origin);
    return url.origin === window.location.origin;
  }

  async function navigate(url, options = {}) {
    const nextUrl = typeof url === "string" ? url : url.toString();
    const scrollX = window.scrollX;
    const scrollY = window.scrollY;
    document.body.classList.add("is-loading");
    try {
      const response = await fetch(nextUrl, {
        headers: {
          "X-Requested-With": "spa-fetch",
        },
      });
      if (!response.ok) {
        window.location.href = nextUrl;
        return;
      }

      const text = await response.text();
      const parser = new DOMParser();
      const doc = parser.parseFromString(text, "text/html");
      const nextToolbar = doc.querySelector(toolbarSelector);
      const nextContent = doc.querySelector(contentSelector);
      const currentToolbar = document.querySelector(toolbarSelector);
      const currentContent = document.querySelector(contentSelector);

      if (!nextToolbar || !nextContent || !currentToolbar || !currentContent) {
        window.location.href = nextUrl;
        return;
      }

      currentToolbar.replaceWith(nextToolbar);
      currentContent.replaceWith(nextContent);
      document.title = doc.title;

      if (options.replace) {
        window.history.replaceState({}, "", nextUrl);
      } else {
        window.history.pushState({}, "", nextUrl);
      }
      window.requestAnimationFrame(() => {
        if (options.scrollTargetSelector) {
          const target = document.querySelector(options.scrollTargetSelector);
          if (target) {
            target.scrollIntoView({ block: "nearest", inline: "nearest" });
            return;
          }
        }
        window.scrollTo(scrollX, scrollY);
      });
    } catch (_error) {
      window.location.href = nextUrl;
    } finally {
      document.body.classList.remove("is-loading");
    }
  }

  document.addEventListener("click", (event) => {
    const previewToggle = event.target.closest("[data-preview-toggle]");
    if (previewToggle) {
      const preview = previewToggle.closest("[data-preview-panel]");
      if (!preview) return;
      event.preventDefault();
      togglePreview(preview);
      return;
    }

    const fullscreenPreview = document.querySelector("[data-preview-panel].is-fullscreen");
    if (fullscreenPreview && event.target === fullscreenPreview) {
      closeFullscreenPreview();
      return;
    }

    const anchor = event.target.closest("a[href]");
    if (!shouldHandleLink(anchor, event)) {
      return;
    }
    event.preventDefault();
    const pagination = anchor.closest("[data-pagination-scope]");
    const scrollTargetSelector = pagination
      ? `[data-pagination-scope="${pagination.getAttribute("data-pagination-scope")}"]`
      : null;
    navigate(anchor.href, { scrollTargetSelector });
  });

  document.addEventListener("submit", (event) => {
    const form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    if (!form.matches("[data-spa-search]")) return;

    event.preventDefault();
    const url = new URL(form.action || window.location.href, window.location.origin);
    const formData = new FormData(form);
    const query = new URLSearchParams();
    for (const [key, value] of formData.entries()) {
      if (typeof value === "string" && value.trim() !== "") {
        query.set(key, value);
      }
    }
    url.search = query.toString();
    const pagination = form.closest("[data-pagination-scope]");
    const scrollTargetSelector = pagination
      ? `[data-pagination-scope="${pagination.getAttribute("data-pagination-scope")}"]`
      : null;
    navigate(url.toString(), { scrollTargetSelector });
  });

  window.addEventListener("popstate", () => {
    closeFullscreenPreview();
    navigate(window.location.href, { replace: true });
  });

  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      closeFullscreenPreview();
    }
  });
})();
