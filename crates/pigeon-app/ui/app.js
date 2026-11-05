// Pigeon panel. It only shows things and reports choices; the Rust side holds
// the link, the contacts, and decides what gets opened.
//
// Anything that came from outside (names, handles, links, messages) is put on
// the page with textContent, never as HTML.
"use strict";

const TAURI = window.__TAURI__;
const DEMO = !TAURI;

const TARGETS = [
  { id: "whatsapp", name: "WhatsApp", color: "#1fb855", people: true, placeholder: "Phone with country code" },
  { id: "telegram", name: "Telegram", color: "#229ed9", people: true, placeholder: "Username, like @sam_k" },
  { id: "instagram", name: "Instagram", color: "#d6336c", people: true, placeholder: "Username or profile link" },
  { id: "messenger", name: "Messenger", color: "#0a7cff", people: true, placeholder: "Facebook username or link" },
  { id: "discord", name: "Discord", color: "#5865f2", people: false },
  { id: "email", name: "Email", color: "#4f6079", people: true, placeholder: "Email address" },
  { id: "x", name: "Post on X", color: "#14171a", people: false },
  { id: "copy", name: "Copy link", color: "#4f6079", people: false },
];

const PLATFORM_COLORS = {
  youtube: "#ff0033",
  instagram: "#d6336c",
  tiktok: "#111111",
  x: "#14171a",
  facebook: "#1877f2",
  reddit: "#ff4500",
  threads: "#111111",
  snapchat: "#f2c500",
  web: "#4f6079",
};

const AVATAR_COLORS = ["#3b82f6", "#16a34a", "#ea8a0c", "#e0485a", "#0891b2", "#db2777", "#4f6079", "#7c5cd6"];

const panel = document.getElementById("panel");
const glass = document.getElementById("glass");
const ring = document.getElementById("ring");
const center = document.getElementById("center");
const stage = document.getElementById("stage");

const api = DEMO ? demoApi() : {
  invoke: (cmd, args) => TAURI.core.invoke(cmd, args),
  listen: (name, fn) => TAURI.event.listen(name, (e) => fn(e.payload)),
};

let s = freshState();
let timers = [];
let ringGeneration = 0;

function freshState() {
  return {
    open: false,
    mode: "share",
    view: "main", // main | people | form | confirm | done
    link: null, // { found, platform, label, short, message }
    target: null,
    contacts: [],
    items: [],
    busy: false,
    openedAt: 0,
    hotkey: "Ctrl+Shift+Space",
  };
}

function later(ms, fn) {
  timers.push(setTimeout(fn, ms));
}

function clearTimers() {
  timers.forEach(clearTimeout);
  timers = [];
}

// ---------- Small DOM helpers ----------

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

function bigIcon(name, color) {
  const wrap = el("div", "big-icon");
  if (color) wrap.style.setProperty("--c", color);
  wrap.append(icon(name));
  return wrap;
}

function setCenter(nodes, wide = false) {
  center.classList.toggle("wide", wide);
  center.classList.remove("done");
  center.replaceChildren(...nodes);
}

function initials(name) {
  // First letter or digit of the first two words, skipping symbols.
  const firsts = name
    .split(/\s+/)
    .map((word) => (word.match(/[\p{L}\p{N}]/u) || [])[0])
    .filter(Boolean);
  return (firsts.slice(0, 2).join("") || "?").toUpperCase();
}

function avatarColor(name) {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.codePointAt(0)) >>> 0;
  return AVATAR_COLORS[h % AVATAR_COLORS.length];
}

// ---------- Ring ----------

