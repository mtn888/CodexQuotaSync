import { describe, expect, it } from "vitest";
import { composeServerUrl, splitServerUrl } from "./serverEndpoint";

describe("server endpoint settings", () => {
  it("splits and recomposes an HTTP server with a custom port", () => {
    expect(splitServerUrl("http://10.10.10.254:18080")).toEqual({ host: "http://10.10.10.254", port: "18080" });
    expect(composeServerUrl("http://10.10.10.254", "18080")).toBe("http://10.10.10.254:18080");
  });

  it("uses HTTP when an address omits the protocol", () => {
    expect(composeServerUrl("nas.example.com", "8787")).toBe("http://nas.example.com:8787");
  });

  it("rejects invalid ports and non-HTTP URLs", () => {
    expect(() => composeServerUrl("nas.example.com", "70000")).toThrow("1 到 65535");
    expect(() => composeServerUrl("https://nas.example.com", "18080")).toThrow("http://");
  });
});
