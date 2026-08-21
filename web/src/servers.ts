/** One managed dev server, as `/api/servers` reports it. */
export type Server = {
  name: string;
  description: string;
  port: number;
  url: string;
  up: boolean;
  icon?: string | null;
};

const el = (tag: string, className: string, text = ""): HTMLElement => {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
};

/** Lucide `scan`, inlined rather than depended on. Strokes take currentColor. */
const SCAN =
  '<svg class="icon placeholder" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7V5a2 2 0 0 1 2-2h2"/><path d="M17 3h2a2 2 0 0 1 2 2v2"/><path d="M21 17v2a2 2 0 0 1-2 2h-2"/><path d="M7 21H5a2 2 0 0 1-2-2v-2"/></svg>';

/** The stand-in for a repo with no favicon — the icon slot is never empty. */
const placeholder = (): Element => {
  const host = document.createElement("span");
  // A constant: nothing a server declares reaches this markup.
  host.innerHTML = SCAN;
  return host.firstElementChild as Element;
};

const card = (server: Server): HTMLAnchorElement => {
  const link = document.createElement("a");
  link.className = server.up ? "server up" : "server down";
  link.href = server.url;
  link.setAttribute(
    "aria-label",
    `${server.name}, port ${server.port}, ${server.up ? "running" : "not running"}`,
  );

  if (server.icon) {
    const icon = document.createElement("img");
    icon.className = "icon";
    icon.src = server.icon;
    icon.alt = "";
    icon.addEventListener("error", () => icon.replaceWith(placeholder()));
    link.append(icon);
  } else {
    link.append(placeholder());
  }

  const body = el("span", "body");
  body.append(el("span", "name", server.name));
  if (server.description) body.append(el("span", "description", server.description));

  link.append(body, el("span", "port", String(server.port)));
  return link;
};

/** Replaces the contents of `container` with one card per server. */
export const renderServers = (container: HTMLElement, servers: Server[]): void => {
  if (servers.length === 0) {
    container.replaceChildren(
      el("p", "notice", "No process in process-compose.yaml declares a PORT."),
    );
    return;
  }
  container.replaceChildren(...servers.map(card));
};
