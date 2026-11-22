import v5Page from "./v5.html";

const server = Bun.serve({
  port: Number(process.env.PORT ?? 3000),
  routes: {
    "/": v5Page,
    "/v5": v5Page,
  },
  development: {
    hmr: true,
    console: true,
  },
});

console.log(`POC server ready on ${server.url}`);
console.log(`  v5: ${server.url}v5  (also /)`);