// items: [{ label, icon?, avatar?, color, badge?, disabled?, pick, remove? }]
function renderRing(items, animate = true) {
  s.items = items;
  const generation = ++ringGeneration;
  const old = [...ring.children];
  const build = () => {
    if (generation !== ringGeneration) return;
    ring.replaceChildren();
    items.forEach((item, i) => {
      const slot = el("div", "slot" + (animate ? " in" : ""));
      slot.style.setProperty("--a", `${(360 / items.length) * i}deg`);
      slot.style.setProperty("--delay", `${i * 22}ms`);

      const button = el("button", "item");
      button.type = "button";
      button.setAttribute("aria-label", item.label);
      button.style.setProperty("--c", item.color);
      if (item.avatar) {
        const face = el("span", "avatar", initials(item.avatar));
        face.style.setProperty("--c", avatarColor(item.avatar));
        button.append(face);
        if (item.badge) {
          const badge = el("span", "badge");
          badge.style.setProperty("--t", item.color);
          badge.append(icon(item.badge));
          button.append(badge);
        }
      } else {
        button.append(icon(item.icon));
      }
      if (i < 9) button.append(el("span", "key", String(i + 1)));
      setDisabled(button, item.disabled);

      button.addEventListener("click", () => {
        if (button.getAttribute("aria-disabled") !== "true") item.pick();
      });
      button.addEventListener("mouseenter", () => showHint(item));
      button.addEventListener("focus", () => showHint(item));
      button.addEventListener("mouseleave", () => showHint(null));
      button.addEventListener("blur", () => showHint(null));
      if (item.remove) {
        button.addEventListener("contextmenu", (e) => {
          e.preventDefault();
          item.remove();
        });
      }
      slot.append(button);
      ring.append(slot);
    });
    if (animate) {
      requestAnimationFrame(() =>
        requestAnimationFrame(() => ring.querySelectorAll(".slot.in").forEach((n) => n.classList.remove("in"))),
      );
    }
  };
  if (animate && old.length) {
    old.forEach((n) => n.classList.add("out"));
    later(150, build);
  } else {
    build();
  }
}

function setDisabled(button, disabled) {
  button.setAttribute("aria-disabled", disabled ? "true" : "false");
  button.tabIndex = disabled ? -1 : 0;
}

function showHint(item) {
  const hint = center.querySelector(".hint");
  if (!hint) return;
  if (item && !item.disabled) hint.textContent = item.hintText || item.label;
  else hint.textContent = hint.dataset.idle || "";
}

// ---------- Views ----------

function linkReady() {
  return !!(s.link && s.link.found);
}

function viewMain(animate = true) {
  s.view = "main";
  s.target = null;
  ring.classList.remove("dim");
  const items = TARGETS.map((t) => ({
    label: t.name,
    hintText: t.id === "copy" ? "Copy the link" : t.id === "x" ? "Post it on X" : `Send with ${t.name}`,
    icon: t.id,
    color: t.color,
    disabled: !linkReady(),
    pick: () => pickTarget(t),
  }));
  if (animate) renderRing(items, true);
  else {
    s.items = items;
    ring.querySelectorAll(".item").forEach((b) => setDisabled(b, !linkReady()));
  }
  centerMain();
}

function centerMain() {
  if (!s.link) {
    setCenter([el("div", "spinner"), el("p", "sub", "Finding the post…")]);
    return;
  }
  if (!s.link.found) {
    setCenter([bigIcon("alert", "#c2410c"), el("p", "title", "Nothing to send"), el("p", "sub", s.link.message)]);
    return;
  }
  const chip = el("span", "chip");
  const dot = el("span", "dot");
  dot.style.setProperty("--c", PLATFORM_COLORS[s.link.platform] || PLATFORM_COLORS.web);
  chip.append(dot, document.createTextNode(s.link.label));
  const hint = el("p", "hint", "Where should it go?");
  hint.dataset.idle = "Where should it go?";
  setCenter([chip, el("p", "url", s.link.short), hint]);
}

function pickTarget(t) {
  if (!linkReady() || s.busy) return;
  if (t.people) viewPeople(t);
  else send(t, null);
}

function viewPeople(t) {
  s.view = "people";
  s.target = t;
  ring.classList.remove("dim");
  const saved = s.contacts.filter((c) => c.target === t.id).slice(0, 6);
  const items = [
    {
      label: `Choose in ${t.name}`,
      hintText:
        t.id === "email" ? "Write a new email" : t.id === "instagram" ? "Pick who in Instagram" : `Pick a chat in ${t.name}`,
      icon: "anyone",
      color: t.color,
      pick: () => send(t, null),
    },
    ...saved.map((c) => ({
      label: c.name,
      hintText: `Send to ${c.name}`,
      avatar: c.name,
      badge: t.id,
      color: t.color,
      pick: () => send(t, c),
      remove: () => viewConfirmRemove(t, c),
    })),
    { label: "Add someone", hintText: "Save someone new", icon: "add", color: "#4f6079", pick: () => viewForm(t) },
  ];
  renderRing(items, true);

  const back = el("button", "back");
  back.type = "button";
  back.append(icon("back"), document.createTextNode("Back"));
  back.addEventListener("click", () => viewMain());
  const idle = saved.length ? "Who's it for?" : "Add someone, or pick a chat";
  const hint = el("p", "hint", idle);
  hint.dataset.idle = idle;
  setCenter([bigIcon(t.id, t.color), el("p", "title", t.name), hint, back]);
}

