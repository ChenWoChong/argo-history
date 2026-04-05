(() => {
  const toolbarSelector = "#toolbar-shell";
  const contentSelector = "#content-root";

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
      window.scrollTo({ top: 0, behavior: "instant" });
    } catch (_error) {
      window.location.href = nextUrl;
    } finally {
      document.body.classList.remove("is-loading");
    }
  }

  document.addEventListener("click", (event) => {
    const anchor = event.target.closest("a[href]");
    if (!shouldHandleLink(anchor, event)) {
      return;
    }
    event.preventDefault();
    navigate(anchor.href);
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
    navigate(url.toString());
  });

  window.addEventListener("popstate", () => {
    navigate(window.location.href, { replace: true });
  });
})();
