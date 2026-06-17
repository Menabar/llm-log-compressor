import { createServer } from "@modelcontextprotocol/sdk/server/index.js";
import { registerTools } from "./tools.js";

export function startServer() {
  const server = createServer({
    name: "loglens-mcp",
    version: "0.1.0"
  });

  registerTools(server);

  server.listen(3000, () => {
    console.log("LogLens MCP running on port 3000");
  });
}