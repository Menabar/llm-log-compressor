import { compressLogs } from "../core/compress/compressor.js";

export function registerTools(server: any) {
  server.tool("compress_logs", async (input: any) => {
    return compressLogs(input.logText);
  });

  server.tool("get_log_block", async (input: any) => {
    return {
      message: "not implemented yet"
    };
  });

  server.tool("search_logs", async (input: any) => {
    return {
      message: "not implemented yet"
    };
  });
}