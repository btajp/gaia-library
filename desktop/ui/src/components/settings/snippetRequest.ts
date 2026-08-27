import type { LatestRequest } from "../../lib/latestRequest";
import { mcpConfigSnippet, type ConnectionSnippet, type Transport } from "../../settingsApi";

export const snippetKey = (name: string, transport: Transport) => JSON.stringify([name, transport]);

export function requestSnippet(request: LatestRequest<ConnectionSnippet>, name: string, transport: Transport): Promise<void> {
  const key = snippetKey(name, transport);
  const snapshot = request.getSnapshot();
  if (snapshot.key === key && snapshot.status === "loading") return Promise.resolve();
  return request.run(key, () => mcpConfigSnippet(name, transport));
}