function viewForm(t) {
  s.view = "form";
  ring.classList.add("dim");
  const form = el("form", "form");
  form.style.setProperty("--c", t.color);
  const name = el("input", "field");
  name.placeholder = "Name";
  name.maxLength = 40;
  name.autocomplete = "off";
  const handle = el("input", "field");
  handle.placeholder = t.placeholder;
  handle.maxLength = 254;
  handle.autocomplete = "off";
  handle.spellcheck = false;
  if (t.id === "email") handle.type = "email";
  if (t.id === "whatsapp") handle.inputMode = "tel";
  const error = el("p", "error");
  const cancel = el("button", "btn", "Cancel");
  cancel.type = "button";
  cancel.addEventListener("click", () => viewPeople(t));
  const save = el("button", "btn primary", "Save");
  save.type = "submit";
  const row = el("div", "row");
  row.append(cancel, save);
  form.append(name, handle, error, row);
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (s.busy) return;
    s.busy = true;
    try {
      const contact = await api.invoke("add_contact", { target: t.id, name: name.value, handle: handle.value });
      s.contacts.unshift(contact);
      viewPeople(t);
    } catch (err) {
      error.textContent = String(err);
    } finally {
      s.busy = false;
    }
  });
  setCenter([el("p", "title", `New ${t.name} contact`), form], true);
  later(60, () => name.focus());
}

function viewConfirmRemove(t, c) {
  s.view = "confirm";
  ring.classList.add("dim");
  const keep = el("button", "btn", "Keep");
  keep.type = "button";
  keep.addEventListener("click", () => viewPeople(t));
  const remove = el("button", "btn danger", "Remove");
  remove.type = "button";
  remove.addEventListener("click", async () => {
    try {
      await api.invoke("remove_contact", { id: c.id });
      s.contacts = s.contacts.filter((x) => x.id !== c.id);
    } catch (_) {
      // Keep showing them; the list will be right next time it opens.
    }
    viewPeople(t);
  });
  const row = el("div", "row");
  row.append(keep, remove);
  setCenter([el("p", "title", `Remove ${c.name}?`), el("p", "sub", `From your ${t.name} people`), row]);
  later(60, () => keep.focus());
}

async function send(t, contact) {
  if (s.busy) return;
  s.busy = true;
  try {
    const hint = await api.invoke("send_link", { target: t.id, contactId: contact ? contact.id : null });
    s.view = "done";
    ring.classList.add("dim");
    // Pigeon opens the chat; the person still presses Enter, so don't say "sent".
    const app = t.id === "x" ? "X" : t.name;
    const title = t.id === "copy" ? "Copied" : contact ? `Opening ${contact.name}` : `Opening ${app}`;
    setCenter([bigIcon("check"), el("p", "title", title), el("p", "sub", hint)]);
    center.classList.add("done");
    // Copying leaves you where you were; the other choices open an app.
    later(t.id === "copy" ? 800 : 1300, () => close(t.id === "copy"));
  } catch (err) {
    setCenter([bigIcon("alert", "#c2410c"), el("p", "title", "Couldn't send"), el("p", "sub", String(err))]);
    later(2200, () => (s.target ? viewPeople(s.target) : viewMain()));
  } finally {
    s.busy = false;
  }
}

function viewWelcome() {
  s.view = "welcome";
  renderRing(
    TARGETS.map((t) => ({ label: t.name, icon: t.id, color: t.color, disabled: true, pick() {} })),
    true,
  );
  setCenter([bigIcon("spark", "#2f7cf6"), el("p", "title", "Pigeon is ready"), el("p", "sub", `Press ${s.hotkey} on any reel or post`)]);
  later(3600, () => close(true));
}

// ---------- Open and close ----------

const BACKDROP = /^data:image\/bmp;base64,[A-Za-z0-9+/=]+$/;

async function onOpen(payload) {
  clearTimers();
  s = freshState();
  s.open = true;
  s.mode = payload.mode;
  s.hotkey = payload.hotkey || s.hotkey;
  s.openedAt = Date.now();
  glass.style.backgroundImage =
    payload.background && BACKDROP.test(payload.background) ? `url("${payload.background}")` : "";
  ring.replaceChildren();
  panel.classList.remove("closing", "keys");
  panel.setAttribute("aria-hidden", "false");
  // Restart the opening animation from the closed pose.
  panel.classList.remove("open");
  void panel.offsetWidth;
  panel.classList.add("open");

  try {
    s.contacts = await api.invoke("list_contacts");
  } catch (_) {
    s.contacts = [];
  }
  if (s.mode === "welcome") viewWelcome();
  else viewMain(true);
}

