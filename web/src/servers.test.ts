import { beforeEach, expect, test } from "vitest";
import { renderServers, type Server } from "./servers";

const server = (overrides: Partial<Server> = {}): Server => ({
  name: "api",
  description: "API dev server",
  port: 8790,
  url: "http://10.99.99.1:8790/",
  up: true,
  ...overrides,
});

let container: HTMLElement;
beforeEach(() => {
  container = document.createElement("main");
});

test("renders one linked card per server, marked up or down", () => {
  renderServers(container, [server(), server({ name: "docs", port: 8791, up: false })]);

  const links = container.querySelectorAll("a.server");
  expect(links.length).toBe(2);
  expect(links[0].getAttribute("href")).toBe("http://10.99.99.1:8790/");
  expect(links[0].className).toBe("server up");
  expect(links[0].querySelector(".name")?.textContent).toBe("api");
  expect(links[0].querySelector(".description")?.textContent).toBe("API dev server");
  expect(links[1].className).toBe("server down");
  expect(links[1].getAttribute("aria-label")).toBe("docs, port 8791, not running");
});

test("shows the repo icon when there is one, and the placeholder when there is not", () => {
  renderServers(container, [server({ icon: "/api/icon/api" }), server({ name: "docs" })]);

  const [withIcon, without] = container.querySelectorAll("a.server");
  expect(withIcon.querySelector("img.icon")?.getAttribute("src")).toBe("/api/icon/api");
  expect(withIcon.querySelector("img.icon")?.getAttribute("alt")).toBe("");
  // Exactly one mark in the icon slot, either way.
  expect(withIcon.querySelector(".placeholder")).toBe(null);
  expect(without.querySelector("img.icon")).toBe(null);
  expect(without.querySelector("svg.placeholder")?.getAttribute("aria-hidden")).toBe("true");
});

test("a broken icon falls back to the placeholder", () => {
  renderServers(container, [server({ icon: "/api/icon/gone" })]);
  const icon = container.querySelector("img.icon") as HTMLImageElement;

  icon.dispatchEvent(new Event("error"));

  expect(container.querySelector("img.icon")).toBe(null);
  expect(container.querySelectorAll("svg.placeholder").length).toBe(1);
});

test("names are set as text, never parsed as markup", () => {
  renderServers(container, [server({ name: "<img src=x onerror=alert(1)>" })]);

  expect(container.querySelector("img")).toBe(null);
  expect(container.querySelector(".name")?.textContent).toBe("<img src=x onerror=alert(1)>");
});

test("replaces previous contents and shows a notice when there is nothing to list", () => {
  renderServers(container, [server()]);
  renderServers(container, []);

  expect(container.querySelectorAll("a.server").length).toBe(0);
  expect(container.querySelector(".notice")?.textContent).toContain("PORT");
});
