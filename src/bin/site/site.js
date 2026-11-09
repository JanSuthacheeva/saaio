"use strict";

const root = document.documentElement;
const dark = window.matchMedia("(prefers-color-scheme: dark)");

document.querySelector(".theme").addEventListener("click", () => {
  const shown = root.dataset.theme || (dark.matches ? "dark" : "light");
  const theme = shown === "dark" ? "light" : "dark";
  root.dataset.theme = theme;
  try {
    localStorage.setItem("theme", theme);
  } catch {
    // Without storage the choice lasts until the next page.
  }
});

// The page list as a drawer on narrow screens.
const nav = document.getElementById("nav");
const menu = document.querySelector(".menu");
function setMenu(open) {
  nav.classList.toggle("open", open);
  menu.setAttribute("aria-expanded", String(open));
}
menu.addEventListener("click", () => setMenu(!nav.classList.contains("open")));
document.querySelector(".scrim").addEventListener("click", () => setMenu(false));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") setMenu(false);
});

// Search: Pagefind's modal, from the header or Cmd/Ctrl+K.
function openSearch() {
  document.querySelector("pagefind-modal")?.open?.();
}
document.querySelectorAll(".search-button, .search-icon").forEach((button) => button.addEventListener("click", openSearch));
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    openSearch();
  }
});
if (!/Mac|iPhone|iPad/.test(navigator.platform)) {
  document.querySelectorAll(".search-button kbd").forEach((kbd) => (kbd.textContent = "Ctrl K"));
}

// Code blocks: a head with the language and a copy button; shell comments dimmed.
const copyIcon = '<svg class="i" viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/></svg>';
const doneIcon = '<svg class="i" viewBox="0 0 24 24"><path d="m5 12 5 5 9-10"/></svg>';
document.querySelectorAll("article pre > code").forEach((code) => {
  const pre = code.parentElement;
  const lang = (code.className.match(/language-(\S+)/) || [])[1] || "text";
  const box = document.createElement("div");
  box.className = "code";
  box.innerHTML = `<div class="code-head"><span class="code-lang">${lang}</span><button class="copy" type="button" aria-label="Copy code"><span class="idle">${copyIcon} Copy</span><span class="done">${doneIcon} Copied</span></button></div>`;
  pre.replaceWith(box);
  box.append(pre);
  if (["sh", "bash", "toml", "text"].includes(lang)) {
    code.innerHTML = code.innerHTML.replace(/(^|\s)(#(?!!).*)$/gm, '$1<span class="c">$2</span>');
  }
  const button = box.querySelector(".copy");
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(code.textContent);
    } catch {
      const range = document.createRange();
      range.selectNodeContents(code);
      getSelection().removeAllRanges();
      getSelection().addRange(range);
    }
    button.classList.add("copied");
    setTimeout(() => button.classList.remove("copied"), 1600);
  });
});

// "On this page" follows the section in view.
const links = [...document.querySelectorAll(".outline a")];
const headings = links.map((link) => document.getElementById(link.hash.slice(1))).filter(Boolean);
function follow() {
  const line = Math.max(parseFloat(getComputedStyle(root).scrollPaddingTop) + 8, innerHeight * 0.25);
  let current = headings[0];
  for (const heading of headings) {
    if (heading.getBoundingClientRect().top <= line) current = heading;
  }
  if (innerHeight + scrollY >= document.documentElement.scrollHeight - 2) current = headings.at(-1);
  links.forEach((link) => link.classList.toggle("active", link.hash === "#" + current?.id));
}
if (headings.length) {
  addEventListener("scroll", follow, { passive: true });
  follow();
}
