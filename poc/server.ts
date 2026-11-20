import v1Page from "./v1.html";
import v2Page from "./v2.html";
import v3Page from "./v3.html";

const server = Bun.serve({
  port: Number(process.env.PORT ?? 3000),
  routes: {
    "/": v3Page,
    "/v1": v1Page,
    "/v2": v2Page,
    "/v3": v3Page,
    "/before": v1Page,
  },
  development: {
    hmr: true,
    console: true,
  },
});

console.log(`POC server ready on ${server.url}`);
console.log(`  v1 (original):   ${server.url}v1`);
console.log(`  v2 (navy dark):  ${server.url}v2`);
console.log(`  v3 (orb tone):   ${server.url}v3  (also /)`);
