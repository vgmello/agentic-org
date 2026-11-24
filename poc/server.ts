import v5Page from "./v5.html";
import v6Page from "./v6.html";

export function servePrototype(port: number, preview = true) {
  return Bun.serve({
    port,
    hostname: preview ? undefined : "127.0.0.1",
    routes: {
      "/": v6Page,
      "/v5": v5Page,
      "/v6": v6Page,
    },
    development: preview ? { hmr: true, console: true } : false,
  });
}

if (import.meta.main) {
  const server = servePrototype(Number(process.env.PORT ?? 3000));
  console.log(`POC server ready on ${server.url}`);
  console.log(`  v5: ${server.url}v5`);
  console.log(`  v6: ${server.url}v6  (also /)`);
}
