/** One managed dev server, as `/api/servers` reports it. */
export type Server = {
  name: string;
  description: string;
  port: number;
  url: string;
  up: boolean;
};

const el = (tag: string, className: string, text = ""): HTMLElement => {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
};

const card = (server: Server): HTMLAnchorElement => {
  const link = document.createElement("a");
  link.className = server.up ? "server up" : "server down";
  link.href = server.url;
  link.setAttribute(
    "aria-label",
    `${server.name}, port ${server.port}, ${server.up ? "running" : "not running"}`,
  );

  const body = el("span", "body");
  body.append(el("span", "name", server.name));
  if (server.description) body.append(el("span", "description", server.description));

  link.append(el("span", "dot"), body, el("span", "port", String(server.port)));
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
