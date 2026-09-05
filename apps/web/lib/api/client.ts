export class ApiError extends Error {
  readonly status: number;
  readonly code: string | null;
  readonly body: unknown;

  constructor(
    status: number,
    code: string | null,
    message: string,
    body: unknown = undefined,
  ) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.body = body;
  }
}

export class InvalidServerDataError extends Error {
  readonly code = "invalid_server_data";

  constructor(message: string) {
    super(message);
    this.name = "InvalidServerDataError";
  }
}

function errorCode(value: unknown): string | null {
  if (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    "error" in value &&
    typeof value.error === "string"
  ) {
    return value.error;
  }
  return null;
}

export async function requestJson(
  path: string,
  init?: RequestInit,
): Promise<unknown> {
  const headers = new Headers(init?.headers);
  if (!headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }

  const response = await fetch(path, {
    credentials: "include",
    ...init,
    headers,
  });
  const bodyText = await response.text();
  let body: unknown = undefined;
  if (bodyText) {
    try {
      body = JSON.parse(bodyText) as unknown;
    } catch {
      throw new ApiError(
        response.status,
        "invalid_json",
        "Server returned invalid JSON",
      );
    }
  }

  if (!response.ok) {
    const code = errorCode(body);
    throw new ApiError(
      response.status,
      code,
      code ?? `Request failed (${response.status})`,
      body,
    );
  }
  return body;
}