function onLink(payload) {
  if (!s.open || s.mode !== "share") return;
  s.link = payload;
  if (s.view === "main") viewMain(false);
}

function close(restoreFocus) {
  if (!s.open) return;
  s.open = false;
  clearTimers();
  panel.classList.add("closing");
  panel.classList.remove("open");
  panel.setAttribute("aria-hidden", "true");
  setTimeout(() => {
    if (s.open) return; // reopened meanwhile
    api.invoke("close_panel", { restoreFocus }).catch(() => {});
    ring.replaceChildren();
    center.replaceChildren();
    glass.style.backgroundImage = "";
    panel.classList.remove("closing");
  }, 170);
}

function back() {
  if (s.view === "form" || s.view === "confirm") viewPeople(s.target);
  else if (s.view === "people") viewMain();
  else close(true);
}

// ---------- Keyboard and mouse ----------

document.addEventListener("keydown", (e) => {
  // Never let the page reload or open browser tools.
  if (e.key === "F5" || e.key === "F12" || ((e.ctrlKey || e.metaKey) && /^[rRpPfFgGuU]$/.test(e.key))) {
    e.preventDefault();
  }
  if (!s.open) return;
  const typing = e.target instanceof HTMLInputElement;
  if (e.key === "Escape") {
    e.preventDefault();
    back();
    return;
  }
  if (typing) return;
  if (e.key === "Tab" || /^[1-9]$/.test(e.key)) panel.classList.add("keys");
  if (e.key === "Backspace" && s.view !== "main") {
    e.preventDefault();
    back();
    return;
  }
  if (/^[1-9]$/.test(e.key) && (s.view === "main" || s.view === "people")) {
    const item = s.items[Number(e.key) - 1];
    if (item && !item.disabled) {
      e.preventDefault();
      item.pick();
    }
  }
});

document.addEventListener("contextmenu", (e) => {
  if (!(e.target instanceof HTMLInputElement)) e.preventDefault();
});

// Clicking the empty corners of the window (outside the circle) closes it.
stage.addEventListener("mousedown", (e) => {
  if (e.target === stage) close(true);
});

window.addEventListener("blur", () => {
  if (s.open && Date.now() - s.openedAt > 400) close(false);
});

async function start() {
  await api.listen("panel:open", onOpen);
  await api.listen("panel:link", onLink);
  await api.listen("panel:close", () => close(true));
  await api.invoke("panel_ready");
}

start();

// ---------- Demo mode (opening ui/index.html in a normal browser) ----------

function demoApi() {
  document.documentElement.classList.add("demo");
  const handlers = {};
  let contacts = [
    { id: "d1", target: "whatsapp", name: "Sam Khan", handle: "97333123456" },
    { id: "d2", target: "whatsapp", name: "Mum", handle: "97339876543" },
    { id: "d3", target: "whatsapp", name: "Layla Ahmed", handle: "97336655443" },
    { id: "d4", target: "telegram", name: "Omar", handle: "omar_k" },
  ];
  const params = new URLSearchParams(location.search);
  const reopen = () => {
    handlers["panel:open"]({ mode: params.get("mode") || "share", background: null, hotkey: "Ctrl+Shift+Space" });
    if (params.get("mode") === "welcome") return;
    setTimeout(() => {
      const missing = params.get("link") === "missing";
      handlers["panel:link"](
        missing
          ? { found: false, platform: "", label: "", short: "", message: "Couldn't pick out one post. Open it, or copy its link." }
          : { found: true, platform: "instagram", label: "Instagram reel", short: "instagram.com/reel/C8xYz12AbCd", message: "" },
      );
    }, Number(params.get("delay") || 700));
  };
  return {
    listen: async (name, fn) => {
      handlers[name] = fn;
    },
    invoke: async (cmd, args) => {
      switch (cmd) {
        case "panel_ready":
          setTimeout(reopen, 150);
          return;
        case "list_contacts":
          return contacts.slice();
        case "add_contact": {
          if (!args.name.trim()) throw "Add a name.";
          if (!args.handle.trim()) throw "Add their number or username.";
          const c = { id: String(Date.now()), target: args.target, name: args.name.trim(), handle: args.handle.trim() };
          contacts.unshift(c);
          return c;
        }
        case "remove_contact":
          contacts = contacts.filter((c) => c.id !== args.id);
          return;
        case "send_link":
          return args.target === "copy" ? "Link copied." : "Opening WhatsApp. Press Enter to send.";
        case "close_panel":
          setTimeout(reopen, 900);
          return;
      }
    },
  };
}
