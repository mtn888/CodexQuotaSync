export interface ServerEndpointFields {
  host: string;
  port: string;
}

export function splitServerUrl(serverUrl: string): ServerEndpointFields {
  const value = serverUrl.trim();
  if (!value) return { host: "", port: "" };

  try {
    const url = new URL(value.includes("://") ? value : `http://${value}`);
    return {
      host: `${url.protocol}//${url.hostname}`,
      port: url.port,
    };
  } catch {
    return { host: value, port: "" };
  }
}

export function composeServerUrl(hostValue: string, portValue: string): string {
  const host = hostValue.trim();
  const port = portValue.trim();
  if (!host) {
    if (port) throw new Error("请先填写服务器地址，再填写端口。");
    return "";
  }

  let url: URL;
  try {
    url = new URL(host.includes("://") ? host : `http://${host}`);
  } catch {
    throw new Error("服务器地址格式无效。");
  }
  if (url.protocol !== "http:") {
    throw new Error("服务器地址必须使用 http://。");
  }
  if (url.username || url.password) {
    throw new Error("服务器地址不能包含用户名或密码。");
  }
  if (url.pathname !== "/" || url.search || url.hash) {
    throw new Error("服务器地址不能包含路径、查询参数或锚点。");
  }
  if (port) {
    if (!/^\d+$/.test(port)) throw new Error("服务器端口必须是 1 到 65535 的数字。");
    const numericPort = Number(port);
    if (!Number.isInteger(numericPort) || numericPort < 1 || numericPort > 65_535) {
      throw new Error("服务器端口必须是 1 到 65535 的数字。");
    }
    url.port = port;
  }
  return url.origin;
}
